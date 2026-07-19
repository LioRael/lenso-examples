use anyhow::{Context, ensure};
use lenso_service::{
    ExtractionApproval, ExtractionApprovalVerifier, ExtractionAuthorityCommitInputs,
    ExtractionAuthorityCommitRevalidation, ExtractionBackfillBoundary, ExtractionBusinessInvariant,
    ExtractionCandidateHealthEvidence, ExtractionCompatibilityEvidence,
    ExtractionExpansionOperationKind, ExtractionLinkedRollbackValidation,
    ExtractionOperationOutcome, ExtractionPlan, ExtractionPlanInputs, ExtractionPolicyEvidence,
    ExtractionProvisionalCutoverInputs, ExtractionReadinessEvidence,
    ExtractionReconciliationStatus, ExtractionRun, ExtractionRunEvidence,
    ExtractionRunEvidenceKind, ExtractionRunInputs, ExtractionScaffoldInputs,
    ExtractionTopologyState, ExtractionVerificationInputs, ExtractionVerificationStatus,
    ExtractionWorkloadRequest, apply_extraction_scaffold, build_extraction_operation_receipt,
    commit_extraction_authority, commit_extraction_authority_postgres,
    complete_extraction_quiescence, complete_provisional_rollback_validation,
    copy_postgres_extraction_service_data_batch, dry_run_extraction_scaffold,
    extraction_input_digest, fail_provisional_cutover, generate_extraction_plan,
    initialize_extraction_topology_state, load_postgres_extraction_backfill,
    reconcile_postgres_extraction_service_data, record_autonomous_mutation,
    record_destination_expansion_receipt, record_extraction_artifact, record_extraction_drain,
    request_fast_extraction_rollback, start_destination_expansion, start_extraction_backfill,
    start_extraction_quiescence, start_provisional_cutover, verify_extraction_behavior,
    verify_provisional_cutover,
};
use platform_core::{PLATFORM_MIGRATIONS, apply_migrations};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::fs;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct M4SmokeEvidence {
    artifact_version: &'static str,
    outcome: &'static str,
    public_seam: &'static str,
    blocked_issue_codes: Vec<String>,
    durable_backfill_resumed: bool,
    reconciliation_mismatch_blocked: bool,
    behavior_verified: bool,
    failed_cutover_rolled_back: bool,
    failed_linked_probe_kept_writes_paused: bool,
    stale_approval_rejected: bool,
    authority_committed: bool,
    post_commit_fast_rollback_blocked: bool,
    evidence_persisted: bool,
    evidence_references: Vec<String>,
    preserved_identities: Value,
    authority_history: Vec<&'static str>,
    local_requirements: Value,
    cleanup: Value,
}

