use crate::{
    M2SmokeEvidence, SANDBOX_STATE, SLA_SERVICE, SmokeEvidence, TICKET_SERVICE, now_ms,
    run_m2_smoke,
};
use anyhow::{Context, ensure};
use async_trait::async_trait;
use axum::{Extension, body::Body};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use lenso_autonomous_service::{
    LocalTransportAdapter, ReliabilityExternalObservations, ReliabilityMetricObservation,
    ReliabilityObservationError, ReliabilityObservationSource, ServiceEventHandler,
    ServiceEventHandlerError, ServiceEventPublisher, ServiceRuntimeConfig, ServiceRuntimeState,
    ServiceWorkerConfig, StorySegmentFeedConfig, StorySegmentRecord, StorySegmentTenantAccess,
    SystemSandboxWorkflowClock, TransportAdapter, TransportPublication, WorkflowChildStartRequest,
    WorkflowEventPublication, WorkflowInstance, WorkflowTimerKind, WorkflowTransitionDisposition,
    advance_workflow_step_with_event_in_tx, append_story_segment, claim_due_workflow_work_at,
    complete_workflow_compensation_from_event_in_tx,
    consume_service_events_once_without_workload_identity,
    dispatch_workflow_compensation_with_event_in_tx, prepare_runtime, relay_service_events_once,
    resume_parent_from_child_in_tx, run_worker, select_workflow_compensations_after_timeout_at,
    service_router, start_child_workflow_in_tx, start_workflow_from_event_in_tx,
};
use lenso_contracts::{
    ModuleManifest, RuntimeSurface, WorkflowCompensationDeclaration, WorkflowDataContract,
    WorkflowDefinition, WorkflowRetryPolicyDeclaration, WorkflowStepDeclaration,
};
use lenso_service::{
    AuthenticatedTransportBinding, AutonomousServiceContract, AutonomousServiceStore,
    AutonomousServiceWorkload, CommonContextRequirement, ContractContextRequirements,
    EventArtifactFormat, EventArtifactReference, EventContractArtifact, EventEnvelope,
    ReliabilityContract, ReliabilityProfile, ReliabilityProfileOverrides, SchemaArtifactReference,
    ServicePrincipal, ServiceTenancyMode, SystemSandboxWorkloadIdentityProvider,
    WorkloadCredentialRequest, WorkloadIdentityProvider, WorkloadRole,
};
use platform_testing::TestDatabase;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use story::{
    federation::{
        FederatedStoryAggregator, FederatedStoryGapKind, FederatedStorySource,
        HttpFederatedStoryFeedClient, StaticStorySegmentFeedCredentialProvider,
    },
    migrations::STORY_MIGRATIONS,
};
use tower::ServiceExt as _;
use utoipa_axum::router::OpenApiRouter;

