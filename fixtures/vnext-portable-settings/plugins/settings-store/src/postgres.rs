use futures::future::LocalBoxFuture;
use lenso_kernel::RuntimeFailure;
use std::{future::Future, time::Duration};
use tokio_postgres::{Client, NoTls};

use crate::{
    ChangeError, ChangeRequest, ChangeResponse, ChangeResult, Persistence, ReadError, ReadRequest,
    ReadResponse, ReadResult, decide, failure, valid_change, valid_principal,
};

/// Only the native Host and the provider's loopback persistence service hold this.
#[derive(Clone)]
pub struct Postgres {
    url: String,
}

impl std::fmt::Debug for Postgres {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Postgres { connection: redacted }")
    }
}

impl Postgres {
    pub fn new(url: String) -> Self {
        Self { url }
    }

    async fn run<T, F, Fut>(&self, operation: F) -> Result<T, RuntimeFailure>
    where
        F: FnOnce(Client) -> Fut,
        Fut: Future<Output = Result<T, RuntimeFailure>>,
    {
        // Connection driving belongs to this operation future, including on cancellation.
        tokio::time::timeout(Duration::from_secs(2), async {
            let (client, connection) = tokio_postgres::connect(&self.url, NoTls)
                .await
                .map_err(|_| failure("settings database unavailable"))?;
            tokio::select! {
                result = operation(client) => result,
                _ = connection => Err(failure("settings database connection closed")),
            }
        })
        .await
        .map_err(|_| failure("settings database operation timed out"))?
    }

    /// Called only by the disposable-cluster harness, not implicitly on requests.
    pub async fn initialize(&self) -> Result<(), RuntimeFailure> {
        self.run(|client| async move {
            client
                .batch_execute(include_str!("schema.sql"))
                .await
                .map_err(|_| failure("settings schema initialization failed"))
        })
        .await
    }
}

impl Persistence for Postgres {
    fn read(&self, request: ReadRequest) -> LocalBoxFuture<'static, ReadResult> {
        let store = self.clone();
        Box::pin(async move {
            if !valid_principal(&request.principal) {
                return Ok(Err(ReadError::Invalid));
            }
            store
                .run(|client| async move {
                    let row = client
                        .query_opt(
                            "SELECT revision, value FROM portable_settings WHERE principal = $1",
                            &[&request.principal],
                        )
                        .await
                        .map_err(|_| failure("settings read failed"))?;
                    Ok(Ok(match row {
                        Some(row) => ReadResponse {
                            revision: row.get(0),
                            value: row.get(1),
                        },
                        None => ReadResponse {
                            revision: 0,
                            value: String::new(),
                        },
                    }))
                })
                .await
        })
    }

    fn change(&self, request: ChangeRequest) -> LocalBoxFuture<'static, ChangeResult> {
        let store = self.clone();
        Box::pin(async move {
            if !valid_change(&request) {
                return Ok(Err(ChangeError::Invalid));
            }
            store.run(|mut client| async move {
            let transaction = client.transaction().await.map_err(db_error)?;
            transaction.execute(
                "INSERT INTO portable_settings (principal, revision, value) VALUES ($1, 0, '') ON CONFLICT DO NOTHING",
                &[&request.principal],
            ).await.map_err(db_error)?;
            // Every mutation and receipt lookup is serialized by the same owner row.
            let row = transaction
                .query_one(
                    "SELECT revision, value FROM portable_settings WHERE principal = $1 FOR UPDATE",
                    &[&request.principal],
                )
                .await
                .map_err(db_error)?;
            let current = ReadResponse {
                revision: row.get(0),
                value: row.get(1),
            };
            let receipt = transaction.query_opt(
                "SELECT request, response FROM portable_settings_receipts WHERE principal = $1 AND idempotency_key = $2",
                &[&request.principal, &request.idempotency_key],
            ).await.map_err(db_error)?;
            let previous = receipt
                .as_ref()
                .map(|row| {
                    let original =
                        serde_json::from_value::<ChangeRequest>(row.get(0)).map_err(failure)?;
                    let response =
                        serde_json::from_value::<ChangeResponse>(row.get(1)).map_err(failure)?;
                    Ok::<_, RuntimeFailure>((original, response))
                })
                .transpose()?;
            let response = match decide(&current, previous.as_ref().map(|(a, b)| (a, b)), &request)
            {
                Ok(response) => response,
                Err(error) => return Ok(Err(error)),
            };
            if previous.is_none() {
                transaction.execute(
                    "UPDATE portable_settings SET revision = $2, value = $3 WHERE principal = $1",
                    &[&request.principal, &response.revision, &response.value],
                ).await.map_err(db_error)?;
                transaction.execute(
                    "INSERT INTO portable_settings_receipts (principal, idempotency_key, request, response) VALUES ($1, $2, $3, $4)",
                    &[&request.principal, &request.idempotency_key,
                        &serde_json::to_value(&request).map_err(failure)?,
                        &serde_json::to_value(&response).map_err(failure)?],
                ).await.map_err(db_error)?;
            }
            transaction.commit().await.map_err(db_error)?;
            Ok(Ok(response))
            }).await
        })
    }
}

fn db_error(_: tokio_postgres::Error) -> RuntimeFailure {
    failure("settings transaction failed")
}
