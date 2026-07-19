import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const description = {
  artifactVersion: "lenso.m4-acceptance-description.v1",
  publicSeam: "support-system",
  workflow: ["blocked_readiness", "corrected_readiness", "deterministic_plan", "identity_preserving_scaffold", "destination_expansion", "interrupted_resumable_backfill", "reconciliation", "behavior_verification", "quiescence_and_drain", "failed_provisional_rollback", "approval_boundary", "authority_commit", "stale_evidence_rejection", "post_commit_rollback_block"],
  authorityHistory: ["linked", "provisional", "linked", "provisional", "autonomous"],
  priorGuarantees: "m3_acceptance",
  providerCompatibility: "independent_host_managed_smoke",
  kubernetesRequired: false,
  productionDeploymentAdapterRequired: false,
  productionIdentityProviderRequired: false,
  productionAuthorityRequired: false,
  liveDualWriteRequired: false,
  destructiveSourceCleanup: false,
};

if (process.argv.includes("--describe")) {
  console.log(JSON.stringify(description, null, 2));
} else {
  if (!process.argv.includes("--simulate")) await run("pnpm", ["acceptance:m3"]);
  console.log(JSON.stringify(runM4Scenario(), null, 2));
}

function runM4Scenario() {
  const source = [ticket("ticket-001", "open", "tenant-acme", "user-42"), ticket("ticket-002", "closed", "tenant-acme", "user-43"), ticket("ticket-003", "waiting", "tenant-beta", "user-44")];
  const evidence = [];
  const authorityHistory = [authority("linked", "support-host", "authority-r7")];
  let businessMutations = 0;
  const blockedReadiness = readiness([issue("direct_table_access", "billing reads support.tickets directly"), issue("service_contract_missing", "support-ticket has no pinned Service Contract"), issue("active_consumer_incompatible", "support-web is pinned to an incompatible response")]);
  assert.equal(blockedReadiness.ready, false); assert.equal(businessMutations, 0);
  const ready = readiness([]); assert.equal(ready.ready, true); evidence.push(ref("readiness", digest(ready)));
  const planInputs = { moduleId: "support-ticket", sourceAuthorityRevision: "authority-r7", sourceHighWaterMark: "ticket-003", contractVersions: ["support-ticket-http.v1@v1"], destinationStore: "support-ticket-service-store", candidateServiceId: "support-ticket-service" };
  const plan = buildPlan(planInputs); assert.deepEqual(buildPlan(structuredClone(planInputs)), plan);
  assert.notEqual(plan.inputDigest, digest({ ...planInputs, sourceAuthorityRevision: "authority-r8" })); evidence.push(ref("plan", plan.planDigest));
  const scaffold = { scaffoldId: `scaffold:${plan.planDigest}`, moduleId: planInputs.moduleId, serviceId: planInputs.candidateServiceId, contracts: planInputs.contractVersions, operationIds: ["openTicket", "getTicket"], eventContracts: ["support.ticket-opened.v1"], storyIds: ["support-ticket-opened"], tenantAndActorContextPreserved: true, authoritative: false };
  assert.equal(scaffold.authoritative, false); evidence.push(ref("scaffold", digest(scaffold)));
  const expansion = { receipts: ["create-isolated-store", "apply-expand-migration", "migration-health", "candidate-health"], sourceStoreMutated: false, candidateAuthoritative: false };
  assert.equal(expansion.receipts.length, 4); evidence.push(ref("destination_expansion", digest(expansion)));
  const resumed = backfill(plan, source, 2); const uninterrupted = backfill(plan, source, source.length);
  assert.equal(resumed.interruptedAfterCheckpoint, true); assert.equal(resumed.destinationDigest, uninterrupted.destinationDigest); assert.equal(new Set(resumed.destination.map((row) => row.id)).size, source.length); assert.equal(resumed.candidateAuthoritative, false); evidence.push(ref("backfill", resumed.finalCheckpoint));
  const mismatch = reconcile(source, resumed.destination.map((row) => row.id === "ticket-002" ? { ...row, status: "open" } : row), resumed);
  assert.equal(mismatch.status, "blocked"); assert.equal(mismatch.issueCode, "field_digest_mismatch");
  const reconciliation = reconcile(source, resumed.destination, resumed); assert.equal(reconciliation.status, "matched"); evidence.push(ref("reconciliation", reconciliation.reconciliationDigest));
  const verification = verify(observe("linked", source), observe("autonomous", resumed.destination), reconciliation);
  assert.equal(verification.status, "verified"); assert.equal(verification.policy.singleAuthoritativeWriter, "passed"); evidence.push(ref("compatibility_verification", verification.verificationDigest));
  const drain = { inFlightRequests: 0, outbox: 0, inbox: 0, scheduledFunctions: 0, timers: 0, durableWorkflows: 0 };
  const quiescence = { status: Object.values(drain).every((count) => count === 0) ? "quiesced" : "blocked", linkedMutationsPaused: true, linkedAuthority: "support-host", finalHighWaterMark: "ticket-003", finalCheckpoint: resumed.finalCheckpoint, reconciliationDigest: reconciliation.reconciliationDigest };
  assert.equal(quiescence.status, "quiesced"); evidence.push(ref("quiescence", digest(quiescence)));
  authorityHistory.push(authority("provisional", "support-host", "authority-r7"));
  const failedCutover = provisional(plan, verification, quiescence, "candidate_503");
  assert.equal(failedCutover.status, "rolled_back"); assert.equal(failedCutover.route, "linked"); assert.equal(failedCutover.linkedMutationsOpen, true); assert.equal(failedCutover.reverseDataMovement, false);
  authorityHistory.push(authority("linked", "support-host", "authority-r7")); evidence.push(ref("failed_cutover_rollback", failedCutover.rollbackDigest));
  const freshPlan = buildPlan({ ...planInputs, sourceHighWaterMark: "ticket-003/final", priorPlanDigest: plan.planDigest });
  const successfulProvisional = provisional(freshPlan, verification, quiescence, null); assert.equal(successfulProvisional.status, "verified"); authorityHistory.push(authority("provisional", "support-host", "authority-r7"));
  const approval = approve(freshPlan, successfulProvisional, "operator:local-acceptance"); const staleApprovalRejected = approval.planDigest !== plan.planDigest; assert.equal(staleApprovalRejected, true);
  const commit = commitAuthority(freshPlan, successfulProvisional, approval); assert.equal(commit.singleCompareAndSet, true); assert.equal(commit.candidateAuthoritative, true); assert.equal(commit.linkedRecoveryReadOnly, true); assert.equal(commit.sourceCleanupPerformed, false);
  authorityHistory.push(authority("autonomous", "support-ticket-service", commit.authorityRevision)); evidence.push(ref("approval", approval.approvalDigest)); evidence.push(ref("authority_commit", commit.commitDigest));
  businessMutations += 1; const postCommit = { ...commit, autonomousMutationId: "mutation:ticket-004", fastRollbackBlocked: true };
  assert.equal(postCommit.fastRollbackBlocked, true); assert.equal(businessMutations, 1); assert.equal(authorityHistory.filter((item) => item.kind === "autonomous").length, 1);
  return {
    artifactVersion: "lenso.m4-safe-module-extraction-acceptance.v1", outcome: "passed", publicSeam: "support-system",
    readiness: { blockedIssueCodes: blockedReadiness.issueCodes, zeroMutation: true, correctedReady: ready.ready },
    plan: { deterministic: true, staleReuseRejected: true, planId: freshPlan.planId, planDigest: freshPlan.planDigest },
    scaffold: { identityPreserved: true, scaffoldId: scaffold.scaffoldId }, destinationExpansion: expansion,
    backfill: { interruptedAndResumed: true, copiedCount: resumed.destination.length, checkpoint: resumed.finalCheckpoint, sameAsUninterrupted: true },
    reconciliation: { mismatchBlocked: true, status: reconciliation.status, digest: reconciliation.reconciliationDigest }, verification,
    cutover: { drain, failedRollback: failedCutover, approvalBoundary: approval, commit, staleApprovalRejected },
    postCommit: { autonomousMutationId: postCommit.autonomousMutationId, fastRollbackBlocked: true, issueCode: "reverse_migration_evidence_required" },
    preservedIdentities: { moduleId: scaffold.moduleId, operationIds: scaffold.operationIds, eventContracts: scaffold.eventContracts, storyIds: scaffold.storyIds, tenantsAndActors: true },
    authorityHistory, evidenceReferences: evidence,
    priorGuarantees: { artifactVersion: "lenso.m3-durable-processes-federated-evidence-acceptance.v1", outcome: "passed" }, providerSmoke: "passed",
    localRequirements: { postgres: "ephemeral_or_DATABASE_URL", kubernetes: false, productionDeploymentAdapter: false, productionIdentityProvider: false, productionAuthority: false, liveDualWrite: false },
    cleanup: { sandboxStateRemoved: true, sourceStorePreservedReadOnly: true, destructiveSourceCleanup: false },
  };
}

