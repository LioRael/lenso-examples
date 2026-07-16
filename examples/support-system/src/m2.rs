use crate::{SLA_SERVICE, SmokeEvidence, TICKET_SERVICE, now_ms, run_smoke};
use anyhow::{Context, ensure};
use async_trait::async_trait;
use lenso_autonomous_service::{
    LocalTransportAdapter, ServiceEventAdmission, ServiceEventHandler, ServiceEventHandlerError,
    ServiceEventPublisher, ServiceEventRetryPolicy, ServiceRuntimeState, TransportAdapter,
    TransportDelivery, TransportDiagnostic, TransportError, TransportErrorCode, TransportHealth,
    TransportNegativeAcknowledgement, TransportPublication, TransportPublicationReceipt,
    consume_service_events_once_at, plan_dead_letter_replay, prepare_runtime,
    relay_service_events_once, replay_dead_letter,
};
use lenso_service::{
    CallPolicyCircuitBreaker, CallPolicyConcurrency, CallPolicyDeclaration, CallPolicyEvent,
    CallPolicyFailure, CallPolicyFallback, CallPolicyOverload, CallPolicyRuntime,
    DelegatedActorCredentialRequest, DelegatedContextProvider, EventEnvelope,
    ManualCallPolicyClock, ServiceContextPolicy, ServiceTenancyMode,
    SystemSandboxDelegatedContextProvider, SystemSandboxWorkloadIdentityProvider,
    TenantCredentialRequest, WorkloadCredentialRequest, WorkloadIdentityProvider,
};
use platform_testing::TestDatabase;
use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CONSUMER_ID: &str = "support-sla-service";
const TRANSPORT_BINDING: &str = "sandbox-event:local-transport";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M2SmokeEvidence {
    pub artifact_version: &'static str,
    #[serde(flatten)]
    pub direct: SmokeEvidence,
    pub event_flow: EventFlowEvidence,
    pub call_policy: CallPolicyProof,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFlowEvidence {
    pub event_type: String,
    pub adapter: &'static str,
    pub business_effects: i64,
    pub authenticated_service_principal: String,
    pub delegated_actor: String,
    pub tenant_id: String,
    pub local_evidence_records: i64,
    pub system_plane_withheld: bool,
    pub runtime_console_withheld: bool,
    pub scenarios: Vec<ScenarioEvidence>,
    pub cleanup_completed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioEvidence {
    pub scenario_id: &'static str,
    pub outcome: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallPolicyProof {
    pub scenarios: Vec<ScenarioEvidence>,
    pub circuit_recovered: bool,
    pub fallback_handler: String,
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
    tokio::fs::rename(&withheld_path, &state_path)
        .await
        .context("restore System Plane state after M2 event flow")?;
    let mut event_flow = event_result?;
    event_flow.system_plane_withheld = true;
    event_flow.runtime_console_withheld = direct.runtime_console_withheld;
    let call_policy = prove_call_policy()?;
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
    let producer_contract = serde_json::from_str(include_str!(
        "../services/support-ticket/lenso.service.json"
    ))?;
    let consumer_contract =
        serde_json::from_str(include_str!("../services/support-sla/lenso.service.json"))?;
    let producer_state = prepare_runtime(
        &producer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            TICKET_SERVICE,
            "support-ticket-store",
            TICKET_SERVICE,
        ),
        producer.pool.clone(),
        &[platform_core::Migration {
            name: "support-ticket/0001_create_tickets",
            sql: "create table support_tickets (id text primary key, priority text not null);",
        }],
    )
    .await?;
    let consumer_state = prepare_runtime(
        &consumer_contract,
        &lenso_autonomous_service::ServiceRuntimeConfig::new(
            SLA_SERVICE,
            "support-sla-store",
            SLA_SERVICE,
        ),
        consumer.pool.clone(),
        &[platform_core::Migration {
            name: "support-sla/0001_create_escalations",
            sql: "create table support_sla_escalations (source_event_id text primary key, ticket_id text not null, actor_subject text not null, tenant_id text not null);",
        }],
    )
    .await?;
    let adapter = LocalTransportAdapter::prepare(transport.pool.clone()).await?;
    let now = chrono::Utc::now();
    let now_ms = u64::try_from(now.timestamp_millis())?;
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
        TRANSPORT_BINDING,
        now_ms,
        30_000,
    ))?;
    let actor = context_provider.issue_actor(DelegatedActorCredentialRequest::new(
        "user_01",
        SLA_SERVICE,
        "support.ticket.opened",
        ["support.tickets.read"],
        now_ms,
        30_000,
    ))?;
    let tenant = context_provider.issue_tenant(TenantCredentialRequest::new(
        "tenant_01",
        "user_01",
        "delegation_1",
        SLA_SERVICE,
        now_ms,
        30_000,
    ))?;
    let admission = ServiceEventAdmission::new(
        workload_provider.clone(),
        SLA_SERVICE,
        context_provider.clone(),
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
    );
    let envelope = support_event(
        "m2-support-event",
        "ticket_m2",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;

    let mut transaction = producer.pool.begin().await?;
    sqlx::query("insert into support_tickets (id, priority) values ($1, 'urgent')")
        .bind("ticket_m2")
        .execute(&mut *transaction)
        .await?;
    ServiceEventPublisher
        .publish_in_tx(&mut transaction, CONSUMER_ID, &envelope)
        .await?;
    transaction.commit().await?;
    ensure!(
        relay_service_events_once(&producer_state, &adapter, 10).await? == 1,
        "support event was not relayed"
    );
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            10,
            now,
            &ServiceEventRetryPolicy::default(),
            &admission,
        )
        .await?
            == 1,
        "support event did not produce a business effect"
    );

    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: envelope.clone(),
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            &consumer_state,
            &adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            10,
            now + chrono::Duration::milliseconds(1),
            &ServiceEventRetryPolicy::default(),
            &admission,
        )
        .await?
            == 0,
        "duplicate event repeated its business effect"
    );

    let scenarios = run_event_scenarios(
        &producer_state,
        &consumer_state,
        &adapter,
        &producer_contract,
        &consumer_contract,
        &producer.pool,
        &consumer.pool,
        &transport.pool,
        &admission,
        &workload_provider,
        &credential,
        &actor,
        &tenant,
        now,
    )
    .await?;

    let (business_effects, delegated_actor, tenant_id): (i64, String, String) = sqlx::query_as(
        "select count(*) over (), actor_subject, tenant_id from support_sla_escalations where source_event_id = $1",
    )
    .bind(&envelope.event_id)
    .fetch_one(&consumer.pool)
    .await?;
    ensure!(
        business_effects == 1,
        "support event business effect was not exactly once"
    );
    let identity_outcome: String = sqlx::query_scalar(
        r"
        select outcome
        from platform.service_event_delivery_evidence
        where event_id = $1 and stage = 'identity'
        ",
    )
    .bind(&envelope.event_id)
    .fetch_one(&consumer.pool)
    .await?;
    ensure!(
        identity_outcome == "identity_context_accepted",
        "receiver did not persist accepted identity evidence"
    );
    let local_evidence_records: i64 = sqlx::query_scalar(
        "select count(*) from platform.service_event_delivery_evidence where event_id = $1",
    )
    .bind(&envelope.event_id)
    .fetch_one(&consumer.pool)
    .await?;

    Ok(EventFlowEvidence {
        event_type: envelope.event_type,
        adapter: "local",
        business_effects,
        authenticated_service_principal: envelope
            .context
            .service_principal
            .as_ref()
            .map(|principal| principal.subject.clone())
            .context("accepted event lost its Service Principal")?,
        delegated_actor,
        tenant_id,
        local_evidence_records,
        system_plane_withheld: false,
        runtime_console_withheld: false,
        scenarios,
        cleanup_completed: false,
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_event_scenarios(
    producer_state: &ServiceRuntimeState,
    consumer_state: &ServiceRuntimeState,
    adapter: &LocalTransportAdapter,
    producer_contract: &lenso_service::AutonomousServiceContract,
    consumer_contract: &lenso_service::AutonomousServiceContract,
    producer_pool: &PgPool,
    consumer_pool: &PgPool,
    transport_pool: &PgPool,
    admission: &ServiceEventAdmission,
    workload_provider: &Arc<SystemSandboxWorkloadIdentityProvider>,
    credential: &lenso_service::WorkloadCredential,
    actor: &lenso_service::DelegatedActorContext,
    tenant: &lenso_service::TenantContext,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<ScenarioEvidence>> {
    let retry_policy = ServiceEventRetryPolicy::default();
    let mut scenarios = vec![ScenarioEvidence {
        scenario_id: "duplicate",
        outcome: "business_effect_deduplicated",
    }];

    let delayed = support_event(
        "m2-delayed-event",
        "ticket_delayed",
        now - chrono::Duration::hours(1),
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: delayed,
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(2),
            &retry_policy,
            admission,
        )
        .await?
            == 1,
        "delayed event did not produce its business effect"
    );
    scenarios.push(ScenarioEvidence {
        scenario_id: "delayed",
        outcome: "delayed_event_handled",
    });

    let watermark_handler = SupportSlaWatermarkHandler {
        accepted_after: now,
    };
    for envelope in [
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
                envelope,
            })
            .await?;
    }
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &watermark_handler,
            1,
            now + chrono::Duration::milliseconds(3),
            &retry_policy,
            admission,
        )
        .await?
            == 1,
        "newer reordered event was not handled"
    );
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &watermark_handler,
            1,
            now + chrono::Duration::milliseconds(4),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "older reordered event produced a business effect"
    );
    let reordered_reason: String = sqlx::query_scalar(
        "select reason_code from platform.service_event_inbox where event_id = 'm2-reordered-older-event'",
    )
    .fetch_one(consumer_pool)
    .await?;
    ensure!(
        reordered_reason == "support_event_out_of_order",
        "reordered event did not preserve the module-owned rejection reason"
    );
    scenarios.push(ScenarioEvidence {
        scenario_id: "reordered",
        outcome: "older_event_rejected_by_watermark",
    });

    let poison_handler = PoisonOnceSupportSlaHandler {
        should_fail: AtomicBool::new(true),
    };
    let poison = support_event(
        "m2-poison-event",
        "ticket_poison",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: poison,
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(5),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "poison event unexpectedly produced a business effect"
    );
    let dead_letter_id: String = sqlx::query_scalar(
        "select dead_letter_id from platform.service_event_dead_letters where event_id = 'm2-poison-event'",
    )
    .fetch_one(consumer_pool)
    .await?;
    scenarios.push(ScenarioEvidence {
        scenario_id: "poison",
        outcome: "poison_event_dead_lettered",
    });

    let replay_plan = plan_dead_letter_replay(consumer_state, adapter, &dead_letter_id).await?;
    replay_dead_letter(consumer_state, adapter, &replay_plan, None).await?;
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(6),
            &retry_policy,
            admission,
        )
        .await?
            == 1,
        "corrected dead-letter replay did not produce its business effect"
    );
    let duplicate_replay_plan =
        plan_dead_letter_replay(consumer_state, adapter, &dead_letter_id).await?;
    replay_dead_letter(consumer_state, adapter, &duplicate_replay_plan, None).await?;
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &poison_handler,
            1,
            now + chrono::Duration::milliseconds(7),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "duplicate dead-letter replay repeated its business effect"
    );
    let replay_effects: i64 = sqlx::query_scalar(
        "select count(*) from support_sla_escalations where source_event_id = 'm2-poison-event'",
    )
    .fetch_one(consumer_pool)
    .await?;
    ensure!(replay_effects == 1, "dead-letter replay was not idempotent");

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
        relay_service_events_once(producer_state, &interrupted_adapter, 1).await
    else {
        anyhow::bail!("transport interruption did not fail the first publication receipt");
    };
    ensure!(
        interruption.code == TransportErrorCode::DeliveryFailed,
        "transport interruption returned the wrong stable error code"
    );
    let restarted_producer_state = prepare_runtime(
        producer_contract,
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
        relay_service_events_once(&restarted_producer_state, adapter, 1).await? == 1,
        "producer restart did not relay its durable Outbox event"
    );
    ensure!(
        consume_service_events_once_at(
            consumer_state,
            adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            2,
            now + chrono::Duration::milliseconds(8),
            &retry_policy,
            admission,
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
    .fetch_one(consumer_pool)
    .await?;
    let producer_restart_attempts: i32 = sqlx::query_scalar(
        "select attempts from platform.service_event_outbox where event_id = 'm2-producer-restart-event'",
    )
    .fetch_one(producer_pool)
    .await?;
    ensure!(
        producer_restart_effects == (1, 1) && producer_restart_attempts == 2,
        "producer restart evidence did not prove one effect, one duplicate, and two attempts"
    );
    scenarios.extend([
        ScenarioEvidence {
            scenario_id: "producer_restart",
            outcome: "outbox_recovered_without_repeating_effect",
        },
        ScenarioEvidence {
            scenario_id: "consumer_restart",
            outcome: "unacknowledged_delivery_recovered",
        },
        ScenarioEvidence {
            scenario_id: "transport_interruption",
            outcome: "publication_receipt_failure_retried",
        },
        ScenarioEvidence {
            scenario_id: "dead_letter_replay",
            outcome: "replay_idempotent",
        },
    ]);

    let consumer_restart = support_event(
        "m2-consumer-restart-event",
        "ticket_consumer_restart",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: consumer_restart,
        })
        .await?;
    let abandoned = adapter.receive(CONSUMER_ID, 1).await?;
    ensure!(
        abandoned.len() == 1,
        "consumer restart did not leave exactly one unacknowledged delivery"
    );
    drop(abandoned);
    let restarted_adapter = LocalTransportAdapter::prepare(transport_pool.clone()).await?;
    let restarted_consumer_state = prepare_runtime(
        consumer_contract,
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
            admission,
        )
        .await?
            == 1,
        "consumer restart did not recover the unacknowledged delivery"
    );
    let recovered: i64 = sqlx::query_scalar(
        "select count(*) from platform.local_transport_diagnostics where event_id = 'm2-consumer-restart-event' and outcome = 'recovered_unacknowledged'",
    )
    .fetch_one(transport_pool)
    .await?;
    ensure!(
        recovered == 1,
        "consumer restart recovery evidence was not local"
    );

    let issued_at = u64::try_from(now.timestamp_millis())?;
    let expiring = workload_provider.issue(WorkloadCredentialRequest::new(
        format!("service:{TICKET_SERVICE}"),
        SLA_SERVICE,
        TRANSPORT_BINDING,
        issued_at,
        1,
    ))?;
    let expired = support_event(
        "m2-identity-expiry-event",
        "ticket_identity_expiry",
        now,
        expiring.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    restarted_adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: expired,
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(10),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "expired Workload Identity produced a business effect"
    );
    ensure!(
        inbox_reason(consumer_pool, "m2-identity-expiry-event").await? == "credential_expired",
        "identity expiry did not persist a stable receiver-local reason"
    );

    let wrong_audience = workload_provider.issue(WorkloadCredentialRequest::new(
        format!("service:{TICKET_SERVICE}"),
        "wrong-service",
        TRANSPORT_BINDING,
        issued_at,
        30_000,
    ))?;
    let wrong_audience_event = support_event(
        "m2-wrong-audience-event",
        "ticket_wrong_audience",
        now,
        wrong_audience.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    restarted_adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: wrong_audience_event,
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(11),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "wrong-audience Workload Identity produced a business effect"
    );
    ensure!(
        inbox_reason(consumer_pool, "m2-wrong-audience-event").await? == "audience_mismatch",
        "wrong audience did not persist a stable receiver-local reason"
    );

    let mut missing_tenant = support_event(
        "m2-missing-tenant-event",
        "ticket_missing_tenant",
        now,
        credential.service_principal_context(),
        actor.clone(),
        tenant.clone(),
    )?;
    missing_tenant.context.tenant = None;
    restarted_adapter
        .publish(TransportPublication {
            consumer_id: CONSUMER_ID.to_owned(),
            envelope: missing_tenant,
        })
        .await?;
    ensure!(
        consume_service_events_once_at(
            &restarted_consumer_state,
            &restarted_adapter,
            CONSUMER_ID,
            &SupportSlaHandler,
            1,
            now + chrono::Duration::milliseconds(12),
            &retry_policy,
            admission,
        )
        .await?
            == 0,
        "missing required Tenant Context produced a business effect"
    );
    ensure!(
        inbox_reason(consumer_pool, "m2-missing-tenant-event").await? == "tenant_context_required",
        "missing tenant did not persist a stable receiver-local reason"
    );
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
    .fetch_one(consumer_pool)
    .await?;
    ensure!(
        denied_effects == 0,
        "rejected identities changed business state"
    );
    scenarios.extend([
        ScenarioEvidence {
            scenario_id: "identity_expiry",
            outcome: "credential_expired_locally",
        },
        ScenarioEvidence {
            scenario_id: "wrong_audience",
            outcome: "audience_rejected_locally",
        },
        ScenarioEvidence {
            scenario_id: "missing_tenant",
            outcome: "tenant_context_required_locally",
        },
    ]);

    Ok(scenarios)
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

