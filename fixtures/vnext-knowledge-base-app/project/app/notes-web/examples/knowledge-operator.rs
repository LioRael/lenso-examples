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
            "CREATE TABLE IF NOT EXISTS knowledge_reference.notes (\
             owner_id TEXT NOT NULL, \
             note_id TEXT NOT NULL, \
             title TEXT NOT NULL, \
             body TEXT NOT NULL, \
             excerpt TEXT NOT NULL, \
             job_id TEXT NOT NULL UNIQUE, \
             processing_status TEXT NOT NULL CHECK (processing_status IN ('queued', 'succeeded')), \
             config_revision BIGINT NOT NULL, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT transaction_timestamp(), \
             PRIMARY KEY (owner_id, note_id))",
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
    for table in ["settings", "notes", "attachments"] {
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
    Ok(())
}
