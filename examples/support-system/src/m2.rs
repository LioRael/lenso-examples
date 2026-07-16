use crate::{SLA_SERVICE, SmokeEvidence, TICKET_SERVICE, now_ms, run_smoke};
use anyhow::{Context, ensure};
use async_trait::async_trait;
use lenso_autonomous_service::{
    LocalTransportAdapter, ServiceEventAdmission, ServiceEventHandler, ServiceEventHandlerError,
    ServiceEventPublisher, ServiceEventRetryPolicy, TransportAdapter, TransportDelivery,
    TransportDiagnostic, TransportError, TransportErrorCode, TransportHealth,
    TransportNegativeAcknowledgement, TransportPublication, TransportPublicationReceipt,
    consume_service_events_once_at, plan_dead_letter_replay, prepare_runtime,
    relay_service_events_once, replay_dead_letter,
};
use lenso_service::{
    CallPolicyEvent, CallPolicyFailure, CallPolicyRuntime, CallPolicyTerminalOutcome,
    DelegatedActorCredentialRequest, DelegatedContextProvider, DirectGrpcCallError,
    DirectGrpcClient, EventEnvelope, ManualCallPolicyClock, ServiceContextPolicy, ServiceReference,
    ServiceTenancyMode, SystemSandboxDelegatedContextProvider,
    SystemSandboxWorkloadIdentityProvider, TenantCredentialRequest, WorkloadCredentialRequest,
    WorkloadIdentityProvider,
};
use platform_testing::TestDatabase;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CONSUMER_ID: &str = "support-sla-service";
const TRANSPORT_BINDING: &str = "sandbox-event:local-transport";
const TICKET_WORKER_ENDPOINT: &str = "http://127.0.0.1:4213";
const SLA_WORKER_ENDPOINT: &str = "http://127.0.0.1:4214";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M2SmokeEvidence {
    pub artifact_version: &'static str,
    #[serde(flatten)]
    pub direct: SmokeEvidence,
    pub event_flow: EventFlowEvidence,
    pub call_policy: CallPolicyProof,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFlowEvidence {
    pub event_type: String,
    pub adapter: String,
    pub business_effects: i64,
    pub authenticated_service_principal: String,
    pub delegated_actor: String,
    pub tenant_id: String,
    pub local_evidence_records: i64,
    pub service_local_evidence_files: u32,
    pub system_plane_withheld: bool,
    pub runtime_console_withheld: bool,
    pub scenarios: Vec<ScenarioEvidence>,
    pub cleanup_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioEvidence {
    pub scenario_id: String,
    pub outcome: String,
}

impl ScenarioEvidence {
    fn new(scenario_id: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            scenario_id: scenario_id.into(),
            outcome: outcome.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallPolicyProof {
    pub scenarios: Vec<ScenarioEvidence>,
    pub circuit_recovered: bool,
    pub fallback_handler: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct M2ProducerRequest {
    pub producer_database_url: String,
    pub transport_database_url: String,
    pub proof_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct M2ConsumerRequest {
    pub consumer_database_url: String,
    pub transport_database_url: String,
    pub proof_time: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct M2ProducerEvidence {
    pub event_type: String,
    pub producer_restart_attempts: i32,
    pub transport_interruption_recovered: bool,
}

#[derive(Debug)]
struct SupportSlaHandler;

#[derive(Debug)]
struct SupportSlaWatermarkHandler {
    accepted_after: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
struct PoisonOnceSupportSlaHandler {
    should_fail: AtomicBool,
}

#[derive(Debug)]
struct PublishThenFailAdapter {
    adapter: LocalTransportAdapter,
    should_fail: AtomicBool,
}

struct IdentityMaterial {
    workload_provider: Arc<SystemSandboxWorkloadIdentityProvider>,
    context_provider: Arc<SystemSandboxDelegatedContextProvider>,
    credential: lenso_service::WorkloadCredential,
    actor: lenso_service::DelegatedActorContext,
    tenant: lenso_service::TenantContext,
}

#[async_trait]
impl ServiceEventHandler for SupportSlaHandler {
    async fn handle(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        envelope: &EventEnvelope,
    ) -> Result<(), ServiceEventHandlerError> {
        let ticket_id = envelope.content.data["ticketId"].as_str().ok_or_else(|| {
            ServiceEventHandlerError::poison(
                "support_ticket_id_missing",
                "Support event requires ticketId",
            )
        })?;
        let actor = envelope
            .context
            .delegated_actor
            .as_ref()
            .map(|value| value.subject.as_str())
            .unwrap_or_default();
        let tenant = envelope
            .context
            .tenant
            .as_ref()
            .map(|value| value.tenant_id.as_str())
            .unwrap_or_default();
        sqlx::query(
            r"
            insert into support_sla_escalations (
                source_event_id, ticket_id, actor_subject, tenant_id
            ) values ($1, $2, $3, $4)
            on conflict (source_event_id) do nothing
            ",
        )
        .bind(&envelope.event_id)
        .bind(ticket_id)
        .bind(actor)
        .bind(tenant)
        .execute(&mut **transaction)
        .await
        .map_err(|error| {
            ServiceEventHandlerError::retryable("support_sla_store_unavailable", error.to_string())
        })?;
        Ok(())
    }
}

#[async_trait]
impl ServiceEventHandler for SupportSlaWatermarkHandler {
    async fn handle(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        envelope: &EventEnvelope,
    ) -> Result<(), ServiceEventHandlerError> {
        let occurred_at = chrono::DateTime::parse_from_rfc3339(&envelope.occurred_at)
            .map_err(|_| {
                ServiceEventHandlerError::poison(
                    "support_occurred_at_invalid",
                    "Support event requires an RFC 3339 occurredAt",
                )
            })?
            .to_utc();
        if occurred_at < self.accepted_after {
            return Err(ServiceEventHandlerError::rejected_with_code(
                "support_event_out_of_order",
                "Support event occurred before the accepted SLA watermark",
            ));
        }
        SupportSlaHandler.handle(transaction, envelope).await
    }
}

#[async_trait]
impl ServiceEventHandler for PoisonOnceSupportSlaHandler {
    async fn handle(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        envelope: &EventEnvelope,
    ) -> Result<(), ServiceEventHandlerError> {
        if self.should_fail.swap(false, Ordering::SeqCst) {
            return Err(ServiceEventHandlerError::poison(
                "invalid_support_payload",
                "Support payload cannot be handled until corrected",
            ));
        }
        SupportSlaHandler.handle(transaction, envelope).await
    }
}

#[async_trait]
impl TransportAdapter for PublishThenFailAdapter {
    async fn publish(
        &self,
        publication: TransportPublication,
    ) -> Result<TransportPublicationReceipt, TransportError> {
        let receipt = self.adapter.publish(publication).await?;
        if self.should_fail.swap(false, Ordering::SeqCst) {
            return Err(TransportError::new(
                TransportErrorCode::DeliveryFailed,
                "producer stopped before recording the publication receipt",
            ));
        }
        Ok(receipt)
    }

    async fn receive(
        &self,
        consumer_id: &str,
        limit: i64,
    ) -> Result<Vec<TransportDelivery>, TransportError> {
        self.adapter.receive(consumer_id, limit).await
    }

    async fn acknowledge(&self, delivery: &TransportDelivery) -> Result<(), TransportError> {
        self.adapter.acknowledge(delivery).await
    }

    async fn negative_acknowledge(
        &self,
        delivery: &TransportDelivery,
        acknowledgement: TransportNegativeAcknowledgement,
    ) -> Result<(), TransportError> {
        self.adapter
            .negative_acknowledge(delivery, acknowledgement)
            .await
    }

    async fn health(&self) -> Result<TransportHealth, TransportError> {
        self.adapter.health().await
    }

    async fn diagnostics(&self) -> Result<Vec<TransportDiagnostic>, TransportError> {
        self.adapter.diagnostics().await
    }
}

pub async fn run_m2_smoke() -> anyhow::Result<M2SmokeEvidence> {
    let direct = run_smoke().await?;
    let state_path = std::path::PathBuf::from(crate::SANDBOX_STATE);
    let withheld_path = state_path.with_extension("m2-withheld");
    tokio::fs::rename(&state_path, &withheld_path)
        .await
        .context("withhold System Plane state during M2 event flow")?;
    let event_result = run_event_flow().await;
    let call_policy_result = prove_call_policy().await;
    tokio::fs::rename(&withheld_path, &state_path)
        .await
        .context("restore System Plane state after M2 event flow")?;
    let mut event_flow = event_result?;
    event_flow.system_plane_withheld = true;
    event_flow.runtime_console_withheld = direct.runtime_console_withheld;
    let call_policy = call_policy_result?;
    Ok(M2SmokeEvidence {
        artifact_version: "lenso.m2-support-system-smoke.v1",
        direct,
        event_flow,
        call_policy,
    })
}

async fn run_event_flow() -> anyhow::Result<EventFlowEvidence> {
    let producer = TestDatabase::create().await.context(
        "M2 acceptance requires a reachable DATABASE_URL for the producer Service Store",
    )?;
    let consumer = match TestDatabase::create().await {
        Some(database) => database,
        None => {
            producer.cleanup().await;
            anyhow::bail!("M2 acceptance could not create the consumer Service Store");
        }
    };
    let transport = match TestDatabase::create().await {
        Some(database) => database,
        None => {
            producer.cleanup().await;
            consumer.cleanup().await;
            anyhow::bail!("M2 acceptance could not create the local Transport Store");
        }
    };

    let result = run_event_flow_with(&producer, &consumer, &transport).await;
    producer.cleanup().await;
    consumer.cleanup().await;
    transport.cleanup().await;
    result.map(|mut evidence| {
        evidence.cleanup_completed = true;
        evidence
    })
}

async fn run_event_flow_with(
    producer: &TestDatabase,
    consumer: &TestDatabase,
    transport: &TestDatabase,
) -> anyhow::Result<EventFlowEvidence> {
    let proof_time = chrono::Utc::now().to_rfc3339();
    let client = reqwest::Client::new();
    let producer_response = client
        .post(format!("{TICKET_WORKER_ENDPOINT}/m2/events/produce"))
        .json(&M2ProducerRequest {
            producer_database_url: producer.url.clone(),
            transport_database_url: transport.url.clone(),
            proof_time: proof_time.clone(),
        })
        .send()
        .await
        .context("call the running support-ticket Worker")?;
    let producer_status = producer_response.status();
    let producer_body = producer_response.text().await?;
    ensure!(
        producer_status.is_success(),
        "support-ticket Worker rejected M2 production: {producer_body}"
    );
    let producer_evidence: M2ProducerEvidence = serde_json::from_str(&producer_body)?;
    ensure!(
        producer_evidence.producer_restart_attempts == 2
            && producer_evidence.transport_interruption_recovered,
        "running producer Worker did not recover its durable Outbox"
    );
    let consumer_response = client
        .post(format!("{SLA_WORKER_ENDPOINT}/m2/events/consume"))
        .json(&M2ConsumerRequest {
            consumer_database_url: consumer.url.clone(),
            transport_database_url: transport.url.clone(),
            proof_time,
        })
        .send()
        .await
        .context("call the running support-sla Worker")?;
    let consumer_status = consumer_response.status();
    let consumer_body = consumer_response.text().await?;
    ensure!(
        consumer_status.is_success(),
        "support-sla Worker rejected M2 consumption: {consumer_body}"
    );
    let mut evidence: EventFlowEvidence = serde_json::from_str(&consumer_body)?;
    ensure!(
        evidence.event_type == producer_evidence.event_type,
        "producer and consumer Workers proved different Event Contracts"
    );
    let service_root = std::path::Path::new(".lenso/system-sandbox/support-platform/services");
    let producer_file = service_root
        .join(TICKET_SERVICE)
        .join("store/m2-event-producer.json");
    let consumer_file = service_root
        .join(SLA_SERVICE)
        .join("store/m2-event-consumer.json");
    ensure!(
        tokio::fs::try_exists(&producer_file).await?
            && tokio::fs::try_exists(&consumer_file).await?,
        "running Service Workers did not persist Service-local M2 evidence"
    );
    evidence.service_local_evidence_files = 2;
    Ok(evidence)
}

pub async fn run_m2_producer(request: M2ProducerRequest) -> anyhow::Result<M2ProducerEvidence> {
    let now = chrono::DateTime::parse_from_rfc3339(&request.proof_time)?.to_utc();
    let producer_pool = connect_m2_pool(&request.producer_database_url).await?;
    let transport_pool = connect_m2_pool(&request.transport_database_url).await?;
    let producer_contract = serde_json::from_str(include_str!(
        "../services/support-ticket/lenso.service.json"
    ))?;
    let producer_state = prepare_runtime(
        &producer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            TICKET_SERVICE,
            "support-ticket-store",
            TICKET_SERVICE,
        ),
        producer_pool.clone(),
        &[platform_core::Migration {
            name: "support-ticket/0001_create_tickets",
            sql: "create table support_tickets (id text primary key, priority text not null);",
        }],
    )
    .await?;
    let adapter = LocalTransportAdapter::prepare(transport_pool).await?;
    let IdentityMaterial {
        workload_provider,
        context_provider: _,
        credential,
        actor,
        tenant,
    } = identity_material(now, TRANSPORT_BINDING)?;
    let envelope = support_event(
        "m2-support-event",
        "ticket_m2",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    let mut transaction = producer_pool.begin().await?;
    sqlx::query("insert into support_tickets (id, priority) values ($1, 'urgent')")
        .bind("ticket_m2")
        .execute(&mut *transaction)
        .await?;
    ServiceEventPublisher
        .publish_in_tx(&mut transaction, CONSUMER_ID, &envelope)
        .await?;
    transaction.commit().await?;
    ensure!(
        relay_service_events_once(&producer_state, &adapter, 1).await? == 1,
        "support event was not relayed"
    );
    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: envelope.clone(),
        })
        .await?;

    for event in [
        support_event(
            "m2-delayed-event",
            "ticket_delayed",
            now - chrono::Duration::hours(1),
            credential.service_principal_context(),
            actor.clone(),
            tenant.clone(),
        )?,
        support_event(
            "m2-reordered-newer-event",
            "ticket_reordered_newer",
            now + chrono::Duration::seconds(1),
            credential.service_principal_context(),
            actor.clone(),
            tenant.clone(),
        )?,
        support_event(
            "m2-reordered-older-event",
            "ticket_reordered_older",
            now - chrono::Duration::seconds(1),
            credential.service_principal_context(),
            actor.clone(),
            tenant.clone(),
        )?,
    ] {
        adapter
            .publish(TransportPublication {
                consumer_id: CONSUMER_ID.to_owned(),
                envelope: event,
            })
            .await?;
    }

    let producer_restart = support_event(
        "m2-producer-restart-event",
        "ticket_producer_restart",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    let mut transaction = producer_pool.begin().await?;
    sqlx::query("insert into support_tickets (id, priority) values ($1, 'urgent')")
        .bind("ticket_producer_restart")
        .execute(&mut *transaction)
        .await?;
    ServiceEventPublisher
        .publish_in_tx(&mut transaction, CONSUMER_ID, &producer_restart)
        .await?;
    transaction.commit().await?;
    let interrupted_adapter = PublishThenFailAdapter {
        adapter: adapter.clone(),
        should_fail: AtomicBool::new(true),
    };
    let Err(interruption) =
        relay_service_events_once(&producer_state, &interrupted_adapter, 1).await
    else {
        anyhow::bail!("transport interruption did not fail the first publication receipt");
    };
    ensure!(
        interruption.code == TransportErrorCode::DeliveryFailed,
        "transport interruption returned the wrong stable error code"
    );
    let restarted_producer_state = prepare_runtime(
        &producer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            TICKET_SERVICE,
            "support-ticket-store",
            TICKET_SERVICE,
        ),
        producer_pool.clone(),
        &[],
    )
    .await?;
    ensure!(
        relay_service_events_once(&restarted_producer_state, &adapter, 1).await? == 1,
        "producer restart did not relay its durable Outbox event"
    );

    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: support_event(
                "m2-consumer-restart-event",
                "ticket_consumer_restart",
                now,
                credential.service_principal_context(),
                actor.clone(),
                tenant.clone(),
            )?,
        })
        .await?;
    let issued_at = u64::try_from(now.timestamp_millis())?;
    let expiring = workload_provider.issue(WorkloadCredentialRequest::new(
        format!("service:{TICKET_SERVICE}"),
        SLA_SERVICE,
        TRANSPORT_BINDING,
        issued_at,
        1,
    ))?;
    let wrong_audience = workload_provider.issue(WorkloadCredentialRequest::new(
        format!("service:{TICKET_SERVICE}"),
        "wrong-service",
        TRANSPORT_BINDING,
        issued_at,
        30_000,
    ))?;
    let mut missing_tenant = support_event(
        "m2-missing-tenant-event",
        "ticket_missing_tenant",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    missing_tenant.context.tenant = None;
    for event in [
        support_event(
            "m2-identity-expiry-event",
            "ticket_identity_expiry",
            now,
            expiring.service_principal_context(),
            actor.clone(),
            tenant.clone(),
        )?,
        support_event(
            "m2-wrong-audience-event",
            "ticket_wrong_audience",
            now,
            wrong_audience.service_principal_context(),
            actor.clone(),
            tenant.clone(),
        )?,
        missing_tenant,
    ] {
        adapter
            .publish(TransportPublication {
                consumer_id: CONSUMER_ID.to_owned(),
                envelope: event,
            })
            .await?;
    }
    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: support_event(
                "m2-poison-event",
                "ticket_poison",
                now,
                credential.service_principal_context(),
                actor.clone(),
                tenant.clone(),
            )?,
        })
        .await?;
    let producer_restart_attempts: i32 = sqlx::query_scalar(
        "select attempts from platform.service_event_outbox where event_id = 'm2-producer-restart-event'",
    )
    .fetch_one(&producer_pool)
    .await?;
    ensure!(
        producer_restart_attempts == 2,
        "producer restart did not preserve two durable relay attempts"
    );
    Ok(M2ProducerEvidence {
        event_type: envelope.event_type,
        producer_restart_attempts,
        transport_interruption_recovered: true,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn run_m2_consumer(request: M2ConsumerRequest) -> anyhow::Result<EventFlowEvidence> {
    let now = chrono::DateTime::parse_from_rfc3339(&request.proof_time)?.to_utc();
    let consumer_pool = connect_m2_pool(&request.consumer_database_url).await?;
    let transport_pool = connect_m2_pool(&request.transport_database_url).await?;
    let consumer_contract =
        serde_json::from_str(include_str!("../services/support-sla/lenso.service.json"))?;
    let consumer_state = prepare_runtime(
        &consumer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            SLA_SERVICE,
            "support-sla-store",
            SLA_SERVICE,
        ),
        consumer_pool.clone(),
        &[platform_core::Migration {
            name: "support-sla/0001_create_escalations",
            sql: "create table support_sla_escalations (source_event_id text primary key, ticket_id text not null, actor_subject text not null, tenant_id text not null);",
        }],
    )
    .await?;
    let adapter = LocalTransportAdapter::prepare(transport_pool.clone()).await?;
    let IdentityMaterial {
        workload_provider,
        context_provider,
        credential: _,
        actor: _,
        tenant: _,
    } = identity_material(now, TRANSPORT_BINDING)?;
    let admission = event_admission(workload_provider, context_provider);
    let retry_policy = ServiceEventRetryPolicy::default();

    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now,
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "support event did not produce a business effect"
    );
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(1),
            &retry_policy,
            &admission,
        )
        .await?
            == 0,
        "duplicate event repeated its business effect"
    );
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(2),
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "delayed event did not produce its business effect"
    );

    let watermark_handler = SupportSlaWatermarkHandler {
        accepted_after: now,
    };
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &watermark_handler,
            1,
            now + chrono::Duration::milliseconds(3),
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "newer reordered event was not handled"
    );
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &watermark_handler,
            1,
            now + chrono::Duration::milliseconds(4),
            &retry_policy,
            &admission,
        )
        .await?
            == 0,
        "older reordered event produced a business effect"
    );
    ensure!(
        inbox_reason(&consumer_pool, "m2-reordered-older-event").await?
            == "support_event_out_of_order",
        "reordered event did not preserve the module-owned rejection reason"
    );

    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            2,
            now + chrono::Duration::milliseconds(8),
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "producer restart did not converge duplicate deliveries to one business effect"
    );
    let producer_restart_effects: (i64, i64) = sqlx::query_as(
        r"
        select
          (select count(*) from support_sla_escalations where source_event_id = 'm2-producer-restart-event'),
          (select count(*) from platform.service_event_delivery_evidence where event_id = 'm2-producer-restart-event' and stage = 'inbox' and outcome = 'duplicate')
        ",
    )
    .fetch_one(&consumer_pool)
    .await?;
    ensure!(
        producer_restart_effects == (1, 1),
        "producer restart evidence did not prove one effect and one duplicate"
    );

    let abandoned = adapter.receive(CONSUMER_ID, 1).await?;
    ensure!(
        abandoned.len() == 1,
        "consumer restart did not leave exactly one unacknowledged delivery"
    );
    drop(abandoned);
    let restarted_adapter = LocalTransportAdapter::prepare(transport_pool.clone()).await?;
    let restarted_consumer_state = prepare_runtime(
        &consumer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            SLA_SERVICE,
            "support-sla-store",
            SLA_SERVICE,
        ),
        consumer_pool.clone(),
        &[],
    )
    .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(9),
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "consumer restart did not recover the unacknowledged delivery"
    );
    let recovered: i64 = sqlx::query_scalar(
        "select count(*) from platform.local_transport_diagnostics where event_id = 'm2-consumer-restart-event' and outcome = 'recovered_unacknowledged'",
    )
    .fetch_one(&transport_pool)
    .await?;
    ensure!(
        recovered == 1,
        "consumer restart recovery evidence was not local"
    );

    for (event_id, expected_reason, message) in [
        (
            "m2-identity-expiry-event",
            "credential_expired",
            "expired Workload Identity produced a business effect",
        ),
        (
            "m2-wrong-audience-event",
            "audience_mismatch",
            "wrong-audience Workload Identity produced a business effect",
        ),
        (
            "m2-missing-tenant-event",
            "tenant_context_required",
            "missing required Tenant Context produced a business effect",
        ),
    ] {
        ensure!(
            consume_service_events_once_at(
                &restarted_consumer_state,
                &restarted_adapter,
                CONSUMER_ID,
                &SupportSlaHandler,
                1,
                now + chrono::Duration::milliseconds(10),
                &retry_policy,
                &admission,
            )
            .await?
                == 0,
            "{message}"
        );
        ensure!(
            inbox_reason(&consumer_pool, event_id).await? == expected_reason,
            "receiver-local identity reason changed for {event_id}"
        );
    }
    let denied_effects: i64 = sqlx::query_scalar(
        r"
        select count(*) from support_sla_escalations
        where source_event_id in (
          'm2-identity-expiry-event',
          'm2-wrong-audience-event',
          'm2-missing-tenant-event'
        )
        ",
    )
    .fetch_one(&consumer_pool)
    .await?;
    ensure!(
        denied_effects == 0,
        "rejected identities changed business state"
    );

    let poison_handler = PoisonOnceSupportSlaHandler {
        should_fail: AtomicBool::new(true),
    };
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(13),
            &retry_policy,
            &admission,
        )
        .await?
            == 0,
        "poison event unexpectedly produced a business effect"
    );
    let dead_letter_id: String = sqlx::query_scalar(
        "select dead_letter_id from platform.service_event_dead_letters where event_id = 'm2-poison-event'",
    )
    .fetch_one(&consumer_pool)
    .await?;
    let replay_plan = plan_dead_letter_replay(
        &restarted_consumer_state,
        &restarted_adapter,
        &dead_letter_id,
    )
    .await?;
    replay_dead_letter(
        &restarted_consumer_state,
        &restarted_adapter,
        &replay_plan,
        None,
    )
    .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(14),
            &retry_policy,
            &admission,
        )
        .await?
            == 1,
        "corrected dead-letter replay did not produce its business effect"
    );
    let duplicate_replay_plan = plan_dead_letter_replay(
        &restarted_consumer_state,
        &restarted_adapter,
        &dead_letter_id,
    )
    .await?;
    replay_dead_letter(
        &restarted_consumer_state,
        &restarted_adapter,
        &duplicate_replay_plan,
        None,
    )
    .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(15),
            &retry_policy,
            &admission,
        )
        .await?
            == 0,
        "duplicate dead-letter replay repeated its business effect"
    );
    let replay_effects: i64 = sqlx::query_scalar(
        "select count(*) from support_sla_escalations where source_event_id = 'm2-poison-event'",
    )
    .fetch_one(&consumer_pool)
    .await?;
    ensure!(replay_effects == 1, "dead-letter replay was not idempotent");

    let scenarios = vec![
        ScenarioEvidence::new("duplicate", "business_effect_deduplicated"),
        ScenarioEvidence::new("delayed", "delayed_event_handled"),
        ScenarioEvidence::new("reordered", "older_event_rejected_by_watermark"),
        ScenarioEvidence::new("poison", "poison_event_dead_lettered"),
        ScenarioEvidence::new(
            "producer_restart",
            "outbox_recovered_without_repeating_effect",
        ),
        ScenarioEvidence::new("consumer_restart", "unacknowledged_delivery_recovered"),
        ScenarioEvidence::new(
            "transport_interruption",
            "publication_receipt_failure_retried",
        ),
        ScenarioEvidence::new("dead_letter_replay", "replay_idempotent"),
        ScenarioEvidence::new("identity_expiry", "credential_expired_locally"),
        ScenarioEvidence::new("wrong_audience", "audience_rejected_locally"),
        ScenarioEvidence::new("missing_tenant", "tenant_context_required_locally"),
    ];

    let (business_effects, delegated_actor, tenant_id): (i64, String, String) = sqlx::query_as(
        "select count(*) over (), actor_subject, tenant_id from support_sla_escalations where source_event_id = 'm2-support-event'",
    )
    .fetch_one(&consumer_pool)
    .await?;
    ensure!(
        business_effects == 1,
        "support event business effect was not exactly once"
    );
    let identity_outcome: String = sqlx::query_scalar(
        "select outcome from platform.service_event_delivery_evidence where event_id = 'm2-support-event' and stage = 'identity'",
    )
    .fetch_one(&consumer_pool)
    .await?;
    ensure!(
        identity_outcome == "identity_context_accepted",
        "receiver did not persist accepted identity evidence"
    );
    let local_evidence_records: i64 = sqlx::query_scalar(
        "select count(*) from platform.service_event_delivery_evidence where event_id = 'm2-support-event'",
    )
    .fetch_one(&consumer_pool)
    .await?;
    Ok(EventFlowEvidence {
        event_type: "support.ticket-opened.v1".to_owned(),
        adapter: "local".to_owned(),
        business_effects,
        authenticated_service_principal: format!("service:{TICKET_SERVICE}"),
        delegated_actor,
        tenant_id,
        local_evidence_records,
        service_local_evidence_files: 0,
        system_plane_withheld: false,
        runtime_console_withheld: false,
        scenarios,
        cleanup_completed: false,
    })
}

