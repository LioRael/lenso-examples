use anyhow::{Context, ensure};
use lenso_service::{
    ExtractionApproval, ExtractionAuthorityCommitInputs, ExtractionAuthorityCommitRevalidation,
    ExtractionBackfillBoundary, ExtractionBackfillRecord, ExtractionBackfillRequest,
    ExtractionBusinessInvariant, ExtractionCompatibilityEvidence, ExtractionDrainSnapshot,
    ExtractionPlan, ExtractionPolicyEvidence, ExtractionProvisionalCutoverInputs,
    ExtractionReconciliationInputs, ExtractionReconciliationStatus, ExtractionRun,
    ExtractionSourceSnapshot, ExtractionVerificationInputs, ExtractionVerificationStatus,
    apply_postgres_extraction_backfill_batch, commit_extraction_authority,
    complete_extraction_quiescence, fail_provisional_cutover, reconcile_extraction_data,
    record_autonomous_mutation, record_extraction_artifact, record_extraction_drain,
    request_fast_extraction_rollback, start_extraction_backfill, start_extraction_quiescence,
    start_provisional_cutover, verify_extraction_behavior, verify_provisional_cutover,
};
use platform_core::{PLATFORM_MIGRATIONS, apply_migrations};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;

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
}

pub async fn run_m4_smoke() -> anyhow::Result<M4SmokeEvidence> {
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    apply_migrations(&pool, PLATFORM_MIGRATIONS).await?;

    let blocked: Value = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.blocked.json"
    ))?;
    let corrected: Value = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.corrected.json"
    ))?;
    let plan: ExtractionPlan = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.plan.json"
    ))?;
    let expansion: ExtractionRun = serde_json::from_str(include_str!(
        "../../../../lenso/contracts/extraction/support-ticket.expansion-run.json"
    ))?;
    ensure!(blocked["ready"] == false && corrected["ready"] == true);
    let blocked_issue_codes = blocked["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|finding| finding["code"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    ensure!(!blocked_issue_codes.is_empty());

    let records = vec![
        record("ticket-001", "open"),
        record("ticket-002", "closed"),
        record("ticket-003", "waiting"),
    ];
    let run = start_extraction_backfill(
        &plan,
        &expansion,
        ExtractionBackfillBoundary::TrustworthyCursor {
            cursor: "support_tickets.id".to_owned(),
            source_high_water_mark: "ticket-003".to_owned(),
        },
    )?;
    let run = apply_postgres_extraction_backfill_batch(
        &pool,
        run,
        ExtractionBackfillRequest::new("batch-001", None, records[..2].to_vec()),
    )
    .await?;
    let restarted: lenso_service::ExtractionBackfillRun =
        serde_json::from_value(serde_json::to_value(&run)?)?;
    let checkpoint = restarted.progress.destination_checkpoint.clone();
    let backfill = apply_postgres_extraction_backfill_batch(
        &pool,
        restarted,
        ExtractionBackfillRequest::new("batch-002", checkpoint, records[2..].to_vec())
            .final_batch(),
    )
    .await?;
    ensure!(backfill.progress.copied_count == 3);

    let mut changed = records.clone();
    changed[1] = record("ticket-002", "open");
    let mismatch = reconcile_extraction_data(reconciliation_inputs(&backfill, changed));
    ensure!(mismatch.status == ExtractionReconciliationStatus::Blocked);
    let reconciliation = reconcile_extraction_data(reconciliation_inputs(&backfill, records));
    ensure!(reconciliation.status == ExtractionReconciliationStatus::Matched);

    let linked = observation("linked");
    let candidate = observation("autonomous");
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

    let quiescence = start_extraction_quiescence(&plan, &plan.expected_authority.revision)?;
    let quiescence = record_extraction_drain(quiescence, ExtractionDrainSnapshot::empty());
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
    let failed = fail_provisional_cutover(
        failed,
        "injected candidate 503",
        "operator:m4-acceptance",
        false,
    );
    ensure!(failed.external_mutations_paused && !failed.linked_mutations_open);

    let cutover = start_provisional_cutover(cutover_inputs(
        &plan,
        &verification,
        &quiescence,
        "routing-r8",
    ))?;
    let cutover = verify_provisional_cutover(cutover, "operator:m4-acceptance");
    let approval = ExtractionApproval::bind(&cutover, "approval:m4", "operator:m4", true);
    let mut stale = approval.clone();
    stale.plan_digest = "sha256:stale".to_owned();
    let stale_approval_rejected =
        commit_extraction_authority(commit_inputs(&cutover, stale)).is_err();
    ensure!(stale_approval_rejected);
    let committed = commit_extraction_authority(commit_inputs(&cutover, approval))?;
    let committed = record_autonomous_mutation(committed, "mutation:ticket-004");
    let post_commit_fast_rollback_blocked =
        request_fast_extraction_rollback(&committed, None).is_err();
    ensure!(post_commit_fast_rollback_blocked);

    let plan_id = &plan.plan_id;
    for artifact in [
        corrected,
        serde_json::to_value(&plan)?,
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
    })
}

fn record(id: &str, status: &str) -> ExtractionBackfillRecord {
    ExtractionBackfillRecord::new(
        id,
        json!({"id":id,"status":status,"tenantId":"tenant-acme","actorId":"user-42"}),
    )
}

fn reconciliation_inputs(
    backfill: &lenso_service::ExtractionBackfillRun,
    records: Vec<ExtractionBackfillRecord>,
) -> ExtractionReconciliationInputs {
    ExtractionReconciliationInputs {
        backfill: backfill.clone(),
        source: ExtractionSourceSnapshot {
            source_high_water_mark: "ticket-003".to_owned(),
            records,
            relationship_counts: vec![],
        },
        destination_relationship_counts: vec![],
        normalized_fields: vec![],
        business_invariants: vec![ExtractionBusinessInvariant::passed(
            "tenant-context-preserved",
            "tenant and actor identity remain present",
        )],
    }
}

fn observation(implementation: &str) -> lenso_service::ExtractionBehaviorObservation {
    lenso_service::ExtractionBehaviorObservation {
        implementation: implementation.to_owned(),
        module_id: "support-ticket".to_owned(),
        operation_id: "openTicket".to_owned(),
        tenant_id: "tenant-acme".to_owned(),
        actor_id: "user-42".to_owned(),
        response: json!({"ticketId":"ticket-003","status":"waiting"}),
        durable_state: json!({"ticket-003":{"status":"waiting"}}),
        event_effects: vec!["support.ticket-opened.v1:ticket-003".to_owned()],
        workflow_outcomes: vec!["support-triage:started".to_owned()],
        story_evidence: vec!["support-ticket-opened:ticket-003".to_owned()],
    }
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
) -> ExtractionAuthorityCommitInputs {
    ExtractionAuthorityCommitInputs {
        cutover: cutover.clone(),
        approval,
        current_authority_revision: cutover.authority_revision.clone(),
        current_routing_revision: cutover.routing_revision_current.clone(),
        current_system_graph_revision: "system-r12".to_owned(),
        revalidation: ExtractionAuthorityCommitRevalidation {
            plan_digest: cutover.plan_digest.clone(),
            authority_revision: cutover.authority_revision.clone(),
            destination_checkpoint: cutover.destination_checkpoint.clone(),
            verification_digest: cutover.verification_digest.clone(),
            quiescence_digest: cutover.quiescence_digest.clone(),
            candidate_healthy: true,
            source_quiesced: true,
            drain_complete: true,
            reconciliation_matched: true,
            compatibility_verified: true,
            policy_verified: true,
        },
    }
}
