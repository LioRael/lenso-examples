use super::*;
use std::io::Write;

#[tokio::test]
#[ignore = "requires an explicitly supplied disposable PostgreSQL URL"]
async fn real_postgres_cas_receipts_and_recreation() {
    let uri =
        std::env::var("LENSO_OPS_TEST_DATABASE_URL").expect("supply a disposable test database");
    let admin = PgPoolOptions::new().connect(&uri).await.unwrap();
    let namespace = format!("ops_test_{}", std::process::id());
    let second = format!("{namespace}_second");
    let missing = format!("{namespace}_missing");
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(uri.as_bytes()).unwrap();
    let binding = |schema: &str| PgBinding {
        connection_uri_file: file.path().to_owned(),
        schema: schema.into(),
    };
    assert!(matches!(
        PgStateHandle::open(binding("invalid;schema")),
        Err(Failure::InvalidBinding)
    ));
    let unprepared = PgStateHandle::open(binding(&missing)).unwrap();
    assert_eq!(unprepared.readiness().await, Err(Failure::SetupRequired));
    unprepared.close().await;

    for (schema, label, value) in [
        (&namespace, "primary-state", 11),
        (&second, "secondary-state", 29),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let mut connection = admin.acquire().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../schema.sql"))
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query("INSERT INTO state VALUES (true, $1, $2, 0)")
            .bind(label)
            .bind(value)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    let primary = PgStateHandle::open(binding(&namespace)).unwrap();
    let secondary = PgStateHandle::open(binding(&second)).unwrap();
    primary.readiness().await.unwrap();
    secondary.readiness().await.unwrap();
    let committed = primary.update(47, 0, "write-1").await.unwrap();
    assert_eq!(
        primary.update(47, 0, "write-1").await,
        Ok(committed.clone())
    );
    assert_eq!(
        primary.update(48, 0, "write-1").await,
        Err(Failure::IdempotencyConflict)
    );
    assert_eq!(
        primary.update(48, 0, "new-stale").await,
        Err(Failure::StaleRevision)
    );
    assert_eq!(
        primary.update(48, 1, "bad key").await,
        Err(Failure::InvalidInput)
    );
    assert_eq!(secondary.read().await.unwrap().value, 29);
    let (first, second_result) = tokio::join!(
        primary.update(71, 1, "race-a"),
        primary.update(83, 1, "race-b")
    );
    assert_eq!(
        usize::from(first.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    assert!(first == Err(Failure::StaleRevision) || second_result == Err(Failure::StaleRevision));
    assert_eq!(primary.read().await.unwrap().revision, 2);
    primary.close().await;
    let recreated = PgStateHandle::open(binding(&namespace)).unwrap();
    recreated.readiness().await.unwrap();
    assert_eq!(recreated.read().await.unwrap().revision, 2);
    assert_eq!(recreated.receipt("write-1").await.unwrap(), Some(committed));
    recreated.close().await;
    secondary.close().await;
    for schema in [&namespace, &second] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
    }
    admin.close().await;
}