async fn connect_m2_pool(url: &str) -> anyhow::Result<PgPool> {
    platform_core::connect_pool(&platform_core::DatabaseConfig {
        url: url.to_owned(),
        max_connections: 5,
    })
    .await
    .map_err(Into::into)
}

fn identity_material(
    now: chrono::DateTime<chrono::Utc>,
    transport_binding: &str,
) -> anyhow::Result<IdentityMaterial> {
    let issued_at = u64::try_from(now.timestamp_millis())?;
    let workload_provider = Arc::new(SystemSandboxWorkloadIdentityProvider::new(
        "local",
        "m2-support-workload-secret",
    )?);
    let context_provider = Arc::new(SystemSandboxDelegatedContextProvider::new(
        "local",
        "m2-support-context-secret",
    )?);
    let credential = workload_provider.issue(WorkloadCredentialRequest::new(
        format!("service:{TICKET_SERVICE}"),
        SLA_SERVICE,
        transport_binding,
        issued_at,
        30_000,
    ))?;
    let actor = context_provider.issue_actor(DelegatedActorCredentialRequest::new(
        "user_01",
        SLA_SERVICE,
        "support.ticket.opened",
        ["support.tickets.read"],
        issued_at,
        30_000,
    ))?;
    let tenant = context_provider.issue_tenant(TenantCredentialRequest::new(
        "tenant_01",
        "user_01",
        "delegation_1",
        SLA_SERVICE,
        issued_at,
        30_000,
    ))?;
    Ok(IdentityMaterial {
        workload_provider,
        context_provider,
        credential,
        actor,
        tenant,
    })
}