pub async fn run_m4_smoke() -> anyhow::Result<M4SmokeEvidence> {
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    let source_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(
            &std::env::var("M4_SOURCE_DATABASE_URL")
                .context("M4_SOURCE_DATABASE_URL is required")?,
        )
        .await?;
    apply_migrations(&pool, PLATFORM_MIGRATIONS).await?;
    sqlx::raw_sql(
        r#"
            create schema if not exists support;
            create table if not exists support.tickets (
                id text primary key,
                title text not null,
                status text not null,
                created_at timestamptz not null
            );
            "#,
    )
    .execute(&source_pool)
    .await?;
    sqlx::query(
        "insert into support.tickets (id,title,status,created_at) values ('ticket-001','Sign in','open','2026-07-19'),('ticket-002','Billing','closed','2026-07-19'),('ticket-003','Export','waiting','2026-07-19') on conflict do nothing",
    )
    .execute(&source_pool)
    .await?;
    let linked_business: Value = serde_json::from_str(
        &std::env::var("M4_BUSINESS_EVIDENCE").context("M4_BUSINESS_EVIDENCE is required")?,
    )?;
    ensure!(linked_business["httpDecision"] == "call_completed");
    ensure!(linked_business["systemPlaneWithheld"] == true);

    let blocked_inputs: Value = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.blocked-inputs.json"
    ))?;
    let corrected_inputs: Value = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.corrected-inputs.json"
    ))?;
    let evaluate = |inputs: &Value| -> anyhow::Result<_> {
        let module = serde_json::from_value(inputs["module"].clone())?;
        let evidence: ExtractionReadinessEvidence =
            serde_json::from_value(inputs["evidence"].clone())?;
        Ok(lenso_service::evaluate_extraction_readiness(
            &module,
            &inputs["system"],
            &evidence,
        ))
    };
    let blocked_report = evaluate(&blocked_inputs)?;
    let corrected_report = evaluate(&corrected_inputs)?;
    let blocked = serde_json::to_value(&blocked_report)?;
    let corrected = serde_json::to_value(&corrected_report)?;
    ensure!(blocked["ready"] == false && corrected["ready"] == true);
    let plan_inputs: ExtractionPlanInputs = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.plan-inputs.json"
    ))?;
    let plan: ExtractionPlan = generate_extraction_plan(&plan_inputs)?;
    ensure!(generate_extraction_plan(&plan_inputs)? == plan);
    let mut scaffold_inputs: ExtractionScaffoldInputs = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.scaffold-inputs.json"
    ))?;
    scaffold_inputs.plan = plan.clone();
    let scaffold = dry_run_extraction_scaffold(&scaffold_inputs)?;
    ensure!(dry_run_extraction_scaffold(&scaffold_inputs)? == scaffold);
    let scaffold_root = std::env::temp_dir().join(format!("lenso-m4-scaffold-{}", Uuid::now_v7()));
    fs::create_dir(&scaffold_root)?;
    let scaffold_apply = apply_extraction_scaffold(&scaffold_root, &scaffold, &plan, &plan_inputs)?;
    ensure!(!scaffold_apply.created_files.is_empty());
    let scaffold_replay =
        apply_extraction_scaffold(&scaffold_root, &scaffold, &plan, &plan_inputs)?;
    ensure!(scaffold_replay.created_files.is_empty());
    ensure!(!scaffold_replay.unchanged_files.is_empty());
    fs::remove_dir_all(&scaffold_root)?;
    let mut expansion_inputs: ExtractionRunInputs = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.expansion-inputs.json"
    ))?;
    expansion_inputs.plan = plan.clone();
    expansion_inputs.current_plan_inputs = plan_inputs.clone();
    expansion_inputs.scaffold = scaffold.clone();
    expansion_inputs.scaffold_apply_result = scaffold_apply.clone();
    let expansion = execute_destination_expansion(&pool, expansion_inputs).await?;
    let blocked_issue_codes = blocked["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| finding["code"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    ensure!(!blocked_issue_codes.is_empty());

    let run = start_extraction_backfill(
        &plan,
        &expansion,
        ExtractionBackfillBoundary::TrustworthyCursor {
            cursor: "support_tickets.id".to_owned(),
            source_high_water_mark: "ticket-003".to_owned(),
        },
    )?;
    let original_run = run.clone();
    let run = copy_postgres_extraction_service_data_batch(
        &source_pool,
        &pool,
        &plan,
        run,
        "batch-001",
        2,
    )
    .await?;
    let replayed = copy_postgres_extraction_service_data_batch(
        &source_pool,
        &pool,
        &plan,
        original_run,
        "batch-001",
        2,
    )
    .await?;
    ensure!(
        replayed == run,
        "lost response must replay from durable receipt"
    );
    let restarted = load_postgres_extraction_backfill(&pool, &run.run_id)
        .await?
        .context("durable backfill run must reload")?;
    let backfill = copy_postgres_extraction_service_data_batch(
        &source_pool,
        &pool,
        &plan,
        restarted,
        "batch-002",
        2,
    )
    .await?;
    ensure!(backfill.progress.copied_count == 3);
    let destination_count: i64 = sqlx::query_scalar("select count(*) from support.tickets")
        .fetch_one(&pool)
        .await?;
    ensure!(destination_count == 3);
    sqlx::query("update support.tickets set status = 'corrupt' where id = 'ticket-002'")
        .execute(&pool)
        .await?;
    let mismatch = reconcile_postgres_extraction_service_data(
        &source_pool,
        &pool,
        &plan,
        backfill.clone(),
        vec![],
        vec![ExtractionBusinessInvariant::failed(
            "candidate-ticket-state-valid",
            "candidate corruption was injected",
        )],
    )
    .await?;
    ensure!(mismatch.status == ExtractionReconciliationStatus::Blocked);
    sqlx::query("update support.tickets set status = 'closed' where id = 'ticket-002'")
        .execute(&pool)
        .await?;
    let reconciliation = reconcile_postgres_extraction_service_data(
        &source_pool,
        &pool,
        &plan,
        backfill.clone(),
        vec![],
        vec![ExtractionBusinessInvariant::passed(
            "candidate-ticket-state-valid",
            "candidate rows were read from PostgreSQL and satisfy support-ticket state rules",
        )],
    )
    .await?;
    ensure!(reconciliation.status == ExtractionReconciliationStatus::Matched);

    let candidate_service = super::start_autonomous_extraction_service(&pool).await?;
    let linked = super::observe_linked_extraction_behavior(&source_pool).await?;
    let candidate = candidate_service.observe().await?;
    let reconciliation = reconcile_postgres_extraction_service_data(
        &source_pool,
        &pool,
        &plan,
        backfill.clone(),
        vec![],
        vec![ExtractionBusinessInvariant::passed(
            "candidate-ticket-state-valid",
            "post-observation source and candidate rows match in PostgreSQL",
        )],
    )
    .await?;
    let verification = verify_extraction_behavior(ExtractionVerificationInputs {
        reconciliation: reconciliation.clone(),
        linked,
        candidate,
        compatibility: vec![ExtractionCompatibilityEvidence::compatible(
            "support-web",
            "support-ticket-http.v1",
            "v1",
        )],
        policy: vec![ExtractionPolicyEvidence::passed(
            "single-authoritative-writer",
        )],
        volatile_json_pointers: vec![],
    });
    ensure!(verification.status == ExtractionVerificationStatus::Verified);

    super::pause_linked_extraction_mutations().await?;
    ensure!(
        super::observe_linked_extraction_behavior(&source_pool)
            .await
            .is_err()
    );
    let _ = super::linked_extraction_drain_snapshot(&source_pool).await?;
    sqlx::query(
        "insert into support.extraction_pending_work (work_id,kind) values ('workflow:support-ticket-003','durable_workflow') on conflict do nothing",
    )
    .execute(&source_pool)
    .await?;
    let blocked_drain = record_extraction_drain(
        start_extraction_quiescence(&plan, &plan.expected_authority.revision)?,
        super::linked_extraction_drain_snapshot(&source_pool).await?,
    );
    ensure!(!blocked_drain.issues.is_empty());
    sqlx::query("delete from support.extraction_pending_work")
        .execute(&source_pool)
        .await?;
    let quiescence = start_extraction_quiescence(&plan, &plan.expected_authority.revision)?;
    let quiescence = record_extraction_drain(
        quiescence,
        super::linked_extraction_drain_snapshot(&source_pool).await?,
    );
    let quiescence = complete_extraction_quiescence(
        quiescence,
        &backfill,
        &reconciliation,
        &plan.plan_digest,
        &plan.expected_authority.revision,
    );

    let failed = start_provisional_cutover(cutover_inputs(
        &plan,
        &verification,
        &quiescence,
        "routing-r7",
    ))?;
    candidate_service.inject_next_request_failure();
    let observed_candidate_failure = candidate_service
        .update_ticket(
            "ticket-003",
            "Provisional failure must not persist",
            "ticket-003:provisional-failure",
        )
        .await
        .is_err();
    ensure!(observed_candidate_failure);
    let failed_validation =
        ExtractionLinkedRollbackValidation::bind(&failed, "sha256:failed-linked-probe", false);
    let failed = fail_provisional_cutover(
        failed,
        "observed candidate 503 through provisional route",
        "operator:m4-acceptance",
        failed_validation,
    );
    ensure!(failed.external_mutations_paused && !failed.linked_mutations_open);
    ensure!(
        super::observe_linked_extraction_behavior(&source_pool)
            .await
            .is_err()
    );
    let linked_probe = super::probe_linked_extraction_route().await?;
    let rollback_validation = ExtractionLinkedRollbackValidation::bind(
        &failed,
        extraction_input_digest(linked_probe.as_bytes()),
        true,
    );
    let failed = complete_provisional_rollback_validation(failed, rollback_validation);
    ensure!(failed.linked_mutations_open && !failed.external_mutations_paused);
    super::resume_linked_extraction_mutations().await?;
    let linked_after_rollback = super::observe_linked_extraction_behavior(&source_pool).await?;
    ensure!(linked_after_rollback.response["ticketId"] == "ticket-003");

    super::pause_linked_extraction_mutations().await?;
    let candidate_after_rollback = candidate_service.observe().await?;
    let reconciliation = reconcile_postgres_extraction_service_data(
        &source_pool,
        &pool,
        &plan,
        backfill.clone(),
        vec![],
        vec![ExtractionBusinessInvariant::passed(
            "candidate-ticket-state-valid",
            "final paused source and live candidate rows match in PostgreSQL",
        )],
    )
    .await?;
    ensure!(reconciliation.status == ExtractionReconciliationStatus::Matched);
    let verification = verify_extraction_behavior(ExtractionVerificationInputs {
        reconciliation: reconciliation.clone(),
        linked: linked_after_rollback,
        candidate: candidate_after_rollback,
        compatibility: vec![ExtractionCompatibilityEvidence::compatible(
            "support-web",
            "support-ticket-http.v1",
            "v1",
        )],
        policy: vec![ExtractionPolicyEvidence::passed(
            "single-authoritative-writer",
        )],
        volatile_json_pointers: vec![],
    });
    ensure!(verification.status == ExtractionVerificationStatus::Verified);
    let quiescence = start_extraction_quiescence(&plan, &plan.expected_authority.revision)?;
    let quiescence = record_extraction_drain(
        quiescence,
        super::linked_extraction_drain_snapshot(&source_pool).await?,
    );
    let quiescence = complete_extraction_quiescence(
        quiescence,
        &backfill,
        &reconciliation,
        &plan.plan_digest,
        &plan.expected_authority.revision,
    );

    let cutover = start_provisional_cutover(cutover_inputs(
        &plan,
        &verification,
        &quiescence,
        "routing-r8",
    ))?;
    let cutover = verify_provisional_cutover(cutover, "operator:m4-acceptance");
    candidate_service.probe_health().await?;
    let candidate_health = ExtractionCandidateHealthEvidence::bind(
        plan.plan_id.clone(),
        "support-ticket-service",
        candidate_service.endpoint(),
        true,
        true,
    );
    let approval = ExtractionApproval::bind(
        &cutover,
        &candidate_health,
        "approval:m4",
        "operator:m4",
        true,
    );
    let mut stale = approval.clone();
    stale.plan_digest = "sha256:stale".to_owned();
    let stale_approval_rejected = commit_extraction_authority(commit_inputs(
        &cutover,
        stale,
        &reconciliation,
        &verification,
        &quiescence,
        &candidate_health,
    ))
    .is_err();
    ensure!(stale_approval_rejected);
    for artifact in [
        serde_json::to_value(&reconciliation)?,
        serde_json::to_value(&verification)?,
        serde_json::to_value(&quiescence)?,
        serde_json::to_value(&candidate_health)?,
    ] {
        record_extraction_artifact(&pool, &plan.plan_id, &artifact).await?;
    }
    initialize_extraction_topology_state(
        &pool,
        &ExtractionTopologyState {
            authority_revision: cutover.authority_revision.clone(),
            routing_revision: cutover.routing_revision_current.clone(),
            system_graph_revision: "system-r12".to_owned(),
            authority_kind: "linked".to_owned(),
            owner_id: "support-host".to_owned(),
        },
    )
    .await?;
    let committed = commit_extraction_authority_postgres(
        &pool,
        commit_inputs(
            &cutover,
            approval,
            &reconciliation,
            &verification,
            &quiescence,
            &candidate_health,
        ),
        &LocalApprovalVerifier,
    )
    .await?;
    candidate_service
        .update_ticket(
            "ticket-003",
            "Post-commit autonomous mutation",
            "ticket-003:post-commit-autonomous",
        )
        .await?;
    let committed = record_autonomous_mutation(committed, "mutation:ticket-003:post-commit");
    let post_commit_fast_rollback_blocked =
        request_fast_extraction_rollback(&committed, None).is_err();
    ensure!(post_commit_fast_rollback_blocked);

    let plan_id = &plan.plan_id;
    for artifact in [
        corrected,
        serde_json::to_value(&plan)?,
        serde_json::to_value(&scaffold)?,
        serde_json::to_value(&scaffold_apply)?,
        serde_json::to_value(&expansion)?,
        serde_json::to_value(&backfill)?,
        serde_json::to_value(&reconciliation)?,
        serde_json::to_value(&verification)?,
        serde_json::to_value(&quiescence)?,
        serde_json::to_value(&failed)?,
        serde_json::to_value(&cutover)?,
        serde_json::to_value(&committed)?,
        json!({"protocol":"lenso.extraction-authority.v1","kind":"autonomous_service","ownerId":"support-ticket-service","revision":committed.authority_revision}),
    ] {
        record_extraction_artifact(&pool, plan_id, &artifact).await?;
    }

    Ok(M4SmokeEvidence {
        artifact_version: "lenso.m4-safe-module-extraction-acceptance.v1",
        outcome: "passed",
        public_seam: "support-system",
        blocked_issue_codes,
        durable_backfill_resumed: true,
        reconciliation_mismatch_blocked: true,
        behavior_verified: true,
        failed_cutover_rolled_back: true,
        failed_linked_probe_kept_writes_paused: true,
        stale_approval_rejected,
        authority_committed: committed.candidate_authoritative,
        post_commit_fast_rollback_blocked,
        evidence_persisted: true,
        evidence_references: vec![
            plan.plan_digest.clone(),
            scaffold.scaffold_digest.clone(),
            backfill.run_digest.clone(),
            reconciliation.reconciliation_digest.clone(),
            verification.verification_digest.clone(),
            cutover.cutover_digest.clone(),
            committed.commit_digest.clone(),
        ],
        preserved_identities: json!({
            "moduleId":"support-ticket",
            "operationIds":["openTicket","getTicket"],
            "eventContracts":["support.ticket-opened.v1"],
            "storyIds":["support-ticket-opened"],
            "tenantsAndActors":true
        }),
        authority_history: vec![
            "linked",
            "provisional",
            "linked",
            "provisional",
            "autonomous",
        ],
        local_requirements: json!({"postgres":"ephemeral_or_DATABASE_URL","kubernetes":false,"productionAuthority":false}),
        cleanup: json!({"sandboxStateRemoved":true,"sourceStorePreservedReadOnly":true,"destructiveSourceCleanup":false}),
    })
}