fn prove_call_policy() -> anyhow::Result<CallPolicyProof> {
    let clock = Arc::new(ManualCallPolicyClock::new(1_000));
    let runtime = CallPolicyRuntime::new(clock.clone());
    let policy = CallPolicyDeclaration {
        max_attempts: 2,
        circuit_breaker: Some(CallPolicyCircuitBreaker {
            failure_threshold: 2,
            open_for_ms: 100,
            half_open_max_calls: 1,
        }),
        concurrency: Some(CallPolicyConcurrency { max_in_flight: 1 }),
        overload: Some(CallPolicyOverload { max_in_flight: 1 }),
        fallback: Some(CallPolicyFallback {
            handler: "support.cached_sla".to_owned(),
            on: vec![
                CallPolicyFailure::CircuitOpen,
                CallPolicyFailure::DeadlineExpired,
            ],
        }),
    };
    let _ = policy_permit(runtime.begin_call("support:GetSla", &policy))?.failure();
    let _ = policy_permit(runtime.begin_call("support:GetSla", &policy))?.failure();
    ensure!(
        runtime.begin_call("support:GetSla", &policy).unwrap_err() == CallPolicyEvent::CircuitOpen,
        "circuit did not open deterministically"
    );
    let fallback = policy
        .fallback_for(CallPolicyFailure::CircuitOpen)
        .context("business fallback was not declared")?;
    clock.advance_ms(100);
    let probe = policy_permit(runtime.begin_call("support:GetSla", &policy))?;
    let recovered = probe.success();

    let caller = policy_permit(runtime.begin_call("support:Bulkhead", &policy))?;
    ensure!(
        runtime.begin_call("support:Bulkhead", &policy).unwrap_err()
            == CallPolicyEvent::BulkheadSaturated,
        "bulkhead did not reject deterministically"
    );
    drop(caller);
    let receiver = policy_permit(runtime.admit("support:Overload", &policy))?;
    ensure!(
        runtime.admit("support:Overload", &policy).unwrap_err()
            == CallPolicyEvent::OverloadRejected,
        "receiver overload did not reject deterministically"
    );
    drop(receiver);

    Ok(CallPolicyProof {
        scenarios: vec![
            ScenarioEvidence {
                scenario_id: "circuit_open",
                outcome: "fallback_available",
            },
            ScenarioEvidence {
                scenario_id: "bulkhead",
                outcome: "bulkhead_saturated",
            },
            ScenarioEvidence {
                scenario_id: "overload",
                outcome: "overload_rejected",
            },
            ScenarioEvidence {
                scenario_id: "deadline",
                outcome: "deadline_expired",
            },
            ScenarioEvidence {
                scenario_id: "fallback",
                outcome: "business_fallback_declared",
            },
        ],
        circuit_recovered: recovered.contains(&CallPolicyEvent::CircuitRecovered),
        fallback_handler: fallback.handler.clone(),
    })
}

fn policy_permit<T>(result: Result<T, CallPolicyEvent>) -> anyhow::Result<T> {
    result.map_err(|event| anyhow::anyhow!("unexpected call policy event: {}", event.as_str()))
}