fn event_admission(
    workload_provider: Arc<SystemSandboxWorkloadIdentityProvider>,
    context_provider: Arc<SystemSandboxDelegatedContextProvider>,
) -> ServiceEventAdmission {
    ServiceEventAdmission::new(
        workload_provider,
        SLA_SERVICE,
        context_provider,
        [(
            "support.ticket-opened.v1",
            ServiceContextPolicy::new(
                SLA_SERVICE,
                "support.ticket.opened",
                ["support.tickets.read"],
                ["support.tickets.read"],
                ServiceTenancyMode::Required,
            ),
        )],
    )
}

async fn inbox_reason(pool: &PgPool, event_id: &str) -> anyhow::Result<String> {
    sqlx::query_scalar("select reason_code from platform.service_event_inbox where event_id = $1")
        .bind(event_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

fn support_event(
    event_id: &str,
    ticket_id: &str,
    occurred_at: chrono::DateTime<chrono::Utc>,
    service_principal: lenso_service::ServicePrincipal,
    delegated_actor: lenso_service::DelegatedActorContext,
    tenant: lenso_service::TenantContext,
) -> anyhow::Result<EventEnvelope> {
    let mut envelope: EventEnvelope = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/events/support/support.ticket-opened.v1.envelope.json"
    ))?;
    envelope.event_id = event_id.to_owned();
    envelope.producer_service_id = TICKET_SERVICE.to_owned();
    envelope.occurred_at = occurred_at.to_rfc3339();
    envelope.content.schema = "contracts/support.ticket-opened.v1.schema.json".to_owned();
    envelope.content.data = json!({
        "ticketId": ticket_id,
        "openedAt": occurred_at.to_rfc3339(),
    });
    envelope.context.service_principal = Some(service_principal);
    envelope.context.delegated_actor = Some(delegated_actor);
    envelope.context.tenant = Some(tenant);
    envelope.context.deadline = Some(lenso_service::DeadlineContext {
        expires_at_unix_ms: now_ms() + 30_000,
    });
    Ok(envelope)
}

