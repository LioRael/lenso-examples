use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{fmt, path::PathBuf};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgBinding {
    pub connection_uri_file: PathBuf,
    pub schema: String,
}

#[derive(Clone)]
pub struct PgStateHandle {
    pool: PgPool,
    schema: String,
}

impl fmt::Debug for PgStateHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PgStateHandle")
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub label: String,
    pub value: i64,
    pub revision: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub snapshot: Snapshot,
    pub receipt_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    InvalidBinding,
    SetupRequired,
    Unavailable,
    InvalidInput,
    StaleRevision,
    IdempotencyConflict,
    UnknownCommit,
}

// Schema identifiers are Host-owned, ASCII-validated here, and never supplied by a request.
// Every domain value remains a SQL bind parameter.
fn valid_schema(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_lowercase() || (index != 0 && byte.is_ascii_digit())
        })
}

fn intent(value: i64, revision: i64, key: &str) -> Result<String, Failure> {
    if value.unsigned_abs() > 9_007_199_254_740_991
        || !(0..9_007_199_254_740_991).contains(&revision)
        || key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Failure::InvalidInput);
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("ops-state.v1\n{revision}\n{value}").as_bytes())
    ))
}

impl PgStateHandle {
    pub fn open(binding: PgBinding) -> Result<Self, Failure> {
        if !valid_schema(&binding.schema) || !binding.connection_uri_file.is_absolute() {
            return Err(Failure::InvalidBinding);
        }
        let uri = std::fs::read_to_string(binding.connection_uri_file)
            .map_err(|_| Failure::InvalidBinding)?;
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_lazy(uri.trim())
            .map_err(|_| Failure::InvalidBinding)?;
        Ok(Self {
            pool,
            schema: binding.schema,
        })
    }

    pub async fn readiness(&self) -> Result<(), Failure> {
        let query = format!(
            "SELECT version FROM {}.schema_version WHERE singleton = true",
            self.schema
        );
        let version: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| match error {
                sqlx::Error::Database(database) if database.code().as_deref() == Some("42P01") => {
                    Failure::SetupRequired
                }
                _ => Failure::Unavailable,
            })?;
        if version != Some(1) {
            return Err(Failure::SetupRequired);
        }
        self.read().await?;
        Ok(())
    }

    pub async fn read(&self) -> Result<Snapshot, Failure> {
        let query = format!(
            "SELECT label, value, revision FROM {}.state WHERE singleton = true",
            self.schema
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(query))
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| Failure::Unavailable)?
            .ok_or(Failure::SetupRequired)?;
        Ok(Snapshot {
            label: row.get("label"),
            value: row.get("value"),
            revision: row.get("revision"),
        })
    }

    pub async fn receipt(&self, key: &str) -> Result<Option<Receipt>, Failure> {
        intent(0, 0, key)?;
        let query = format!(
            "SELECT label, value, revision, receipt_id FROM {}.receipts WHERE idempotency_key = $1",
            self.schema
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| Failure::Unavailable)?;
        Ok(row.map(|row| Receipt {
            snapshot: Snapshot {
                label: row.get("label"),
                value: row.get("value"),
                revision: row.get("revision"),
            },
            receipt_id: row.get("receipt_id"),
        }))
    }

    pub async fn update(
        &self,
        value: i64,
        expected_revision: i64,
        key: &str,
    ) -> Result<Receipt, Failure> {
        let digest = intent(value, expected_revision, key)?;
        let mut transaction = self.pool.begin().await.map_err(|_| Failure::Unavailable)?;
        let query = format!(
            "SELECT label, value, revision FROM {}.state WHERE singleton = true FOR UPDATE",
            self.schema
        );
        let current = sqlx::query(sqlx::AssertSqlSafe(query))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| Failure::Unavailable)?
            .ok_or(Failure::SetupRequired)?;
        let query = format!(
            "SELECT intent_digest, label, value, revision, receipt_id FROM {}.receipts WHERE idempotency_key = $1",
            self.schema
        );
        if let Some(prior) = sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|_| Failure::Unavailable)?
        {
            if prior.get::<String, _>("intent_digest") != digest {
                return Err(Failure::IdempotencyConflict);
            }
            return Ok(Receipt {
                snapshot: Snapshot {
                    label: prior.get("label"),
                    value: prior.get("value"),
                    revision: prior.get("revision"),
                },
                receipt_id: prior.get("receipt_id"),
            });
        }
        let revision: i64 = current.get("revision");
        if revision != expected_revision {
            return Err(Failure::StaleRevision);
        }
        let revision = revision.checked_add(1).ok_or(Failure::Unavailable)?;
        let label: String = current.get("label");
        let receipt_id = format!("{label}:{revision}:{digest}");
        let query = format!(
            "UPDATE {}.state SET value = $1, revision = $2 WHERE singleton = true",
            self.schema
        );
        sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(value)
            .bind(revision)
            .execute(&mut *transaction)
            .await
            .map_err(|_| Failure::Unavailable)?;
        let query = format!(
            "INSERT INTO {}.receipts (idempotency_key,intent_digest,label,value,revision,receipt_id) VALUES ($1,$2,$3,$4,$5,$6)",
            self.schema
        );
        sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(key)
            .bind(&digest)
            .bind(&label)
            .bind(value)
            .bind(revision)
            .bind(&receipt_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| Failure::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| Failure::UnknownCommit)?;
        Ok(Receipt {
            snapshot: Snapshot {
                label,
                value,
                revision,
            },
            receipt_id,
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests;
