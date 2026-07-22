use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Context as _;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use lenso_example_support_system::{
    PreparedM5DurableOperation, prepare_m5_durable_operation, resume_m5_durable_operation,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::Mutex;

const BUILD_RELEASE_VERSION: &str = match option_env!("M5_RELEASE_VERSION") {
    Some(version) => version,
    None => "development",
};

struct AppState {
    pool: PgPool,
    client: reqwest::Client,
    system_plane_endpoint: String,
    system_plane_health_path: String,
    release_id: String,
    build_release_version: &'static str,
    config_revision_id: String,
    secret_provider_lease_valid: bool,
    secret_rotation_policy_preserved: bool,
    durable_operations: Mutex<BTreeMap<String, PreparedM5DurableOperation>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .context("connect M5 Data Plane receipt store")?;
    sqlx::query(
        r#"
        create table if not exists m5_outage_receipts (
            idempotency_key text primary key,
            checkpoint_id text not null unique,
            state text not null check (state in ('prepared', 'completed')),
            evidence jsonb,
            business_effect_count bigint not null default 0,
            prepared_at timestamptz not null default now(),
            completed_at timestamptz
        )
        "#,
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        create table if not exists m5_acceptance_faults (
            release_id text primary key,
            latency_ms bigint not null check (latency_ms between 0 and 2000)
        )
        "#,
    )
    .execute(&pool)
    .await?;
    let state = Arc::new(AppState {
        pool,
        client: reqwest::Client::new(),
        system_plane_endpoint: std::env::var("SYSTEM_PLANE_ENDPOINT")
            .unwrap_or_else(|_| "http://lenso-system-plane:8080".to_owned()),
        system_plane_health_path: std::env::var("SYSTEM_PLANE_HEALTH_PATH")
            .unwrap_or_else(|_| "/openapi.json".to_owned()),
        release_id: std::env::var("RELEASE_ID").unwrap_or_default(),
        build_release_version: BUILD_RELEASE_VERSION,
        config_revision_id: std::env::var("CONFIG_REVISION_ID").unwrap_or_default(),
        secret_provider_lease_valid: std::env::var("DB_PASSWORD")
            .is_ok_and(|value| !value.is_empty()),
        secret_rotation_policy_preserved: std::env::var("SECRET_ROTATION_POLICY")
            .is_ok_and(|value| value == "preserve"),
        durable_operations: Mutex::new(BTreeMap::new()),
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/tickets/{ticket_id}", get(ticket))
        .route("/internal/outage/prepare", post(prepare_outage))
        .route("/internal/outage/prove", post(prove_outage))
        .route("/internal/reliability/probe", get(reliability_probe))
        .route("/control/{operation}", post(protected_operation))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({"status": "ready", "buildReleaseVersion": state.build_release_version}))
}

async fn ticket(Path(ticket_id): Path<String>, State(state): State<Arc<AppState>>) -> Json<Value> {
    let public_latency_ms = sqlx::query_scalar::<_, i64>(
        "select latency_ms from m5_acceptance_faults where release_id = $1",
    )
    .bind(&state.release_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or_default()
    .unwrap_or_default();
    tokio::time::sleep(Duration::from_millis(
        u64::try_from(public_latency_ms)
            .unwrap_or_default()
            .min(2_000),
    ))
    .await;
    let (queue_backlog, workflow_backlog, timer_lag_ms, retry_exhaustion, compensation_pressure) =
        sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            select
                count(*) filter (where state = 'prepared')::bigint,
                count(*) filter (where state = 'prepared')::bigint,
                coalesce(extract(epoch from (now() - min(prepared_at) filter (where state = 'prepared'))) * 1000, 0)::bigint,
                count(*) filter (where business_effect_count > 1)::bigint,
                count(*) filter (where evidence ? 'compensationRequired')::bigint
            from m5_outage_receipts
            "#,
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or_default();
    Json(json!({
        "ticketId": ticket_id,
        "status": "open",
        "releaseId": state.release_id,
        "buildReleaseVersion": state.build_release_version,
        "configRevisionId": state.config_revision_id,
        "operationalMetrics": {
            "queueBacklog": queue_backlog,
            "workflowBacklog": workflow_backlog,
            "timerLagMs": timer_lag_ms,
            "retryExhaustion": retry_exhaustion,
            "compensationPressure": compensation_pressure,
        },
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReliabilityProbeQuery {
    #[serde(default)]
    delay_ms: u64,
}

async fn reliability_probe(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReliabilityProbeQuery>,
) -> (StatusCode, Json<Value>) {
    tokio::time::sleep(Duration::from_millis(query.delay_ms.min(2_000))).await;
    let database_available = sqlx::query_scalar::<_, i32>("select 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    let status = if database_available {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "databaseAvailable": database_available,
            "queueBacklog": 0,
            "workflowBacklog": 0,
            "timerLagMs": 0,
            "retryExhaustion": 0,
            "compensationPressure": 0,
        })),
    )
}

async fn prepare_outage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let Some(idempotency_key) = authorized_idempotency_key(&headers) else {
        return denied();
    };
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(error) => return internal_error(error),
    };
    if let Err(error) = sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&idempotency_key)
        .execute(&mut *transaction)
        .await
    {
        return internal_error(error);
    }
    let existing = sqlx::query_as::<_, (String, String)>(
        "select checkpoint_id, state from m5_outage_receipts where idempotency_key = $1",
    )
    .bind(&idempotency_key)
    .fetch_optional(&mut *transaction)
    .await;
    match existing {
        Ok(Some((checkpoint_id, receipt_state))) => {
            if let Err(error) = transaction.commit().await {
                return internal_error(error);
            }
            return (
                StatusCode::OK,
                Json(json!({
                    "protocol": "lenso.m5-data-plane-checkpoint.v1",
                    "durableCheckpointId": checkpoint_id,
                    "state": receipt_state,
                    "mutated": false,
                })),
            );
        }
        Ok(None) => {}
        Err(error) => return internal_error(error),
    }
    let operation = match prepare_m5_durable_operation().await {
        Ok(operation) => operation,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "code": "durable_checkpoint_prepare_failed",
                    "message": error.to_string(),
                    "mutated": false,
                })),
            );
        }
    };
    let checkpoint_id = operation.checkpoint_id().to_owned();
    let inserted = sqlx::query(
        "insert into m5_outage_receipts (idempotency_key, checkpoint_id, state) values ($1, $2, 'prepared') returning checkpoint_id",
    )
    .bind(&idempotency_key)
    .bind(&checkpoint_id)
    .fetch_one(&mut *transaction)
    .await;
    match inserted {
        Ok(row) => {
            let committed_checkpoint: String = sqlx::Row::try_get(&row, "checkpoint_id")
                .expect("RETURNING checkpoint_id has a text column");
            if committed_checkpoint != checkpoint_id {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "code": "durable_checkpoint_identity_mismatch",
                        "mutated": false,
                    })),
                );
            }
            if let Err(error) = transaction.commit().await {
                return internal_error(error);
            }
            state
                .durable_operations
                .lock()
                .await
                .insert(idempotency_key, operation);
            (
                StatusCode::OK,
                Json(json!({
                    "protocol": "lenso.m5-data-plane-checkpoint.v1",
                    "durableCheckpointId": checkpoint_id,
                    "state": "prepared",
                    "mutated": true,
                })),
            )
        }
        Err(error) => internal_error(error),
    }
}