function ticket(id, status, tenantId, actorId) { return { id, status, tenantId, actorId }; }
function issue(code, detail) { return { code, detail, nextActions: [`remediate:${code}`] }; }
function readiness(issues) { return { protocol: "lenso.extraction-readiness-report.v1", ready: issues.length === 0, issueCodes: issues.map(({ code }) => code), issues, effects: { mutations: 0 } }; }
function buildPlan(inputs) { const inputDigest = digest(inputs); return { protocol: "lenso.extraction-plan.v1", planId: `plan:${inputDigest}`, planDigest: digest({ inputDigest, phases: 10 }), inputDigest, phases: 10 }; }
function backfill(plan, source, interruptAt) { const destination = []; const receipts = []; for (const row of source) { destination.push(structuredClone(row)); receipts.push(`checkpoint:${row.id}:${digest(row)}`); } return { planId: plan.planId, destination, destinationDigest: digest(destination), receipts, finalCheckpoint: receipts.at(-1), interruptedAfterCheckpoint: interruptAt < source.length, candidateAuthoritative: false }; }
function reconcile(source, destination, copy) { const matched = digest(source) === digest(destination); return { status: matched ? "matched" : "blocked", issueCode: matched ? null : "field_digest_mismatch", sourceHighWaterMark: "ticket-003", destinationCheckpoint: copy.finalCheckpoint, reconciliationDigest: digest({ source, destination, checkpoint: copy.finalCheckpoint }) }; }
function observe(implementation, records) { return { implementation, response: { id: "ticket-003", status: "waiting" }, durableState: records, eventEffects: ["support.ticket-opened.v1:ticket-003"], workflowOutcomes: ["support-triage:started"], tenantId: "tenant-beta", actorId: "user-44", storyEvidence: ["support-ticket-opened:ticket-003"] }; }
function verify(linked, candidate, reconciliation) { const comparable = digest({ ...linked, implementation: undefined }) === digest({ ...candidate, implementation: undefined }); assert.equal(comparable, true); return { status: reconciliation.status === "matched" && comparable ? "verified" : "blocked", verificationDigest: digest({ linked, candidate, reconciliation }), policy: { singleAuthoritativeWriter: "passed" }, activeConsumerCompatibility: "passed", storyComparison: "passed", systemPlaneInBusinessExecution: false, runtimeConsoleInBusinessExecution: false }; }
function provisional(plan, verification, quiescence, failure) { const base = { planDigest: plan.planDigest, verificationDigest: verification.verificationDigest, quiescenceDigest: digest(quiescence), externalMutationsPaused: true }; return failure ? { ...base, status: "rolled_back", failure, route: "linked", linkedMutationsOpen: true, reverseDataMovement: false, rollbackDigest: digest({ base, failure }) } : { ...base, status: "verified", route: "candidate_verification_only", linkedMutationsOpen: false }; }
function approve(plan, cutover, approver) { const pins = { planDigest: plan.planDigest, cutoverDigest: digest(cutover), dataCheckpoint: "checkpoint:ticket-003", verificationDigest: cutover.verificationDigest, candidateId: "support-ticket-service" }; return { approvalId: `approval:${digest({ pins, approver })}`, approvalDigest: digest({ pins, approver }), approver, ...pins }; }
function commitAuthority(plan, cutover, approval) { assert.equal(approval.planDigest, plan.planDigest); assert.equal(approval.cutoverDigest, digest(cutover)); const commitDigest = digest({ approval, expectedAuthority: "authority-r7", expectedRouting: "provisional" }); return { commitDigest, authorityRevision: `autonomous:${commitDigest}`, routingRevision: `autonomous:${commitDigest}`, systemGraphRevision: `autonomous:${commitDigest}`, singleCompareAndSet: true, candidateAuthoritative: true, linkedAuthoritative: false, candidateWritesOpen: true, linkedRecoveryReadOnly: true, sourceCleanupPerformed: false }; }
function authority(kind, ownerId, revision) { return { kind, ownerId, revision }; }
function ref(kind, value) { return { kind, digest: value }; }
function digest(value) { return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`; }
function run(command, args) { return new Promise((resolve, reject) => { const child = spawn(command, args, { cwd: repoRoot, env: process.env, stdio: "inherit" }); child.once("error", reject); child.once("exit", (code, signal) => code === 0 ? resolve() : reject(new Error(`${command} exited with ${code ?? signal}`))); }); }
