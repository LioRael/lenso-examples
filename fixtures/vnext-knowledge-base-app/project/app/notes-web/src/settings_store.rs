use sqlx::{PgPool, Row};

use crate::settings_core::{
    BusinessSettings, DEFAULT_EXCERPT_LIMIT, SettingsCommand, UpdateBusinessSettings,
    prepare_settings_command,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsStoreOutcome {
    Applied(BusinessSettings),
    StaleRevision,
    IdempotencyConflict,
}

pub async fn get_or_create_settings(
    database: &PgPool,
    owner_id: &str,
) -> Result<BusinessSettings, sqlx::Error> {
    sqlx::query(
        "INSERT INTO knowledge_reference.settings (owner_id, revision, excerpt_limit) \
         VALUES ($1, 1, $2) ON CONFLICT (owner_id) DO NOTHING",
    )
    .bind(owner_id)
    .bind(DEFAULT_EXCERPT_LIMIT)
    .execute(database)
    .await?;
    let row = sqlx::query(
        "SELECT revision, excerpt_limit FROM knowledge_reference.settings WHERE owner_id = $1",
    )
    .bind(owner_id)
    .fetch_one(database)
    .await?;
    Ok(BusinessSettings {
        excerpt_limit: row.get("excerpt_limit"),
        revision: row.get("revision"),
    })
}

pub async fn compare_and_set_settings(
    database: &PgPool,
    owner_id: &str,
    command: &SettingsCommand,
) -> Result<SettingsStoreOutcome, sqlx::Error> {
    let expected = prepare_settings_command(
        UpdateBusinessSettings {
            excerpt_limit: command.excerpt_limit,
            predecessor_revision: command.predecessor_revision,
        },
        command.idempotency_key.as_deref(),
    )
    .map_err(|error| sqlx::Error::Protocol(format!("invalid settings command: {error:?}")))?;
    if expected.payload_sha256 != command.payload_sha256 {
        return Err(sqlx::Error::Protocol(
            "settings command fingerprint does not match its validated payload".to_owned(),
        ));
    }
    let mut transaction = database.begin().await?;
    sqlx::query(
        "INSERT INTO knowledge_reference.settings (owner_id, revision, excerpt_limit) \
         VALUES ($1, 1, $2) ON CONFLICT (owner_id) DO NOTHING",
    )
    .bind(owner_id)
    .bind(DEFAULT_EXCERPT_LIMIT)
    .execute(&mut *transaction)
    .await?;

    if let Some(key) = command.idempotency_key.as_deref() {
        let inserted = sqlx::query(
            "INSERT INTO knowledge_reference.settings_idempotency \
             (owner_id, idempotency_key, payload_sha256, outcome_kind) \
             VALUES ($1, $2, $3, 'pending') ON CONFLICT (owner_id, idempotency_key) DO NOTHING",
        )
        .bind(owner_id)
        .bind(key)
        .bind(&command.payload_sha256)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1;
        if !inserted {
            let row = sqlx::query(
                "SELECT payload_sha256, outcome_kind, result_revision, result_excerpt_limit \
                 FROM knowledge_reference.settings_idempotency \
                 WHERE owner_id = $1 AND idempotency_key = $2 FOR UPDATE",
            )
            .bind(owner_id)
            .bind(key)
            .fetch_one(&mut *transaction)
            .await?;
            let outcome = if row.get::<String, _>("payload_sha256") != command.payload_sha256 {
                SettingsStoreOutcome::IdempotencyConflict
            } else {
                match row.get::<String, _>("outcome_kind").as_str() {
                    "applied" => {
                        let excerpt_limit = row
                            .get::<Option<i64>, _>("result_excerpt_limit")
                            .ok_or_else(|| {
                                sqlx::Error::Protocol(
                                    "committed settings idempotency result lacks excerpt_limit"
                                        .to_owned(),
                                )
                            })?;
                        let revision =
                            row.get::<Option<i64>, _>("result_revision")
                                .ok_or_else(|| {
                                    sqlx::Error::Protocol(
                                        "committed settings idempotency result lacks revision"
                                            .to_owned(),
                                    )
                                })?;
                        SettingsStoreOutcome::Applied(BusinessSettings {
                            excerpt_limit,
                            revision,
                        })
                    }
                    "stale" => SettingsStoreOutcome::StaleRevision,
                    other => {
                        return Err(sqlx::Error::Protocol(format!(
                            "unexpected committed settings idempotency outcome: {other}"
                        )));
                    }
                }
            };
            transaction.commit().await?;
            return Ok(outcome);
        }
    }

    let row = sqlx::query(
        "UPDATE knowledge_reference.settings SET revision = revision + 1, excerpt_limit = $1 \
         WHERE owner_id = $2 AND revision = $3 RETURNING revision, excerpt_limit",
    )
    .bind(command.excerpt_limit)
    .bind(owner_id)
    .bind(command.predecessor_revision)
    .fetch_optional(&mut *transaction)
    .await?;
    let outcome = match row {
        Some(row) => SettingsStoreOutcome::Applied(BusinessSettings {
            excerpt_limit: row.get("excerpt_limit"),
            revision: row.get("revision"),
        }),
        None => SettingsStoreOutcome::StaleRevision,
    };
    if let Some(key) = command.idempotency_key.as_deref() {
        let (kind, revision, excerpt_limit) = match &outcome {
            SettingsStoreOutcome::Applied(settings) => (
                "applied",
                Some(settings.revision),
                Some(settings.excerpt_limit),
            ),
            SettingsStoreOutcome::StaleRevision => ("stale", None, None),
            SettingsStoreOutcome::IdempotencyConflict => unreachable!(),
        };
        let updated = sqlx::query(
            "UPDATE knowledge_reference.settings_idempotency \
             SET outcome_kind = $1, result_revision = $2, result_excerpt_limit = $3 \
             WHERE owner_id = $4 AND idempotency_key = $5",
        )
        .bind(kind)
        .bind(revision)
        .bind(excerpt_limit)
        .bind(owner_id)
        .bind(key)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(sqlx::Error::Protocol(
                "settings idempotency reservation disappeared during CAS".to_owned(),
            ));
        }
    }
    transaction.commit().await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_core::{UpdateBusinessSettings, prepare_settings_command};
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    fn command(limit: i64, revision: i64, key: &str) -> SettingsCommand {
        prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: limit,
                predecessor_revision: revision,
            },
            Some(key),
        )
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires a disposable PostgreSQL database prepared by knowledge-operator setup"]
    async fn settings_cas_and_idempotency_survive_replay_and_concurrency() {
        let database_url = std::env::var("LENSO_KNOWLEDGE_DATABASE_URL")
            .expect("set LENSO_KNOWLEDGE_DATABASE_URL to a disposable PostgreSQL database");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .unwrap();
        let owner_a = format!("settings-a-{}", Uuid::now_v7());
        let owner_b = format!("settings-b-{}", Uuid::now_v7());
        let initial = BusinessSettings {
            excerpt_limit: 96,
            revision: 1,
        };
        assert_eq!(
            get_or_create_settings(&pool, &owner_a).await.unwrap(),
            initial
        );
        assert_eq!(
            get_or_create_settings(&pool, &owner_b).await.unwrap(),
            initial
        );

        let first = command(48, 1, "same-key");
        let applied = SettingsStoreOutcome::Applied(BusinessSettings {
            excerpt_limit: 48,
            revision: 2,
        });
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &first)
                .await
                .unwrap(),
            applied
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &first)
                .await
                .unwrap(),
            applied
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &command(64, 1, "same-key"))
                .await
                .unwrap(),
            SettingsStoreOutcome::IdempotencyConflict
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &command(48, 2, "same-key"))
                .await
                .unwrap(),
            SettingsStoreOutcome::IdempotencyConflict
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &command(64, 1, "stale-key"))
                .await
                .unwrap(),
            SettingsStoreOutcome::StaleRevision
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &command(64, 1, "stale-key"))
                .await
                .unwrap(),
            SettingsStoreOutcome::StaleRevision
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_b, &first)
                .await
                .unwrap(),
            applied
        );

        let same = command(64, 2, "concurrent-same");
        let (first_same, second_same) = tokio::join!(
            compare_and_set_settings(&pool, &owner_a, &same),
            compare_and_set_settings(&pool, &owner_a, &same)
        );
        let same_result = SettingsStoreOutcome::Applied(BusinessSettings {
            excerpt_limit: 64,
            revision: 3,
        });
        assert_eq!(first_same.unwrap(), same_result);
        assert_eq!(second_same.unwrap(), same_result);
        assert_eq!(
            get_or_create_settings(&pool, &owner_a)
                .await
                .unwrap()
                .revision,
            3
        );

        let left = command(80, 3, "concurrent-left");
        let right = command(96, 3, "concurrent-right");
        let (left_result, right_result) = tokio::join!(
            compare_and_set_settings(&pool, &owner_a, &left),
            compare_and_set_settings(&pool, &owner_a, &right)
        );
        let results = [left_result.unwrap(), right_result.unwrap()];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, SettingsStoreOutcome::Applied(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == SettingsStoreOutcome::StaleRevision)
                .count(),
            1
        );
        let final_settings = get_or_create_settings(&pool, &owner_a).await.unwrap();
        assert_eq!(final_settings.revision, 4);
        assert!([80, 96].contains(&final_settings.excerpt_limit));

        let without_key = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 128,
                predecessor_revision: 4,
            },
            None,
        )
        .unwrap();
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &without_key)
                .await
                .unwrap(),
            SettingsStoreOutcome::Applied(BusinessSettings {
                excerpt_limit: 128,
                revision: 5,
            })
        );
        assert_eq!(
            compare_and_set_settings(&pool, &owner_a, &without_key)
                .await
                .unwrap(),
            SettingsStoreOutcome::StaleRevision
        );
        for predecessor_revision in [0, -1] {
            let old_predecessor = prepare_settings_command(
                UpdateBusinessSettings {
                    excerpt_limit: 160,
                    predecessor_revision,
                },
                None,
            )
            .unwrap();
            assert_eq!(
                compare_and_set_settings(&pool, &owner_a, &old_predecessor)
                    .await
                    .unwrap(),
                SettingsStoreOutcome::StaleRevision
            );
        }

        let overflow_owner = format!("settings-overflow-{}", Uuid::now_v7());
        sqlx::query(
            "INSERT INTO knowledge_reference.settings (owner_id, revision, excerpt_limit) \
             VALUES ($1, $2, 96)",
        )
        .bind(&overflow_owner)
        .bind(i64::MAX)
        .execute(&pool)
        .await
        .unwrap();
        let overflowing = command(48, i64::MAX, "rolled-back-key");
        assert!(
            compare_and_set_settings(&pool, &overflow_owner, &overflowing)
                .await
                .is_err()
        );
        let reservations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_reference.settings_idempotency \
             WHERE owner_id = $1 AND idempotency_key = 'rolled-back-key'",
        )
        .bind(&overflow_owner)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reservations, 0);
        pool.close().await;
    }
}