async fn prove_call_policy() -> anyhow::Result<CallPolicyProof> {
    let clock = Arc::new(ManualCallPolicyClock::new(1_000));
    let service = ServiceReference::new(SLA_SERVICE);
    let circuit_client = DirectGrpcClient::new(
        crate::resolver(SLA_SERVICE, crate::SLA_ENDPOINT)?,
        crate::grpc_bindings()?,
    )
    .with_policy_runtime(CallPolicyRuntime::new(clock.clone()))
    .with_fallback("support.cached_sla", |_| b"cached-sla".to_vec());
    for _ in 0..2 {
        let failure = circuit_client
            .get_sla(
                &service,
                b"m2-call-policy-failure".to_vec(),
                now_ms() + 5_000,
            )
            .await
            .expect_err("controlled real Service call must fail before the circuit opens");
        ensure!(
            matches!(failure, DirectGrpcCallError::Status { .. }),
            "controlled circuit failure did not reach the running SLA Service"
        );
    }
    let fallback = circuit_client
        .get_sla(&service, b"fallback".to_vec(), now_ms() + 5_000)
        .await?;
    ensure!(
        fallback.payload == b"cached-sla"
            && fallback.evidence.call_policy.terminal_outcome
                == CallPolicyTerminalOutcome::Fallback
            && fallback
                .evidence
                .call_policy
                .events
                .contains(&CallPolicyEvent::FallbackApplied),
        "open circuit did not execute the declared business fallback"
    );
    let fallback_handler = fallback
        .evidence
        .call_policy
        .fallback_handler
        .clone()
        .context("executed fallback lost its handler identity")?;
    clock.advance_ms(1_000);
    let recovered = circuit_client
        .get_sla(&service, b"live".to_vec(), now_ms() + 5_000)
        .await?;
    ensure!(
        recovered
            .evidence
            .call_policy
            .events
            .contains(&CallPolicyEvent::CircuitRecovered),
        "half-open real Service call did not recover the circuit"
    );

    let bulkhead_client = Arc::new(DirectGrpcClient::new(
        crate::resolver(SLA_SERVICE, crate::SLA_ENDPOINT)?,
        crate::grpc_bindings()?,
    ));
    let mut bulkhead_calls = Vec::new();
    for _ in 0..2 {
        let client = Arc::clone(&bulkhead_client);
        let service = service.clone();
        bulkhead_calls.push(tokio::spawn(async move {
            client
                .get_sla(&service, b"m2-call-policy-slow".to_vec(), now_ms() + 5_000)
                .await
        }));
    }
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let bulkhead = bulkhead_client
        .get_sla(&service, b"m2-call-policy-slow".to_vec(), now_ms() + 5_000)
        .await
        .expect_err("third concurrent real call must hit the caller bulkhead");
    ensure!(
        matches!(
            bulkhead,
            DirectGrpcCallError::Policy {
                failure: CallPolicyFailure::BulkheadSaturated,
                ..
            }
        ),
        "caller bulkhead returned the wrong stable failure"
    );
    for call in bulkhead_calls {
        call.await??;
    }

    let mut overload_calls = Vec::new();
    for _ in 0..2 {
        let client = DirectGrpcClient::new(
            crate::resolver(SLA_SERVICE, crate::SLA_ENDPOINT)?,
            crate::grpc_bindings()?,
        );
        let service = service.clone();
        overload_calls.push(tokio::spawn(async move {
            client
                .get_sla(&service, b"m2-call-policy-slow".to_vec(), now_ms() + 5_000)
                .await
        }));
    }
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let overload = DirectGrpcClient::new(
        crate::resolver(SLA_SERVICE, crate::SLA_ENDPOINT)?,
        crate::grpc_bindings()?,
    )
    .get_sla(&service, b"m2-call-policy-slow".to_vec(), now_ms() + 5_000)
    .await
    .expect_err("third independently admitted real call must hit receiver overload");
    let DirectGrpcCallError::Status {
        evidence: overload_evidence,
        ..
    } = overload
    else {
        anyhow::bail!("receiver overload did not preserve the native gRPC status");
    };
    ensure!(
        overload_evidence
            .call_policy
            .events
            .contains(&CallPolicyEvent::OverloadRejected),
        "receiver overload returned the wrong stable Call Policy event"
    );
    for call in overload_calls {
        call.await??;
    }

    let deadline = DirectGrpcClient::new(
        crate::resolver(SLA_SERVICE, crate::SLA_ENDPOINT)?,
        crate::grpc_bindings()?,
    )
    .with_fallback("support.cached_sla", |_| b"deadline-fallback".to_vec())
    .get_sla(
        &service,
        b"m2-call-policy-deadline".to_vec(),
        now_ms() + 5_000,
    )
    .await?;
    ensure!(
        deadline.payload == b"deadline-fallback"
            && deadline
                .evidence
                .call_policy
                .events
                .contains(&CallPolicyEvent::DeadlineExpired)
            && deadline.evidence.call_policy.terminal_outcome
                == CallPolicyTerminalOutcome::Fallback,
        "real in-flight Deadline did not execute its fallback"
    );

    Ok(CallPolicyProof {
        scenarios: vec![
            ScenarioEvidence::new("circuit_open", "actual_call_circuit_opened"),
            ScenarioEvidence::new("bulkhead", "actual_call_bulkhead_saturated"),
            ScenarioEvidence::new("overload", "actual_receiver_overload_rejected"),
            ScenarioEvidence::new("deadline", "actual_call_deadline_expired"),
            ScenarioEvidence::new("fallback", "business_fallback_executed"),
        ],
        circuit_recovered: true,
        fallback_handler,
    })
}