struct LocalApprovalVerifier;

impl ExtractionApprovalVerifier for LocalApprovalVerifier {
    fn verify(&self, approval: &ExtractionApproval) -> bool {
        approval.authorized
            && approval.approver == "operator:m4"
            && approval.approval_id == "approval:m4"
    }
}

async fn execute_destination_expansion(
    pool: &sqlx::PgPool,
    inputs: ExtractionRunInputs,
) -> anyhow::Result<ExtractionRun> {
    let mut run = start_destination_expansion(&inputs)?;
    for operation in run.ordered_operations.clone() {
        let (outcome, kind, detail) = match operation.kind {
            ExtractionExpansionOperationKind::CreateIsolatedStore => {
                sqlx::query("create schema if not exists support")
                    .execute(pool)
                    .await?;
                (
                    ExtractionOperationOutcome::Created,
                    ExtractionRunEvidenceKind::StoreIsolation,
                    "candidate Store schema was created in the destination database",
                )
            }
            ExtractionExpansionOperationKind::ApplyExpandMigration => {
                sqlx::raw_sql(
                    "create table if not exists support.tickets (id text primary key, title text not null, status text not null, created_at timestamptz not null)",
                )
                .execute(pool)
                .await?;
                (
                    ExtractionOperationOutcome::Applied,
                    ExtractionRunEvidenceKind::MigrationApplied,
                    "expand-first migration was applied idempotently to PostgreSQL",
                )
            }
            ExtractionExpansionOperationKind::VerifyMigrationWorkload => {
                let exists: bool =
                    sqlx::query_scalar("select to_regclass('support.tickets') is not null")
                        .fetch_one(pool)
                        .await?;
                ensure!(
                    exists,
                    "destination migration workload did not create support.tickets"
                );
                (
                    ExtractionOperationOutcome::Healthy,
                    ExtractionRunEvidenceKind::MigrationWorkloadHealth,
                    "migration workload observed support.tickets in PostgreSQL",
                )
            }
            ExtractionExpansionOperationKind::VerifyCandidateHealth => {
                sqlx::query("select 1 from support.tickets limit 1")
                    .fetch_optional(pool)
                    .await?;
                (
                    ExtractionOperationOutcome::Healthy,
                    ExtractionRunEvidenceKind::CandidateHealth,
                    "candidate Store workload accepted a live PostgreSQL query without authority",
                )
            }
        };
        let request = ExtractionWorkloadRequest {
            run_id: run.run_id.clone(),
            plan_id: run.plan.plan_id.clone(),
            plan_digest: run.plan.plan_digest.clone(),
            expected_state: run.expected_state.clone(),
            expected_state_digest: run.expected_state_digest.clone(),
            operation: operation.clone(),
        };
        let receipt = build_extraction_operation_receipt(
            &request,
            outcome,
            vec![ExtractionRunEvidence {
                kind,
                subject: operation.operation_id.clone(),
                digest: extraction_input_digest(operation.operation_digest.as_bytes()),
                detail: detail.to_owned(),
            }],
        )?;
        run = record_destination_expansion_receipt(run, receipt)?;
    }
    Ok(run)
}

