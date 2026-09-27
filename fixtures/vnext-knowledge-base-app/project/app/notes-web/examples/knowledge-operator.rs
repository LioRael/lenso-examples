use std::{env, error::Error};

use sqlx::{Executor as _, postgres::PgPoolOptions};

const DATABASE_URL_ENV: &str = "LENSO_KNOWLEDGE_DATABASE_URL";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let command = env::args().nth(1).ok_or("missing setup or check command")?;
    if env::args().nth(2).is_some() {
        return Err("unexpected extra argument".into());
    }
    let database_url = env::var(DATABASE_URL_ENV).map_err(|_| {
        format!("{DATABASE_URL_ENV} must be set; URLs are never accepted as arguments")
    })?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    match command.as_str() {
        "setup" => setup(&pool).await?,
        "check" => check(&pool).await?,
        _ => return Err(format!("unknown command `{command}`").into()),
    }
    pool.close().await;
    Ok(())
}

async fn setup(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    transaction
        .execute("CREATE SCHEMA IF NOT EXISTS knowledge_reference")
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS knowledge_reference.settings (\
             owner_id TEXT PRIMARY KEY, \
             revision BIGINT NOT NULL CHECK (revision > 0), \
             excerpt_limit BIGINT NOT NULL CHECK (excerpt_limit BETWEEN 16 AND 512))",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS knowledge_reference.settings_idempotency (\
             owner_id TEXT NOT NULL REFERENCES knowledge_reference.settings(owner_id) ON DELETE CASCADE, \
             idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128), \
             payload_sha256 TEXT NOT NULL, \
             outcome_kind TEXT NOT NULL CHECK (outcome_kind IN ('pending', 'applied', 'stale')), \
             result_revision BIGINT, \
             result_excerpt_limit BIGINT, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(), \
             PRIMARY KEY (owner_id, idempotency_key))",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS knowledge_reference.notes (\
             owner_id TEXT NOT NULL, \
             note_id TEXT NOT NULL, \
             title TEXT NOT NULL, \
             body TEXT NOT NULL, \
             excerpt TEXT NOT NULL, \
             job_id TEXT UNIQUE, \
             processing_status TEXT NOT NULL CONSTRAINT notes_processing_status_check \
               CHECK (processing_status IN ('dispatch_pending', 'queued', 'succeeded', 'failed')), \
             config_revision BIGINT NOT NULL, \
             excerpt_limit BIGINT NOT NULL DEFAULT 96 CHECK (excerpt_limit BETWEEN 16 AND 512), \
             available_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z', \
             dispatch_attempts BIGINT NOT NULL DEFAULT 0 CHECK (dispatch_attempts >= 0), \
             next_dispatch_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(), \
             created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(), \
             PRIMARY KEY (owner_id, note_id))",
        )
        .await?;
    transaction
        .execute("ALTER TABLE knowledge_reference.notes ALTER COLUMN job_id DROP NOT NULL")
        .await?;
    transaction
        .execute(
            "ALTER TABLE knowledge_reference.notes \
             ADD COLUMN IF NOT EXISTS excerpt_limit BIGINT NOT NULL DEFAULT 96",
        )
        .await?;
    transaction
        .execute(
            "ALTER TABLE knowledge_reference.notes \
             ADD COLUMN IF NOT EXISTS available_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'",
        )
        .await?;
    transaction
        .execute(
            "ALTER TABLE knowledge_reference.notes \
             ADD COLUMN IF NOT EXISTS dispatch_attempts BIGINT NOT NULL DEFAULT 0",
        )
        .await?;
    transaction
        .execute(
            "ALTER TABLE knowledge_reference.notes \
             ADD COLUMN IF NOT EXISTS next_dispatch_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp()",
        )
        .await?;
    transaction
        .execute("ALTER TABLE knowledge_reference.notes DROP CONSTRAINT IF EXISTS notes_processing_status_check")
        .await?;
    transaction
        .execute(
            "ALTER TABLE knowledge_reference.notes ADD CONSTRAINT notes_processing_status_check \
             CHECK (processing_status IN ('dispatch_pending', 'queued', 'succeeded', 'failed'))",
        )
        .await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS knowledge_reference.attachments (\
             owner_id TEXT NOT NULL, \
             attachment_id TEXT NOT NULL, \
             note_id TEXT NOT NULL, \
             filename TEXT NOT NULL, \
             media_type TEXT NOT NULL, \
             content BYTEA NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 1048576), \
             policy_revision TEXT, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(), \
             PRIMARY KEY (owner_id, attachment_id), \
             FOREIGN KEY (owner_id, note_id) REFERENCES knowledge_reference.notes(owner_id, note_id) ON DELETE CASCADE)",
        )
        .await?;
    transaction
        .execute("ALTER TABLE knowledge_reference.attachments ADD COLUMN IF NOT EXISTS policy_revision TEXT")
        .await?;
    transaction.commit().await
}

async fn check(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    for table in ["settings", "settings_idempotency", "notes", "attachments"] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'knowledge_reference' AND table_name = $1)",
        )
        .bind(table)
        .fetch_one(pool)
        .await?;
        if !exists {
            return Err(sqlx::Error::Protocol(format!(
                "knowledge_reference.{table} is missing"
            )));
        }
    }
    let policy_revision_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns \
         WHERE table_schema = 'knowledge_reference' \
         AND table_name = 'attachments' AND column_name = 'policy_revision')",
    )
    .fetch_one(pool)
    .await?;
    if !policy_revision_exists {
        return Err(sqlx::Error::Protocol(
            "knowledge_reference.attachments.policy_revision is missing".to_owned(),
        ));
    }
    let pending_columns_exist: bool = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) = 4 FROM information_schema.columns \
         WHERE table_schema = 'knowledge_reference' AND table_name = 'notes' \
           AND column_name IN ('excerpt_limit', 'available_at', \
                               'dispatch_attempts', 'next_dispatch_at'))",
    )
    .fetch_one(pool)
    .await?;
    if !pending_columns_exist {
        return Err(sqlx::Error::Protocol(
            "knowledge_reference.notes outbox columns are missing".to_owned(),
        ));
    }
    let job_id_nullable: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns \
         WHERE table_schema = 'knowledge_reference' AND table_name = 'notes' \
           AND column_name = 'job_id' AND is_nullable = 'YES')",
    )
    .fetch_one(pool)
    .await?;
    if !job_id_nullable {
        return Err(sqlx::Error::Protocol(
            "knowledge_reference.notes.job_id must be nullable before dispatch".to_owned(),
        ));
    }
    let failed_status_supported: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_constraint constraint_record \
         JOIN pg_class table_record ON table_record.oid = constraint_record.conrelid \
         JOIN pg_namespace schema_record ON schema_record.oid = table_record.relnamespace \
         WHERE schema_record.nspname = 'knowledge_reference' \
           AND table_record.relname = 'notes' \
           AND constraint_record.conname = 'notes_processing_status_check' \
           AND pg_get_constraintdef(constraint_record.oid) LIKE '%failed%' \
           AND pg_get_constraintdef(constraint_record.oid) LIKE '%dispatch_pending%')",
    )
    .fetch_one(pool)
    .await?;
    if !failed_status_supported {
        return Err(sqlx::Error::Protocol(
            "knowledge_reference.notes must accept failed processing status".to_owned(),
        ));
    }
    Ok(())
}