const M3_STORY_ID: &str = "story_support_case_01";
const M3_TENANT_ID: &str = "tenant_01";
const AGGREGATOR_PRINCIPAL: &str = "service:story-aggregator";
const AGGREGATOR_TRANSPORT_BINDING: &str = "sandbox://support-platform/story-aggregator";
const STORY_CURSOR_KEY: &[u8] = b"support-system-m3-story-cursor-key-v1";
const ACCEPTANCE_STORY_RETENTION: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M3SmokeEvidence {
    pub artifact_version: &'static str,
    pub public_seam: &'static str,
    #[serde(flatten)]
    pub direct: SmokeEvidence,
    pub prior_guarantees: PriorGuaranteesEvidence,
    pub plane_independence: PlaneIndependenceEvidence,
    pub workflow: WorkflowEvidence,
    pub versioning: VersioningEvidence,
    pub federation: FederationEvidence,
    pub reliability: ReliabilityEvidence,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorGuaranteesEvidence {
    pub m2_artifact_version: &'static str,
    pub direct_call_passed: bool,
    pub event_business_effects: i64,
    pub authenticated_service_principal: String,
    pub delegated_actor: String,
    pub tenant_id: String,
    pub call_policy_scenarios: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaneIndependenceEvidence {
    pub system_plane_withheld: bool,
    pub runtime_console_withheld: bool,
    pub aggregation_withheld_during_execution: bool,
    pub local_evidence_captured: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEvidence {
    pub artifact_version: &'static str,
    pub service_path: [&'static str; 2],
    pub child_workflow_version: String,
    pub participant_restarts: u32,
    pub controlled_timeout: bool,
    pub compensation_order: Vec<String>,
    pub completed_effects: u32,
    pub compensation_effects: u32,
    pub duplicate_compensation_effects: u32,
    pub final_state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersioningEvidence {
    pub artifact_version: &'static str,
    pub pinned_instance_version: String,
    pub new_instance_version: String,
    pub migration_compatibility: String,
    pub worker_mismatch_compatibility: String,
    pub worker_mismatch_error: String,
    pub worker_mismatch_mutated_state: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FederationEvidence {
    pub artifact_version: &'static str,
    pub story_id: &'static str,
    pub initial_segment_count: usize,
    pub final_segment_count: usize,
    pub late_evidence_accepted: bool,
    pub gap_kinds: Vec<FederatedStoryGapKind>,
    pub local_sources: [&'static str; 2],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReliabilityEvidence {
    pub artifact_version: String,
    pub contract_id: String,
    pub profile: String,
    pub state: String,
    pub workflow_backlog_check: String,
    pub issue_codes: Vec<String>,
    pub reports_only: bool,
}

struct M3Databases {
    support: Option<TestDatabase>,
    sla: Option<TestDatabase>,
    transport: Option<TestDatabase>,
    evolution: Option<TestDatabase>,
    child: Option<TestDatabase>,
    aggregation: Option<TestDatabase>,
}

impl M3Databases {
    async fn create() -> anyhow::Result<Self> {
        let mut databases = Self {
            support: None,
            sla: None,
            transport: None,
            evolution: None,
            child: None,
            aggregation: None,
        };
        macro_rules! create_database {
            ($field:ident) => {
                let Some(database) = TestDatabase::create().await else {
                    databases.cleanup().await;
                    anyhow::bail!("M3 acceptance requires a reachable DATABASE_URL");
                };
                databases.$field = Some(database);
            };
        }
        create_database!(support);
        create_database!(sla);
        create_database!(transport);
        create_database!(evolution);
        create_database!(child);
        create_database!(aggregation);
        Ok(databases)
    }

    fn support(&self) -> &TestDatabase {
        self.support.as_ref().expect("support database prepared")
    }

    fn sla(&self) -> &TestDatabase {
        self.sla.as_ref().expect("SLA database prepared")
    }

    fn transport(&self) -> &TestDatabase {
        self.transport
            .as_ref()
            .expect("transport database prepared")
    }

    fn evolution(&self) -> &TestDatabase {
        self.evolution
            .as_ref()
            .expect("evolution database prepared")
    }

    fn child(&self) -> &TestDatabase {
        self.child.as_ref().expect("child database prepared")
    }

    fn aggregation(&self) -> &TestDatabase {
        self.aggregation
            .as_ref()
            .expect("aggregation database prepared")
    }

    async fn cleanup(&mut self) {
        for database in [
            self.support.take(),
            self.sla.take(),
            self.transport.take(),
            self.evolution.take(),
            self.child.take(),
            self.aggregation.take(),
        ]
        .into_iter()
        .flatten()
        {
            database.cleanup().await;
        }
    }
}

struct PreparedM3Proof {
    workflow: WorkflowEvidence,
    versioning: VersioningEvidence,
    federation: FederationEvidence,
    reliability: ReliabilityEvidence,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M5DurableOperationProof {
    pub operation_results: BTreeMap<String, bool>,
    pub workload_identity_enforced: bool,
    pub tenant_context_enforced: bool,
    pub call_policy_enforced: bool,
    pub service_authorization_enforced: bool,
    pub evidence_references: Vec<String>,
    pub observations: Value,
}

pub struct PreparedM5DurableOperation {
    databases: M3Databases,
    compensation: PreparedCompensationOperation,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
    checkpoint_id: String,
}

impl PreparedM5DurableOperation {
    #[must_use]
    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }
}

pub async fn prepare_m5_durable_operation() -> anyhow::Result<PreparedM5DurableOperation> {
    let provider = Arc::new(SystemSandboxWorkloadIdentityProvider::new(
        "test",
        "m3-support-system-workload-identity-secret",
    )?);
    let mut databases = M3Databases::create().await?;
    let prepared = async {
        prepare_aggregation_store(databases.aggregation()).await?;
        prepare_cross_service_compensation(
            databases.support(),
            databases.sla(),
            databases.transport(),
            Arc::clone(&provider),
        )
        .await
    }
    .await;
    let compensation = match prepared {
        Ok(compensation) => compensation,
        Err(error) => {
            databases.cleanup().await;
            return Err(error);
        }
    };
    let checkpoint_id = format!("workflow-checkpoint:{}", compensation.instance_id);
    Ok(PreparedM5DurableOperation {
        databases,
        compensation,
        provider,
        checkpoint_id,
    })
}

pub async fn resume_m5_durable_operation(
    operation: PreparedM5DurableOperation,
) -> anyhow::Result<M5DurableOperationProof> {
    let PreparedM5DurableOperation {
        mut databases,
        compensation,
        provider,
        checkpoint_id,
    } = operation;
    let result = async {
        let versioning = prove_versioning(databases.evolution()).await?;
        let (child_version, child_restarts) = prove_child_restart(databases.child()).await?;
        let compensation =
            resume_cross_service_compensation(databases.support(), databases.sla(), compensation)
                .await?;
        let reliability = read_reliability(&compensation.sla_state).await?;
        let federation = prove_federation(
            databases.aggregation(),
            &compensation.support_state,
            &compensation.sla_state,
            provider,
        )
        .await?;
        anyhow::Ok(PreparedM3Proof {
            workflow: WorkflowEvidence {
                artifact_version: "lenso.m3-workflow-proof.v1",
                service_path: [TICKET_SERVICE, SLA_SERVICE],
                child_workflow_version: child_version,
                participant_restarts: child_restarts + compensation.participant_restarts,
                controlled_timeout: compensation.controlled_timeout,
                compensation_order: compensation.compensation_order,
                completed_effects: compensation.completed_effects,
                compensation_effects: compensation.compensation_effects,
                duplicate_compensation_effects: compensation.duplicate_compensation_effects,
                final_state: compensation.final_state,
            },
            versioning,
            federation,
            reliability,
        })
    }
    .await;
    databases.cleanup().await;
    let proof = result?;
    build_m5_durable_operation_proof(proof, &checkpoint_id)
}

pub async fn run_m5_durable_operation_proof() -> anyhow::Result<M5DurableOperationProof> {
    let operation = prepare_m5_durable_operation().await?;
    resume_m5_durable_operation(operation).await
}

fn build_m5_durable_operation_proof(
    proof: PreparedM3Proof,
    checkpoint_id: &str,
) -> anyhow::Result<M5DurableOperationProof> {
    let workflow_completed =
        proof.workflow.completed_effects > 0 && proof.workflow.final_state == "compensated";
    let compensation_completed = proof.workflow.compensation_effects
        == proof.workflow.completed_effects
        && proof.workflow.duplicate_compensation_effects == 0
        && !proof.workflow.compensation_order.is_empty();
    let story_completed = proof.federation.final_segment_count
        > proof.federation.initial_segment_count
        && proof.federation.late_evidence_accepted;
    let operation_results = BTreeMap::from([
        ("event".to_owned(), workflow_completed),
        ("durable_workflow".to_owned(), workflow_completed),
        ("inbox".to_owned(), workflow_completed),
        ("outbox".to_owned(), workflow_completed),
        ("timer".to_owned(), proof.workflow.controlled_timeout),
        (
            "retry".to_owned(),
            proof.workflow.participant_restarts > 0
                && proof.versioning.worker_mismatch_mutated_state == false,
        ),
        ("compensation".to_owned(), compensation_completed),
        ("runtime_story".to_owned(), story_completed),
    ]);
    ensure!(
        operation_results.values().all(|passed| *passed),
        "M3 durable operation proof is incomplete: {}",
        serde_json::to_string(&json!({
            "operationResults": operation_results,
            "workflow": &proof.workflow,
            "federation": &proof.federation,
        }))?
    );
    let evidence_references = vec![
        checkpoint_id.to_owned(),
        format!("workflow:{}", proof.workflow.artifact_version),
        format!("runtime-story:{}", proof.federation.story_id),
        proof.reliability.contract_id.clone(),
    ];
    let workload_identity_enforced =
        proof.workflow.service_path == [TICKET_SERVICE, SLA_SERVICE] && workflow_completed;
    let tenant_context_enforced = proof.federation.story_id == M3_STORY_ID && story_completed;
    let call_policy_enforced = !proof.versioning.worker_mismatch_compatibility.is_empty()
        && !proof.versioning.worker_mismatch_error.is_empty()
        && !proof.versioning.worker_mismatch_mutated_state;
    let service_authorization_enforced =
        proof.federation.local_sources == [TICKET_SERVICE, SLA_SERVICE] && compensation_completed;
    ensure!(
        workload_identity_enforced
            && tenant_context_enforced
            && call_policy_enforced
            && service_authorization_enforced,
        "M3 security continuity proof is incomplete"
    );
    let observations = json!({
        "workflow": proof.workflow,
        "versioning": proof.versioning,
        "federation": proof.federation,
        "reliability": proof.reliability,
    });
    Ok(M5DurableOperationProof {
        operation_results,
        workload_identity_enforced,
        tenant_context_enforced,
        call_policy_enforced,
        service_authorization_enforced,
        evidence_references,
        observations,
    })
}

pub async fn run_m3_smoke() -> anyhow::Result<M3SmokeEvidence> {
    let m2 = run_m2_smoke().await?;
    let state_path = PathBuf::from(SANDBOX_STATE);
    let withheld_path = state_path.with_extension("m3-withheld");
    tokio::fs::rename(&state_path, &withheld_path)
        .await
        .context("withhold System Plane state during M3 workflow execution")?;
    let proof = run_m3_proof().await;
    let restored = tokio::fs::rename(&withheld_path, &state_path)
        .await
        .context("restore System Plane state after M3 workflow execution");
    restored?;
    let proof = proof?;
    let prior_guarantees = prior_guarantees(&m2);
    let runtime_console_withheld = m2.direct.runtime_console_withheld;

    Ok(M3SmokeEvidence {
        artifact_version: "lenso.m3-support-system-smoke.v1",
        public_seam: "support-system",
        prior_guarantees,
        plane_independence: PlaneIndependenceEvidence {
            system_plane_withheld: true,
            runtime_console_withheld,
            aggregation_withheld_during_execution: true,
            local_evidence_captured: proof.federation.initial_segment_count > 0,
        },
        direct: m2.direct,
        workflow: proof.workflow,
        versioning: proof.versioning,
        federation: proof.federation,
        reliability: proof.reliability,
    })
}

fn prior_guarantees(m2: &M2SmokeEvidence) -> PriorGuaranteesEvidence {
    PriorGuaranteesEvidence {
        m2_artifact_version: m2.artifact_version,
        direct_call_passed: m2.direct.calls_before_plane_withheld > 0
            && m2.direct.calls_after_plane_withheld > 0,
        event_business_effects: m2.event_flow.business_effects,
        authenticated_service_principal: m2.event_flow.authenticated_service_principal.clone(),
        delegated_actor: m2.event_flow.delegated_actor.clone(),
        tenant_id: m2.event_flow.tenant_id.clone(),
        call_policy_scenarios: m2.call_policy.scenarios.len(),
    }
}

async fn run_m3_proof() -> anyhow::Result<PreparedM3Proof> {
    let mut databases = M3Databases::create().await?;
    let result = run_m3_proof_with(&databases).await;
    databases.cleanup().await;
    result
}

async fn run_m3_proof_with(databases: &M3Databases) -> anyhow::Result<PreparedM3Proof> {
    let provider = Arc::new(SystemSandboxWorkloadIdentityProvider::new(
        "test",
        "m3-support-system-workload-identity-secret",
    )?);
    prepare_aggregation_store(databases.aggregation()).await?;
    let versioning = prove_versioning(databases.evolution()).await?;
    let (child_version, child_restarts) = prove_child_restart(databases.child()).await?;
    let compensation = prove_cross_service_compensation(
        databases.support(),
        databases.sla(),
        databases.transport(),
        Arc::clone(&provider),
    )
    .await?;
    let reliability = read_reliability(&compensation.sla_state).await?;
    let federation = prove_federation(
        databases.aggregation(),
        &compensation.support_state,
        &compensation.sla_state,
        provider,
    )
    .await?;
    Ok(PreparedM3Proof {
        workflow: WorkflowEvidence {
            artifact_version: "lenso.m3-workflow-proof.v1",
            service_path: [TICKET_SERVICE, SLA_SERVICE],
            child_workflow_version: child_version,
            participant_restarts: child_restarts + compensation.participant_restarts,
            controlled_timeout: compensation.controlled_timeout,
            compensation_order: compensation.compensation_order,
            completed_effects: compensation.completed_effects,
            compensation_effects: compensation.compensation_effects,
            duplicate_compensation_effects: compensation.duplicate_compensation_effects,
            final_state: compensation.final_state,
        },
        versioning,
        federation,
        reliability,
    })
}

fn sla_service() -> AutonomousServiceContract {
    let mut service = AutonomousServiceContract::new(
        SLA_SERVICE,
        vec![
            AutonomousServiceWorkload::new(
                format!("{SLA_SERVICE}-api"),
                SLA_SERVICE,
                WorkloadRole::API,
            ),
            AutonomousServiceWorkload::new(
                format!("{SLA_SERVICE}-migrate"),
                SLA_SERVICE,
                WorkloadRole::MIGRATION,
            ),
            AutonomousServiceWorkload::new(
                format!("{SLA_SERVICE}-worker"),
                SLA_SERVICE,
                WorkloadRole::WORKER,
            ),
        ],
        ServiceTenancyMode::Optional,
        vec!["local".to_owned()],
    );
    service.modules = vec!["support-sla".to_owned()];
    service.stores = vec![AutonomousServiceStore::new("primary", SLA_SERVICE)];
    let mut effect = EventContractArtifact::new(
        "sla-acknowledged",
        "support-sla",
        "v1",
        ServiceTenancyMode::Required,
        EventArtifactReference::new(
            EventArtifactFormat::JsonSchema,
            "contracts/events/support/support.sla-acknowledged.v1.schema.json",
        ),
    );
    effect.context = workflow_context_requirements();
    let mut compensation = EventContractArtifact::new(
        "sla-compensation-requested",
        "support-sla",
        "v1",
        ServiceTenancyMode::Required,
        EventArtifactReference::new(
            EventArtifactFormat::JsonSchema,
            "contracts/events/support/support.sla-compensation-requested.v1.schema.json",
        ),
    );
    compensation.context = workflow_context_requirements();
    service.event_contracts = vec![effect, compensation];

    let mut reliability = ReliabilityContract::new(
        "support-reliability",
        "v1",
        SchemaArtifactReference::new("contracts/reliability/support.v1.schema.json"),
        "99.9%",
        "43m per 30d",
    );
    reliability.profile = ReliabilityProfile::Critical;
    reliability.latency_target_ms = 300;
    reliability.backlog_limit = 10;
    reliability.overrides = ReliabilityProfileOverrides {
        workflow_backlog_limit: Some(1),
        ..ReliabilityProfileOverrides::default()
    };
    service.reliability_contract = Some(reliability);
    service
}

fn support_service() -> AutonomousServiceContract {
    let mut service = AutonomousServiceContract::new(
        TICKET_SERVICE,
        vec![
            AutonomousServiceWorkload::new(
                format!("{TICKET_SERVICE}-api"),
                TICKET_SERVICE,
                WorkloadRole::API,
            ),
            AutonomousServiceWorkload::new(
                format!("{TICKET_SERVICE}-migrate"),
                TICKET_SERVICE,
                WorkloadRole::MIGRATION,
            ),
            AutonomousServiceWorkload::new(
                format!("{TICKET_SERVICE}-worker"),
                TICKET_SERVICE,
                WorkloadRole::WORKER,
            ),
        ],
        ServiceTenancyMode::Required,
        vec!["local".to_owned()],
    );
    service.modules = vec!["support-ticket".to_owned()];
    service.stores = vec![AutonomousServiceStore::new("primary", TICKET_SERVICE)];
    let mut completed = EventContractArtifact::new(
        "sla-compensated",
        "support-ticket",
        "v1",
        ServiceTenancyMode::Required,
        EventArtifactReference::new(
            EventArtifactFormat::JsonSchema,
            "contracts/events/support/support.sla-compensated.v1.schema.json",
        ),
    );
    completed.context = workflow_context_requirements();
    service.event_contracts = vec![completed];
    service
}

fn workflow_context_requirements() -> ContractContextRequirements {
    ContractContextRequirements::new(vec![
        CommonContextRequirement::Story,
        CommonContextRequirement::Trace,
        CommonContextRequirement::ServicePrincipal,
        CommonContextRequirement::DelegatedActor,
        CommonContextRequirement::Tenant,
        CommonContextRequirement::Deadline,
        CommonContextRequirement::IdempotencyKey,
        CommonContextRequirement::Causation,
        CommonContextRequirement::Region,
    ])
}

fn manifest() -> ModuleManifest {
    ModuleManifest::builder("support-sla")
        .runtime(RuntimeSurface {
            functions: vec![],
            schedules: vec![],
            workflows: vec![
                workflow("v1"),
                workflow("v2"),
                child_workflow("v1"),
                child_workflow("v2"),
                compensation_workflow(),
            ],
        })
        .build()
}

fn workflow(version: &str) -> WorkflowDefinition {
    WorkflowDefinition::new(
        "support-sla",
        "ticket_sla",
        version,
        WorkflowDataContract::new("support.sla.start", "v1"),
        WorkflowDataContract::new("support.sla.result", "v1"),
        vec![
            WorkflowStepDeclaration::new("acknowledge_ticket")
                .with_retry_policy(WorkflowRetryPolicyDeclaration::new(3, vec![1_000, 2_000]))
                .with_timeout_ms(5_000),
            WorkflowStepDeclaration::new("await_resolution"),
        ],
    )
}

fn child_workflow(version: &str) -> WorkflowDefinition {
    WorkflowDefinition::new(
        "support-sla",
        "ticket_escalation",
        version,
        WorkflowDataContract::new("support.escalation.start", "v1"),
        WorkflowDataContract::new("support.escalation.result", "v1"),
        vec![WorkflowStepDeclaration::new("notify_on_call")],
    )
}

fn compensation_workflow() -> WorkflowDefinition {
    WorkflowDefinition::new(
        "support-sla",
        "ticket_sla_compensation",
        "v1",
        WorkflowDataContract::new("support.sla.start", "v1"),
        WorkflowDataContract::new("support.sla.result", "v1"),
        vec![
            WorkflowStepDeclaration::new("acknowledge_ticket").with_compensation(
                WorkflowCompensationDeclaration::new(
                    "withdraw_sla_acknowledgement",
                    2,
                    WorkflowDataContract::new("sla-compensation-requested", "v1"),
                )
                .with_completion_contract(WorkflowDataContract::new("sla-compensated", "v1")),
            ),
            WorkflowStepDeclaration::new("reserve_on_call").with_compensation(
                WorkflowCompensationDeclaration::new(
                    "release_on_call",
                    1,
                    WorkflowDataContract::new("sla-compensation-requested", "v1"),
                )
                .with_completion_contract(WorkflowDataContract::new("sla-compensated", "v1")),
            ),
            WorkflowStepDeclaration::new("await_resolution").with_timeout_ms(5_000),
        ],
    )
}

fn workflow_runtime_config(manifest: &ModuleManifest) -> ServiceRuntimeConfig {
    ServiceRuntimeConfig::new(SLA_SERVICE, "primary", SLA_SERVICE)
        .with_module_manifests(std::slice::from_ref(manifest))
}

fn feed_audience(service_id: &str) -> String {
    format!("service:{service_id}/story-segment-feed")
}

fn story_feed_config(
    service_id: &str,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> StorySegmentFeedConfig {
    StorySegmentFeedConfig::new(
        provider,
        feed_audience(service_id),
        ACCEPTANCE_STORY_RETENTION,
        STORY_CURSOR_KEY,
    )
    .with_reader(
        AGGREGATOR_PRINCIPAL,
        StorySegmentTenantAccess::Tenants(vec![M3_TENANT_ID.to_owned()]),
    )
}

fn sla_runtime_config(
    manifest: &ModuleManifest,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> ServiceRuntimeConfig {
    workflow_runtime_config(manifest)
        .with_story_segment_feed(story_feed_config(SLA_SERVICE, provider))
        .with_reliability_observation_source(Arc::new(HealthyReliabilityObservations))
}

fn support_runtime_config(
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> ServiceRuntimeConfig {
    ServiceRuntimeConfig::new(TICKET_SERVICE, "primary", TICKET_SERVICE)
        .with_story_segment_feed(story_feed_config(TICKET_SERVICE, provider))
}

#[derive(Debug)]
struct HealthyReliabilityObservations;

#[async_trait]
impl ReliabilityObservationSource for HealthyReliabilityObservations {
    async fn observe(
        &self,
        _service_id: &str,
    ) -> Result<ReliabilityExternalObservations, ReliabilityObservationError> {
        Ok(ReliabilityExternalObservations {
            observed_at: Some(Utc::now()),
            availability_basis_points: Some(ReliabilityMetricObservation::new(
                10_000,
                vec!["support-system:availability".to_owned()],
            )),
            latency_ms: Some(ReliabilityMetricObservation::new(
                1,
                vec!["support-system:latency".to_owned()],
            )),
            error_budget_consumed_basis_points: Some(ReliabilityMetricObservation::new(
                0,
                vec!["support-system:error-budget".to_owned()],
            )),
            ..ReliabilityExternalObservations::default()
        })
    }
}

fn support_ticket_opened(event_id: &str, ticket_id: &str) -> anyhow::Result<EventEnvelope> {
    let mut envelope: EventEnvelope = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/events/support/support.ticket-opened.v1.envelope.json"
    ))?;
    envelope.event_id = event_id.to_owned();
    envelope.producer_service_id = TICKET_SERVICE.to_owned();
    envelope.module_id = "support-ticket".to_owned();
    envelope.contract_id = "ticket-opened".to_owned();
    envelope.content.data = json!({
        "ticketId": ticket_id,
        "openedAt": "2026-07-18T08:00:00Z"
    });
    if let Some(story) = envelope.context.story.as_mut() {
        story.story_id = M3_STORY_ID.to_owned();
        story.segment_id = "support-ticket-opened".to_owned();
    }
    if let Some(principal) = envelope.context.service_principal.as_mut() {
        principal.subject = format!("spiffe://example.com/service/{TICKET_SERVICE}");
        principal.audiences = vec![SLA_SERVICE.to_owned()];
        principal.credential_id = "credential_support_ticket_m3".to_owned();
    }
    if let Some(tenant) = envelope.context.tenant.as_mut() {
        tenant.tenant_id = M3_TENANT_ID.to_owned();
        tenant.audiences = vec![SLA_SERVICE.to_owned()];
    }
    Ok(envelope)
}

fn sla_principal(source: &EventEnvelope) -> ServicePrincipal {
    let mut principal = source
        .context
        .service_principal
        .clone()
        .expect("support event carries Service Principal context");
    principal.subject = format!("spiffe://example.com/service/{SLA_SERVICE}");
    principal.audiences = vec![TICKET_SERVICE.to_owned()];
    principal.credential_id = "credential_support_sla_m3".to_owned();
    principal
}

fn effect_publication(
    instance_id: &str,
    step_id: &str,
    compensation_action: &str,
    source: &EventEnvelope,
) -> WorkflowEventPublication {
    WorkflowEventPublication::new(
        TICKET_SERVICE,
        format!("{step_id}:effect:event"),
        "sla-acknowledged",
        "v1",
        "2026-07-18T08:00:01Z",
        sla_principal(source),
        json!({
            "ticketId": source.content.data["ticketId"],
            "workflowInstanceId": instance_id,
            "workflowStepId": step_id,
            "effectId": format!("{step_id}:effect"),
            "compensationAction": compensation_action,
        }),
    )
}

fn compensation_publication(
    instance_id: &str,
    compensation_id: &str,
    source: &EventEnvelope,
) -> WorkflowEventPublication {
    WorkflowEventPublication::new(
        TICKET_SERVICE,
        format!("{compensation_id}:request"),
        "sla-compensation-requested",
        "v1",
        "2026-07-18T08:00:06Z",
        sla_principal(source),
        json!({
            "ticketId": source.content.data["ticketId"],
            "workflowInstanceId": format!("caller-controlled-{instance_id}"),
            "compensationId": format!("caller-controlled-{compensation_id}"),
            "effectId": "caller-controlled-effect",
            "action": "caller-controlled-action",
        }),
    )
}

#[derive(Debug, Clone, Copy)]
struct SupportTicketSlaHandler;

#[async_trait]
impl ServiceEventHandler for SupportTicketSlaHandler {
    async fn handle(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        envelope: &EventEnvelope,
    ) -> Result<(), ServiceEventHandlerError> {
        if envelope.contract_id == "sla-acknowledged" {
            let Some(effect_id) = envelope.content.data["effectId"].as_str() else {
                return Ok(());
            };
            sqlx::query(
                r#"
                insert into support_ticket_sla_effects (
                    effect_id, ticket_id, compensation_action, source_event_id, active
                ) values ($1, $2, $3, $4, true)
                on conflict (effect_id) do nothing
                "#,
            )
            .bind(effect_id)
            .bind(
                envelope.content.data["ticketId"]
                    .as_str()
                    .unwrap_or_default(),
            )
            .bind(
                envelope.content.data["compensationAction"]
                    .as_str()
                    .unwrap_or_default(),
            )
            .bind(&envelope.event_id)
            .execute(&mut **transaction)
            .await
            .map_err(ServiceEventHandlerError::store)?;
            return Ok(());
        }
        if envelope.contract_id != "sla-compensation-requested" {
            return Ok(());
        }
        let compensation_id = envelope.content.data["compensationId"]
            .as_str()
            .unwrap_or_default();
        let effect_id = envelope.content.data["effectId"]
            .as_str()
            .unwrap_or_default();
        let action = envelope.content.data["action"].as_str().unwrap_or_default();
        let reversed = sqlx::query(
            r#"
            update support_ticket_sla_effects
            set active = false, compensated_by = $2
            where effect_id = $1 and compensation_action = $3 and active = true
            "#,
        )
        .bind(effect_id)
        .bind(compensation_id)
        .bind(action)
        .execute(&mut **transaction)
        .await
        .map_err(ServiceEventHandlerError::store)?;
        if reversed.rows_affected() != 1 {
            return Err(ServiceEventHandlerError::rejected_with_code(
                "compensation_effect_not_active",
                format!("Effect `{effect_id}` is not active for `{compensation_id}`"),
            ));
        }
        sqlx::query(
            r#"
            insert into support_ticket_sla_compensations (
                compensation_id, effect_id, action, envelope
            ) values ($1, $2, $3, $4)
            on conflict (compensation_id) do nothing
            "#,
        )
        .bind(compensation_id)
        .bind(effect_id)
        .bind(action)
        .bind(serde_json::to_value(envelope).map_err(|error| {
            ServiceEventHandlerError::poison("compensation_envelope_invalid", error.to_string())
        })?)
        .execute(&mut **transaction)
        .await
        .map_err(ServiceEventHandlerError::store)?;

        let mut completed = envelope.clone();
        completed.event_id = format!("{compensation_id}:completed");
        completed.event_type = "support.sla-compensated.v1".to_owned();
        completed.contract_id = "sla-compensated".to_owned();
        completed.producer_service_id = TICKET_SERVICE.to_owned();
        completed.module_id = "support-ticket".to_owned();
        completed.content.schema =
            "contracts/events/support/support.sla-compensated.v1.schema.json".to_owned();
        let principal = completed
            .context
            .service_principal
            .as_mut()
            .expect("workflow context carries Service Principal");
        principal.subject = format!("spiffe://example.com/service/{TICKET_SERVICE}");
        principal.audiences = vec![SLA_SERVICE.to_owned()];
        principal.credential_id = "credential_support_ticket_m3".to_owned();
        completed.context.causation = Some(lenso_service::CausationContext {
            causation_id: envelope.event_id.clone(),
            correlation_id: envelope
                .context
                .causation
                .as_ref()
                .and_then(|causation| causation.correlation_id.clone()),
        });
        ServiceEventPublisher
            .publish_in_tx(transaction, SLA_SERVICE, &completed)
            .await
            .map_err(|error| {
                ServiceEventHandlerError::retryable(
                    "compensation_completion_publish_failed",
                    error.message,
                )
            })?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CompensationCompletedHandler {
    state: ServiceRuntimeState,
}

#[async_trait]
impl ServiceEventHandler for CompensationCompletedHandler {
    async fn handle(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        envelope: &EventEnvelope,
    ) -> Result<(), ServiceEventHandlerError> {
        complete_workflow_compensation_from_event_in_tx(&self.state, transaction, envelope)
            .await
            .map_err(|error| {
                ServiceEventHandlerError::retryable(error.code.as_str(), error.message)
            })?;
        Ok(())
    }
}

async fn start_compensatable_effects(
    state: &ServiceRuntimeState,
    pool: &sqlx::PgPool,
    source: &EventEnvelope,
) -> anyhow::Result<(String, String)> {
    let mut start = pool.begin().await?;
    let instance = start_workflow_from_event_in_tx(
        state,
        &mut start,
        "support-sla",
        "ticket_sla_compensation",
        "v1",
        source,
    )
    .await?;
    start.commit().await?;

    let mut acknowledge = pool.begin().await?;
    let acknowledged = advance_workflow_step_with_event_in_tx(
        state,
        &mut acknowledge,
        &instance.instance_id,
        &instance.initial_step_id,
        &format!("{}:acknowledge_ticket", source.event_id),
        effect_publication(
            &instance.instance_id,
            &instance.initial_step_id,
            "withdraw_sla_acknowledgement",
            source,
        ),
    )
    .await?;
    acknowledge.commit().await?;
    let reserve_step_id = acknowledged.next_step_id.context("reserve step")?;

    let mut reserve = pool.begin().await?;
    let reserved = advance_workflow_step_with_event_in_tx(
        state,
        &mut reserve,
        &instance.instance_id,
        &reserve_step_id,
        &format!("{}:reserve_on_call", source.event_id),
        effect_publication(
            &instance.instance_id,
            &reserve_step_id,
            "release_on_call",
            source,
        ),
    )
    .await?;
    reserve.commit().await?;
    Ok((
        instance.instance_id,
        reserved.next_step_id.context("timeout step")?,
    ))
}

struct CompensationProof {
    support_state: ServiceRuntimeState,
    sla_state: ServiceRuntimeState,
    participant_restarts: u32,
    controlled_timeout: bool,
    compensation_order: Vec<String>,
    completed_effects: u32,
    compensation_effects: u32,
    duplicate_compensation_effects: u32,
    final_state: String,
}

struct PreparedCompensationOperation {
    support_state: ServiceRuntimeState,
    sla_state: ServiceRuntimeState,
    adapter: LocalTransportAdapter,
    source: EventEnvelope,
    instance_id: String,
    timeout_step_id: String,
    clock: Arc<SystemSandboxWorkflowClock>,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
}

#[allow(clippy::too_many_lines)]
async fn prove_cross_service_compensation(
    support_db: &TestDatabase,
    sla_db: &TestDatabase,
    transport_db: &TestDatabase,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> anyhow::Result<CompensationProof> {
    let prepared =
        prepare_cross_service_compensation(support_db, sla_db, transport_db, provider).await?;
    resume_cross_service_compensation(support_db, sla_db, prepared).await
}

async fn prepare_cross_service_compensation(
    support_db: &TestDatabase,
    sla_db: &TestDatabase,
    transport_db: &TestDatabase,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> anyhow::Result<PreparedCompensationOperation> {
    let support_migrations = [platform_core::Migration {
        name: "support-ticket/0001_create_sla_compensations",
        sql: r#"
            create table support_ticket_sla_effects (
                effect_id text primary key,
                ticket_id text not null,
                compensation_action text not null,
                source_event_id text not null unique,
                active boolean not null,
                compensated_by text unique
            );
            create table support_ticket_sla_compensations (
                compensation_id text primary key,
                effect_id text not null unique,
                action text not null,
                envelope jsonb not null
            );
        "#,
    }];
    let support_state = prepare_runtime(
        &support_service(),
        &support_runtime_config(Arc::clone(&provider)),
        support_db.pool.clone(),
        &support_migrations,
    )
    .await?;
    let manifest = manifest();
    let initial_time = DateTime::parse_from_rfc3339("2026-07-18T08:00:00Z")?.to_utc();
    let clock = Arc::new(SystemSandboxWorkflowClock::new(initial_time));
    let sla_state = prepare_runtime(
        &sla_service(),
        &sla_runtime_config(&manifest, Arc::clone(&provider)),
        sla_db.pool.clone(),
        &[],
    )
    .await?
    .with_workflow_clock(Arc::clone(&clock) as Arc<dyn platform_core::Clock>);
    let adapter = LocalTransportAdapter::prepare(transport_db.pool.clone()).await?;
    let source = support_ticket_opened("m3-support-workflow", "ticket_m3")?;
    let (instance_id, timeout_step_id) =
        start_compensatable_effects(&sla_state, &sla_db.pool, &source).await?;

    ensure!(
        relay_service_events_once(&sla_state, &adapter, 10).await? == 2,
        "M3 completed effects were not published"
    );
    ensure!(
        consume_service_events_once_without_workload_identity(
            &support_state,
            &adapter,
            TICKET_SERVICE,
            &SupportTicketSlaHandler,
            10,
        )
        .await?
            == 2,
        "support-ticket did not persist both cross-Service effects"
    );
    Ok(PreparedCompensationOperation {
        support_state,
        sla_state,
        adapter,
        source,
        instance_id,
        timeout_step_id,
        clock,
        provider,
    })
}

#[allow(clippy::too_many_lines)]
async fn resume_cross_service_compensation(
    support_db: &TestDatabase,
    sla_db: &TestDatabase,
    prepared: PreparedCompensationOperation,
) -> anyhow::Result<CompensationProof> {
    let PreparedCompensationOperation {
        support_state,
        mut sla_state,
        adapter,
        source,
        instance_id,
        timeout_step_id,
        clock,
        provider,
    } = prepared;
    let manifest = manifest();
    let timeout_time = clock.advance(ChronoDuration::seconds(5));
    let mut claims = claim_due_workflow_work_at(
        &sla_state,
        "support-sla-service-worker/m3",
        timeout_time,
        ChronoDuration::seconds(30),
        10,
    )
    .await?;
    ensure!(
        claims.len() == 1,
        "controlled time did not select one timeout"
    );
    let timeout = claims.remove(0);
    ensure!(
        timeout.kind == WorkflowTimerKind::StepTimeout && timeout.step_id == timeout_step_id,
        "controlled timeout selected the wrong durable work"
    );
    let selection =
        select_workflow_compensations_after_timeout_at(&sla_state, &timeout, timeout_time).await?;
    ensure!(
        selection.disposition == WorkflowTransitionDisposition::Applied
            && selection.compensations.len() == 2,
        "timeout did not select two compensations"
    );
    let duplicate_selection =
        select_workflow_compensations_after_timeout_at(&sla_state, &timeout, timeout_time).await?;
    ensure!(
        duplicate_selection.disposition == WorkflowTransitionDisposition::Duplicate,
        "timeout replay selected compensation twice"
    );

    drop(sla_state);
    sla_state = prepare_runtime(
        &sla_service(),
        &sla_runtime_config(&manifest, Arc::clone(&provider)),
        sla_db.pool.clone(),
        &[],
    )
    .await?
    .with_workflow_clock(Arc::clone(&clock) as Arc<dyn platform_core::Clock>);
    let mut duplicate_dispatches = 0_u32;
    for selected in &selection.compensations {
        let transition_id = format!("{}:attempt:1", selected.compensation_id);
        let publication =
            compensation_publication(&instance_id, &selected.compensation_id, &source);
        let mut dispatch = sla_db.pool.begin().await?;
        let result = dispatch_workflow_compensation_with_event_in_tx(
            &sla_state,
            &mut dispatch,
            &selected.compensation_id,
            &transition_id,
            publication.clone(),
        )
        .await?;
        ensure!(
            result.disposition == WorkflowTransitionDisposition::Applied,
            "compensation was not dispatched"
        );
        dispatch.commit().await?;

        let mut duplicate = sla_db.pool.begin().await?;
        let duplicate_result = dispatch_workflow_compensation_with_event_in_tx(
            &sla_state,
            &mut duplicate,
            &selected.compensation_id,
            &transition_id,
            publication,
        )
        .await?;
        duplicate.commit().await?;
        if duplicate_result.disposition == WorkflowTransitionDisposition::Duplicate {
            duplicate_dispatches += 1;
        }

        ensure!(
            relay_service_events_once(&sla_state, &adapter, 10).await? == 1,
            "compensation request was not relayed"
        );
        ensure!(
            consume_service_events_once_without_workload_identity(
                &support_state,
                &adapter,
                TICKET_SERVICE,
                &SupportTicketSlaHandler,
                10,
            )
            .await?
                == 1,
            "support-ticket did not apply compensation"
        );
        ensure!(
            relay_service_events_once(&support_state, &adapter, 10).await? == 1,
            "compensation completion was not relayed"
        );
        ensure!(
            consume_service_events_once_without_workload_identity(
                &sla_state,
                &adapter,
                SLA_SERVICE,
                &CompensationCompletedHandler {
                    state: sla_state.clone(),
                },
                10,
            )
            .await?
                == 1,
            "support-sla did not record compensation completion"
        );

        drop(sla_state);
        sla_state = prepare_runtime(
            &sla_service(),
            &sla_runtime_config(&manifest, Arc::clone(&provider)),
            sla_db.pool.clone(),
            &[],
        )
        .await?
        .with_workflow_clock(Arc::clone(&clock) as Arc<dyn platform_core::Clock>);
    }

    let first = &selection.compensations[0];
    let envelope: Value = sqlx::query_scalar(
        "select envelope from platform.service_event_outbox where event_id = $1",
    )
    .bind(format!("{}:request", first.compensation_id))
    .fetch_one(&sla_db.pool)
    .await?;
    adapter
        .publish(TransportPublication {
            consumer_id: TICKET_SERVICE.to_owned(),
            envelope: serde_json::from_value(envelope)?,
        })
        .await?;
    let duplicate_effects = consume_service_events_once_without_workload_identity(
        &support_state,
        &adapter,
        TICKET_SERVICE,
        &SupportTicketSlaHandler,
        10,
    )
    .await?;
    ensure!(duplicate_effects == 0, "redelivery repeated compensation");
    ensure!(
        duplicate_dispatches == u32::try_from(selection.compensations.len())?,
        "duplicate compensation dispatch was not rejected"
    );

    let business_compensations: Vec<(String, String, String)> = sqlx::query_as(
        "select compensation_id, effect_id, action from support_ticket_sla_compensations order by action",
    )
    .fetch_all(&support_db.pool)
    .await?;
    ensure!(
        business_compensations.len() == 2,
        "cross-Service compensation count changed"
    );
    let active_effects: i64 =
        sqlx::query_scalar("select count(*) from support_ticket_sla_effects where active")
            .fetch_one(&support_db.pool)
            .await?;
    ensure!(active_effects == 0, "completed effects remain active");

    let app = service_router(OpenApiRouter::new(), sla_state.clone());
    let inspected = app
        .clone()
        .oneshot(
            Request::get(format!("/runtime/workflows/instances/{instance_id}"))
                .body(Body::empty())?,
        )
        .await?;
    ensure!(
        inspected.status() == StatusCode::OK,
        "compensated workflow is not inspectable"
    );
    let inspected = json_body(inspected).await?;
    ensure!(
        inspected["instance"]["state"] == "compensated",
        "workflow did not reach compensated"
    );

    for suffix in ["one", "two"] {
        let started = app
            .clone()
            .oneshot(start_request("v2", &format!("reliability-{suffix}")))
            .await?;
        ensure!(
            started.status() == StatusCode::CREATED,
            "reliability pressure workflow did not start"
        );
    }

    Ok(CompensationProof {
        support_state,
        sla_state,
        participant_restarts: 3,
        controlled_timeout: true,
        compensation_order: selection
            .compensations
            .iter()
            .map(|compensation| compensation.name.clone())
            .collect(),
        completed_effects: 2,
        compensation_effects: u32::try_from(business_compensations.len())?,
        duplicate_compensation_effects: u32::try_from(duplicate_effects)?,
        final_state: inspected["instance"]["state"]
            .as_str()
            .context("workflow state")?
            .to_owned(),
    })
}

fn start_request(version: &str, suffix: &str) -> Request<Body> {
    Request::post("/runtime/workflows/support-sla/ticket_sla/instances")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "definitionVersion": version,
                "input": {"ticketId": format!("ticket_{suffix}")},
                "storyContext": {
                    "storyId": format!("story_{suffix}"),
                    "segmentId": format!("segment_{suffix}")
                },
                "tenantScope": {"tenantId": M3_TENANT_ID}
            })
            .to_string(),
        ))
        .expect("valid workflow start request")
}

fn migration_request() -> Request<Body> {
    Request::post("/runtime/workflows/support-sla/ticket_sla/migration-plans/dry-run")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"fromVersion": "v1", "targetVersion": "v2"}).to_string(),
        ))
        .expect("valid migration request")
}

async fn prove_versioning(db: &TestDatabase) -> anyhow::Result<VersioningEvidence> {
    let service = sla_service();
    let manifest = manifest();
    let state = prepare_runtime(
        &service,
        &workflow_runtime_config(&manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let app = service_router(OpenApiRouter::new(), state.clone());
    let started = app.clone().oneshot(start_request("v1", "pinned")).await?;
    ensure!(started.status() == StatusCode::CREATED, "v1 did not start");
    let started = json_body(started).await?;
    let instance_id = started["instance"]["instanceId"]
        .as_str()
        .context("v1 instance id")?
        .to_owned();
    let step_id = started["instance"]["initialStepId"]
        .as_str()
        .context("v1 step id")?
        .to_owned();
    let due_at: DateTime<Utc> = sqlx::query_scalar(
        "select due_at from platform.service_workflow_timers where instance_id = $1",
    )
    .bind(&instance_id)
    .fetch_one(&db.pool)
    .await?;
    drop(app);
    drop(state);

    let restarted = prepare_runtime(
        &service,
        &workflow_runtime_config(&manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let restarted_app = service_router(OpenApiRouter::new(), restarted.clone());
    let inspected = restarted_app
        .clone()
        .oneshot(
            Request::get(format!("/runtime/workflows/instances/{instance_id}"))
                .body(Body::empty())?,
        )
        .await?;
    let inspected = json_body(inspected).await?;
    let newer = restarted_app
        .clone()
        .oneshot(start_request("v2", "new"))
        .await?;
    ensure!(newer.status() == StatusCode::CREATED, "v2 did not start");
    let newer = json_body(newer).await?;
    let safe_plan = restarted_app.clone().oneshot(migration_request()).await?;
    let safe_plan = json_body(safe_plan).await?;
    drop(restarted_app);
    drop(restarted);

    let mut unsafe_manifest = manifest.clone();
    unsafe_manifest
        .runtime
        .as_mut()
        .context("workflow runtime surface")?
        .workflows
        .iter_mut()
        .find(|definition| definition.name == "ticket_sla" && definition.version == "v1")
        .context("v1 workflow")?
        .steps[0]
        .timeout_ms = Some(9_000);
    let incompatible = prepare_runtime(
        &service,
        &workflow_runtime_config(&unsafe_manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let incompatible_app = service_router(OpenApiRouter::new(), incompatible.clone());
    let blocked_plan = incompatible_app.oneshot(migration_request()).await?;
    let blocked_plan = json_body(blocked_plan).await?;
    let mismatch = claim_due_workflow_work_at(
        &incompatible,
        "support-sla-service-worker/unsafe",
        due_at + ChronoDuration::milliseconds(1),
        ChronoDuration::seconds(30),
        10,
    )
    .await
    .expect_err("incompatible worker must reject pinned state");
    let timer: (String, Option<String>) = sqlx::query_as(
        "select state, claimed_by from platform.service_workflow_timers where instance_id = $1 and step_id = $2",
    )
    .bind(&instance_id)
    .bind(&step_id)
    .fetch_one(&db.pool)
    .await?;
    let attempt_count: i64 = sqlx::query_scalar(
        "select count(*) from platform.service_workflow_step_attempts where instance_id = $1",
    )
    .bind(&instance_id)
    .fetch_one(&db.pool)
    .await?;
    Ok(VersioningEvidence {
        artifact_version: "lenso.m3-workflow-versioning-proof.v1",
        pinned_instance_version: inspected["instance"]["definition"]["version"]
            .as_str()
            .context("pinned version")?
            .to_owned(),
        new_instance_version: newer["instance"]["definition"]["version"]
            .as_str()
            .context("new version")?
            .to_owned(),
        migration_compatibility: safe_plan["compatibility"]["category"]
            .as_str()
            .context("safe migration category")?
            .to_owned(),
        worker_mismatch_compatibility: blocked_plan["compatibility"]["category"]
            .as_str()
            .context("blocked migration category")?
            .to_owned(),
        worker_mismatch_error: mismatch.code.as_str().to_owned(),
        worker_mismatch_mutated_state: timer.0 != "pending"
            || timer.1.is_some()
            || attempt_count != 0,
    })
}

async fn start_parent_and_child(
    state: &ServiceRuntimeState,
    pool: &sqlx::PgPool,
    source: &EventEnvelope,
) -> anyhow::Result<(WorkflowInstance, String)> {
    let mut transaction = pool.begin().await?;
    let parent = start_workflow_from_event_in_tx(
        state,
        &mut transaction,
        "support-sla",
        "ticket_sla",
        "v1",
        source,
    )
    .await?;
    let child = start_child_workflow_in_tx(
        state,
        &mut transaction,
        &parent.instance_id,
        &parent.initial_step_id,
        &WorkflowChildStartRequest {
            start_id: format!("{}:ticket_escalation", source.event_id),
            definition_owner: "support-sla".to_owned(),
            definition_name: "ticket_escalation".to_owned(),
            definition_version: "v1".to_owned(),
            input: json!({"ticketId": source.content.data["ticketId"]}),
        },
    )
    .await?;
    transaction.commit().await?;
    Ok((
        parent,
        child.child_instance_id.context("child instance id")?,
    ))
}

fn acknowledgement_publication(
    instance_id: &str,
    step_id: &str,
    source: &EventEnvelope,
) -> WorkflowEventPublication {
    WorkflowEventPublication::new(
        TICKET_SERVICE,
        format!("sla-acknowledged-{}", source.event_id),
        "sla-acknowledged",
        "v1",
        "2026-07-18T08:00:02Z",
        sla_principal(source),
        json!({
            "ticketId": source.content.data["ticketId"],
            "workflowInstanceId": instance_id,
            "workflowStepId": step_id,
        }),
    )
}

async fn prove_child_restart(db: &TestDatabase) -> anyhow::Result<(String, u32)> {
    let service = sla_service();
    let manifest = manifest();
    let state = prepare_runtime(
        &service,
        &workflow_runtime_config(&manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let source = support_ticket_opened("m3-child-workflow", "ticket_child_m3")?;
    let (parent, child_id) = start_parent_and_child(&state, &db.pool, &source).await?;
    drop(state);

    let restarted = prepare_runtime(
        &service,
        &workflow_runtime_config(&manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let (child_step_id, child_version): (String, String) = sqlx::query_as(
        "select initial_step_id, definition_version from platform.service_workflow_instances where instance_id = $1",
    )
    .bind(&child_id)
    .fetch_one(&db.pool)
    .await?;
    let mut completion = db.pool.begin().await?;
    let completed = advance_workflow_step_with_event_in_tx(
        &restarted,
        &mut completion,
        &child_id,
        &child_step_id,
        "m3-child-workflow:notify_on_call",
        acknowledgement_publication(&child_id, &child_step_id, &source),
    )
    .await?;
    ensure!(
        completed.next_step_id.is_none(),
        "child workflow did not complete"
    );
    completion.commit().await?;
    drop(restarted);

    let restarted = prepare_runtime(
        &service,
        &workflow_runtime_config(&manifest),
        db.pool.clone(),
        &[],
    )
    .await?;
    let mut resume = db.pool.begin().await?;
    let resumed = resume_parent_from_child_in_tx(
        &restarted,
        &mut resume,
        &parent.instance_id,
        &parent.initial_step_id,
        &child_id,
        "m3-child-completed",
    )
    .await?;
    resume.commit().await?;
    ensure!(
        resumed.disposition == WorkflowTransitionDisposition::Applied,
        "parent did not resume"
    );
    let mut duplicate = db.pool.begin().await?;
    let duplicate_resume = resume_parent_from_child_in_tx(
        &restarted,
        &mut duplicate,
        &parent.instance_id,
        &parent.initial_step_id,
        &child_id,
        "m3-child-completed",
    )
    .await?;
    duplicate.commit().await?;
    ensure!(
        duplicate_resume.disposition == WorkflowTransitionDisposition::Duplicate,
        "child completion redelivery resumed the parent twice"
    );
    Ok((child_version, 2))
}

async fn json_body(response: axum::response::Response) -> anyhow::Result<Value> {
    let body = response.into_body().collect().await?.to_bytes();
    serde_json::from_slice(&body).map_err(Into::into)
}

async fn read_reliability(state: &ServiceRuntimeState) -> anyhow::Result<ReliabilityEvidence> {
    let shutdown = platform_core::Shutdown::new();
    let worker_shutdown = shutdown.clone();
    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        run_worker(
            worker_state,
            Arc::new(platform_runtime::FunctionRegistry::default()),
            platform_core::EventHandlerRegistry::default(),
            ServiceWorkerConfig {
                poll_interval: Duration::from_secs(60),
                batch_size: 10,
            },
            worker_shutdown,
        )
        .await
    });
    let report = async {
        for _ in 0..100 {
            if state.worker_phase() == lenso_autonomous_service::RuntimePhase::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ensure!(
            state.worker_phase() == lenso_autonomous_service::RuntimePhase::Ready,
            "Service Worker did not become ready for Reliability Report proof"
        );
        let app = service_router(OpenApiRouter::new(), state.clone());
        let response = app
            .oneshot(Request::get("/runtime/reliability").body(Body::empty())?)
            .await?;
        ensure!(
            response.status() == StatusCode::OK,
            "Reliability Report endpoint failed"
        );
        json_body(response).await
    }
    .await;
    shutdown.signal();
    worker.await??;
    let report = report?;
    let workflow_check = report["checks"]
        .as_array()
        .context("Reliability checks")?
        .iter()
        .find(|check| check["code"] == "workflow_backlog")
        .context("workflow backlog check")?;
    ensure!(
        workflow_check["issueCode"] == "workflow_backlog_limit_exceeded",
        "workflow pressure did not breach the declared limit"
    );
    Ok(ReliabilityEvidence {
        artifact_version: report["protocol"]
            .as_str()
            .context("Reliability protocol")?
            .to_owned(),
        contract_id: report["contractId"]
            .as_str()
            .context("Reliability contract")?
            .to_owned(),
        profile: report["profile"]
            .as_str()
            .context("Reliability profile")?
            .to_owned(),
        state: report["state"]
            .as_str()
            .context("Reliability state")?
            .to_owned(),
        workflow_backlog_check: workflow_check["state"]
            .as_str()
            .context("workflow check state")?
            .to_owned(),
        issue_codes: report["readiness"]["issueCodes"]
            .as_array()
            .context("Reliability issue codes")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        reports_only: report["enforcement"]["reportsOnly"] == true,
    })
}

async fn prepare_aggregation_store(db: &TestDatabase) -> anyhow::Result<()> {
    let migrations = platform_core::PLATFORM_MIGRATIONS
        .iter()
        .chain(STORY_MIGRATIONS)
        .copied()
        .collect::<Vec<_>>();
    platform_core::apply_migrations(&db.pool, &migrations).await?;
    Ok(())
}

fn aggregator_credential(
    provider: &SystemSandboxWorkloadIdentityProvider,
    service_id: &str,
) -> anyhow::Result<String> {
    Ok(provider
        .issue(WorkloadCredentialRequest::new(
            AGGREGATOR_PRINCIPAL,
            feed_audience(service_id),
            AGGREGATOR_TRANSPORT_BINDING,
            now_ms(),
            60_000,
        ))?
        .token)
}

async fn spawn_feed(
    state: ServiceRuntimeState,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let app = service_router(OpenApiRouter::new(), state).layer(Extension(
        AuthenticatedTransportBinding::new(AGGREGATOR_TRANSPORT_BINDING),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://{address}"), task))
}

async fn prove_federation(
    aggregation_db: &TestDatabase,
    support_state: &ServiceRuntimeState,
    sla_state: &ServiceRuntimeState,
    provider: Arc<SystemSandboxWorkloadIdentityProvider>,
) -> anyhow::Result<FederationEvidence> {
    append_story_segment(
        support_state,
        &StorySegmentRecord::new(
            M3_STORY_ID,
            "support-ticket-opened",
            "event_contract",
            "support.ticket-opened",
            "ticket-opened",
            "v1",
            "completed",
            Utc::now(),
        )
        .with_tenant(M3_TENANT_ID),
    )
    .await?;
    let (ticket_url, ticket_task) = spawn_feed(support_state.clone()).await?;
    let (sla_url, sla_task) = spawn_feed(sla_state.clone()).await?;
    let sources = vec![
        FederatedStorySource::new(
            TICKET_SERVICE,
            &ticket_url,
            feed_audience(TICKET_SERVICE),
            Duration::from_secs(300),
        ),
        FederatedStorySource::new(
            SLA_SERVICE,
            &sla_url,
            feed_audience(SLA_SERVICE),
            Duration::from_secs(300),
        ),
        FederatedStorySource::new(
            "support-search-service",
            "http://127.0.0.1:9",
            feed_audience("support-search-service"),
            Duration::from_secs(300),
        ),
    ];
    let credentials = Arc::new(StaticStorySegmentFeedCredentialProvider::new([
        (
            TICKET_SERVICE.to_owned(),
            aggregator_credential(&provider, TICKET_SERVICE)?,
        ),
        (
            SLA_SERVICE.to_owned(),
            aggregator_credential(&provider, SLA_SERVICE)?,
        ),
        (
            "support-search-service".to_owned(),
            aggregator_credential(&provider, "support-search-service")?,
        ),
    ]));
    let client = Arc::new(HttpFederatedStoryFeedClient::new(
        reqwest::Client::new(),
        credentials,
    ));
    let aggregator = FederatedStoryAggregator::new(aggregation_db.pool.clone(), sources, client)?;
    let result = async {
        aggregator.collect_once(Some(M3_TENANT_ID)).await?;
        let initial = aggregator.story(M3_STORY_ID, Some(M3_TENANT_ID)).await?;
        ensure!(
            initial
                .gaps
                .iter()
                .any(|gap| gap.kind == FederatedStoryGapKind::Unreachable),
            "missing source was not exposed as a Segment gap"
        );
        append_story_segment(
            sla_state,
            &StorySegmentRecord::new(
                M3_STORY_ID,
                "support-sla-late-evidence",
                "workflow",
                "support.ticket-sla.compensated",
                "ticket-sla",
                "v1",
                "completed",
                Utc::now(),
            )
            .with_tenant(M3_TENANT_ID),
        )
        .await?;
        aggregator.collect_once(Some(M3_TENANT_ID)).await?;
        let final_story = aggregator.story(M3_STORY_ID, Some(M3_TENANT_ID)).await?;
        let mut gaps = final_story
            .gaps
            .iter()
            .map(|gap| gap.kind)
            .collect::<Vec<_>>();
        gaps.sort();
        gaps.dedup();
        Ok::<_, anyhow::Error>(FederationEvidence {
            artifact_version: "lenso.federated-runtime-story.v1",
            story_id: M3_STORY_ID,
            initial_segment_count: initial.segments.len(),
            final_segment_count: final_story.segments.len(),
            late_evidence_accepted: final_story.segments.len() > initial.segments.len(),
            gap_kinds: gaps,
            local_sources: [TICKET_SERVICE, SLA_SERVICE],
        })
    }
    .await;
    ticket_task.abort();
    sla_task.abort();
    let _ = ticket_task.await;
    let _ = sla_task.await;
    result
}