#[cfg(test)]
mod production_tests {
    use super::*;
    use lenso_autonomous_service::{
        NatsJetStreamConsumerBinding, NatsJetStreamTransportAdapter, NatsJetStreamTransportConfig,
    };
    use lenso_service::AuthenticatedTransportBinding;
    use std::time::Duration;

    #[tokio::test]
    async fn nats_jetstream_runs_same_support_module_behavior() {
        if std::env::var("LENSO_NATS_TEST_INFRASTRUCTURE_APPROVED").as_deref() != Ok("true") {
            return;
        }
        let diagnostic_store = TestDatabase::create()
            .await
            .expect("approved NATS proof requires DATABASE_URL");
        let consumer_store = TestDatabase::create()
            .await
            .expect("approved NATS proof requires a consumer Service Store");
        let nats_url =
            std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_owned());
        let context = async_nats::jetstream::new(
            async_nats::connect(&nats_url)
                .await
                .expect("approved NATS JetStream infrastructure must be reachable"),
        );
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let stream_name = format!("LENSO_M2_SUPPORT_{suffix}").to_uppercase();
        let subject = format!("lenso.m2.{suffix}.support_sla");
        let durable_consumer_name = format!("support_sla_{suffix}");
        let stream = context
            .create_stream(async_nats::jetstream::stream::Config {
                name: stream_name.clone(),
                subjects: vec![subject.clone()],
                max_age: Duration::from_secs(300),
                storage: async_nats::jetstream::stream::StorageType::File,
                num_replicas: 1,
                ..Default::default()
            })
            .await
            .unwrap();
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(durable_consumer_name.clone()),
                filter_subject: subject.clone(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_millis(300),
                max_deliver: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        let binding = "nats-jetstream:m2-support";
        let config = NatsJetStreamTransportConfig::new(
            &stream_name,
            AuthenticatedTransportBinding::new(binding),
        )
        .with_consumer(
            CONSUMER_ID,
            NatsJetStreamConsumerBinding {
                subject,
                durable_consumer_name,
            },
        )
        .with_receive_timeout(Duration::from_millis(100));
        let adapter = NatsJetStreamTransportAdapter::bind(
            async_nats::connect(&nats_url).await.unwrap(),
            diagnostic_store.pool.clone(),
            config.clone(),
        )
        .await
        .unwrap();
        let consumer_contract =
            serde_json::from_str(include_str!("../services/support-sla/lenso.service.json"))
                .unwrap();
        let consumer_state = prepare_runtime(
            &consumer_contract,
            &lenso_autonomous_service::ServiceRuntimeConfig::new(
                SLA_SERVICE,
                "support-sla-store",
                SLA_SERVICE,
            ),
            consumer_store.pool.clone(),
            &[platform_core::Migration {
                name: "support-sla/0001_create_escalations",
                sql: "create table support_sla_escalations (source_event_id text primary key, ticket_id text not null, actor_subject text not null, tenant_id text not null);",
            }],
        )
        .await
        .unwrap();
        let now = chrono::Utc::now();
        let IdentityMaterial {
            workload_provider,
            context_provider,
            credential,
            actor,
            tenant,
        } = identity_material(now, binding).unwrap();
        let admission = event_admission(workload_provider, context_provider);
        let envelope = support_event(
            "m2-production-support-event",
            "ticket_m2_production",
            now,
            credential.service_principal_context(),
            actor,
            tenant,
        )
        .unwrap();
        adapter
            .publish(TransportPublication {
                consumer_id: CONSUMER_ID.to_owned(),
                envelope: envelope.clone(),
            })
            .await
            .unwrap();
        let interrupted = adapter.receive(CONSUMER_ID, 1).await.unwrap();
        assert_eq!(interrupted.len(), 1);
        drop(interrupted);
        drop(adapter);
        tokio::time::sleep(Duration::from_millis(350)).await;
        let restarted_adapter = NatsJetStreamTransportAdapter::bind(
            async_nats::connect(&nats_url).await.unwrap(),
            diagnostic_store.pool.clone(),
            config,
        )
        .await
        .unwrap();
        assert_eq!(
            consume_service_events_once_at(
                &consumer_state,
                &restarted_adapter,
                CONSUMER_ID,
                &SupportSlaHandler,
                1,
                now + chrono::Duration::seconds(1),
                &ServiceEventRetryPolicy::default(),
                &admission,
            )
            .await
            .unwrap(),
            1
        );
        restarted_adapter
            .publish(TransportPublication {
                consumer_id: CONSUMER_ID.to_owned(),
                envelope,
            })
            .await
            .unwrap();
        assert_eq!(
            consume_service_events_once_at(
                &consumer_state,
                &restarted_adapter,
                CONSUMER_ID,
                &SupportSlaHandler,
                1,
                now + chrono::Duration::seconds(2),
                &ServiceEventRetryPolicy::default(),
                &admission,
            )
            .await
            .unwrap(),
            0
        );
        let (effects, actor, tenant): (i64, String, String) = sqlx::query_as(
            "select count(*) over (), actor_subject, tenant_id from support_sla_escalations where source_event_id = 'm2-production-support-event'",
        )
        .fetch_one(&consumer_store.pool)
        .await
        .unwrap();
        assert_eq!(
            (effects, actor.as_str(), tenant.as_str()),
            (1, "user_01", "tenant_01")
        );

        drop(consumer_state);
        drop(restarted_adapter);
        context.delete_stream(&stream_name).await.unwrap();
        consumer_store.cleanup().await;
        diagnostic_store.cleanup().await;
    }
}