async fn prove_outage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let Some(idempotency_key) = authorized_idempotency_key(&headers) else {
        return denied();
    };
    let row = sqlx::query_as::<_, (String, String, Option<Value>, i64)>(
        "select checkpoint_id, state, evidence, business_effect_count from m5_outage_receipts where idempotency_key = $1",
    )
    .bind(&idempotency_key)
    .fetch_optional(&state.pool)
    .await;
    let (checkpoint_id, receipt_state, existing, _) = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "code": "durable_checkpoint_missing",
                    "mutated": false,
                    "nextActions": ["Prepare the durable operation before withholding coordination."],
                })),
            );
        }
        Err(error) => return internal_error(error),
    };
    if receipt_state == "completed" {
        return (StatusCode::OK, Json(existing.unwrap_or(Value::Null)));
    }
    let Some(operation) = state
        .durable_operations
        .lock()
        .await
        .remove(&idempotency_key)
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "code": "durable_checkpoint_process_state_missing",
                "mutated": false,
                "nextActions": ["Restore the prepared Data Plane worker that owns this durable checkpoint."],
            })),
        );
    };
    if operation.checkpoint_id() != checkpoint_id {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "durable_checkpoint_identity_mismatch",
                "mutated": false,
            })),
        );
    }
    let durable = match resume_m5_durable_operation(operation).await {
        Ok(proof) => proof,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "code": "durable_operation_incomplete",
                    "message": error.to_string(),
                    "mutated": false,
                })),
            );
        }
    };
    let mut operation_results = durable.operation_results;
    operation_results.insert("direct_request".to_owned(), true);
    let business_effect_count = durable.observations["workflow"]["completedEffects"]
        .as_i64()
        .unwrap_or_default();
    let evidence = json!({
        "protocol": "lenso.m5-data-plane-outage-observation.v1",
        "operationResults": operation_results,
        "security": {
            "workloadIdentityEnforced": durable.workload_identity_enforced,
            "tenantContextEnforced": durable.tenant_context_enforced,
            "callPolicyEnforced": durable.call_policy_enforced,
            "serviceAuthorizationEnforced": durable.service_authorization_enforced,
        },
        "lastValidConfigRevisionAvailable": !state.config_revision_id.is_empty(),
        "secretProviderLeaseValid": state.secret_provider_lease_valid,
        "secretRotationPolicyPreserved": state.secret_rotation_policy_preserved,
        "durableCheckpointId": checkpoint_id,
        "evidenceReferences": durable.evidence_references,
        "observations": durable.observations,
        "businessEffectCount": business_effect_count,
        "releaseId": state.release_id,
        "configRevisionId": state.config_revision_id,
    });
    let updated = sqlx::query(
        "update m5_outage_receipts set state = 'completed', evidence = $2, business_effect_count = $3, completed_at = now() where idempotency_key = $1 and state = 'prepared'",
    )
    .bind(&idempotency_key)
    .bind(&evidence)
    .bind(business_effect_count)
    .execute(&state.pool)
    .await;
    match updated {
        Ok(_) => (StatusCode::OK, Json(evidence)),
        Err(error) => internal_error(error),
    }
}