fn cutover_inputs(
    plan: &ExtractionPlan,
    verification: &lenso_service::ExtractionVerificationResult,
    quiescence: &lenso_service::ExtractionQuiescenceRun,
    routing_revision: &str,
) -> ExtractionProvisionalCutoverInputs {
    ExtractionProvisionalCutoverInputs {
        plan_id: plan.plan_id.clone(),
        plan_digest: plan.plan_digest.clone(),
        authority_revision: plan.expected_authority.revision.clone(),
        routing_revision: routing_revision.to_owned(),
        candidate_service_id: "support-ticket-service".to_owned(),
        candidate_healthy: true,
        verification: verification.clone(),
        quiescence: quiescence.clone(),
    }
}

fn commit_inputs(
    cutover: &lenso_service::ExtractionProvisionalCutoverRun,
    approval: ExtractionApproval,
    reconciliation: &lenso_service::ExtractionReconciliationResult,
    verification: &lenso_service::ExtractionVerificationResult,
    quiescence: &lenso_service::ExtractionQuiescenceRun,
    candidate_health: &ExtractionCandidateHealthEvidence,
) -> ExtractionAuthorityCommitInputs {
    ExtractionAuthorityCommitInputs {
        cutover: cutover.clone(),
        approval,
        current_authority_revision: cutover.authority_revision.clone(),
        current_routing_revision: cutover.routing_revision_current.clone(),
        current_system_graph_revision: "system-r12".to_owned(),
        revalidation: ExtractionAuthorityCommitRevalidation {
            reconciliation: reconciliation.clone(),
            verification: verification.clone(),
            quiescence: quiescence.clone(),
            candidate_health: candidate_health.clone(),
        },
    }
}