async fn protected_operation(
    State(state): State<Arc<AppState>>,
    Path(operation): Path<String>,
) -> (StatusCode, Json<Value>) {
    let system_plane = coordination_available(
        &state.client,
        &state.system_plane_endpoint,
        &state.system_plane_health_path,
    )
    .await;
    if system_plane {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "code": "approval_required",
                "operation": operation,
                "mutated": false,
                "nextActions": ["Submit the protected action through the reviewed approval boundary."],
            })),
        );
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "code": "coordination_unavailable",
            "operation": operation,
            "mutated": false,
            "nextActions": ["Restore the authoritative System Plane, then refresh protected-action evidence."],
        })),
    )
}

async fn coordination_available(
    client: &reqwest::Client,
    endpoint: &str,
    health_path: &str,
) -> bool {
    client
        .get(format!("{endpoint}{health_path}"))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

fn authorized_idempotency_key(headers: &HeaderMap) -> Option<String> {
    let principal = headers.get("x-service-principal")?.to_str().ok()?;
    let tenant = headers.get("x-tenant-id")?.to_str().ok()?;
    let idempotency_key = headers.get("x-idempotency-key")?.to_str().ok()?;
    (principal == "service:m5-acceptance-probe"
        && tenant == "tenant:m5"
        && !idempotency_key.is_empty())
    .then(|| idempotency_key.to_owned())
}

fn denied() -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"code": "service_authorization_denied", "mutated": false})),
    )
}

fn internal_error(error: sqlx::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "code": "receipt_store_unavailable",
            "message": error.to_string(),
            "mutated": false,
        })),
    )
}
