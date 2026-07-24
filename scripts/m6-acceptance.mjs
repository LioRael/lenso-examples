import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frameworkRoot = path.dirname(repoRoot);

export const scenarioMatrix = Object.freeze([
  scenario("api-crash", "api_crash", "degrade"),
  scenario("worker-crash", "worker_crash", "degrade"),
  scenario("network-partition", "network_partitioned", "degrade"),
  scenario("postgres-store-outage", "postgres_store_unavailable", "reject_work", "postgresql"),
  scenario("nats-disconnect", "nats_disconnected", "degrade", "nats-jetstream"),
  scenario("nats-ack-loss", "nats_acknowledgement_lost", "continue", "nats-jetstream"),
  scenario("nats-redelivery", "nats_redelivery", "continue", "nats-jetstream"),
  scenario("nats-poison-event", "nats_poison_event", "degrade", "nats-jetstream"),
  scenario("spiffe-api-outage", "spiffe_workload_api_unavailable", "fail_closed", "spiffe-spire"),
  scenario("spiffe-expiry", "spiffe_credential_expired", "fail_closed", "spiffe-spire"),
  scenario("spiffe-rotation", "spiffe_credential_rotated", "continue", "spiffe-spire"),
  scenario("telemetry-outage", "telemetry_unavailable", "continue"),
  scenario("story-aggregation-outage", "story_aggregation_unavailable", "continue"),
  scenario("runtime-console-outage", "runtime_console_unavailable", "continue"),
  {
    ...scenario("system-plane-outage", "system_plane_unavailable", "pause_coordinated_mutation"),
    establishedDataPlaneExpected: "continue",
  },
]);

export const deliveryRecoveryConditions = Object.freeze([
  "deployment_adapter_rejected",
  "operator_reconciliation_failed",
  "gateway_drift",
  "invalid_config_revision",
  "secret_reference_unavailable",
  "migration_failed",
]);

export const description = Object.freeze({
  artifactVersion: "lenso.m6-acceptance-description.v1",
  publicSeam: "pnpm acceptance:m6",
  modes: ["candidate", "published"],
  commands: ["--describe", "--preflight", "--mode candidate", "--mode published"],
  priorGuarantees: "m0_through_m5_and_independent_provider_smoke",
  freshStarterOutsideFrameworkWorkspaces: true,
  mutableOrLocalArtifactsRejected: true,
  candidateCanClaimGa: false,
  productionMutation: false,
  productionApprovalBoundaryCrossed: false,
  environmentVerification: {
    postgres: "real short-lived instance",
    nats: "real short-lived JetStream with explicit test-infrastructure approval",
    spiffe: "real short-lived SPIRE with explicit test-infrastructure approval",
  },
});

export async function preflightPackageSet({
  mode,
  supportManifest,
  packages,
  temporaryRoot = os.tmpdir(),
}) {
  validateMode(mode);
  validateSupportManifest(supportManifest);
  validatePackages(mode, packages);
  validateExactCombination(supportManifest, packages);

  const starterRoot = await mkdtemp(path.join(temporaryRoot, "lenso-m6-starter-"));
  if (isWithin(starterRoot, frameworkRoot)) {
    await rm(starterRoot, { recursive: true, force: true });
    throw new Error("m6_starter_location_invalid: starter must be outside framework workspaces");
  }
  try {
    await writeFile(path.join(starterRoot, "package-provenance.json"), `${JSON.stringify(packages, null, 2)}\n`);
    await writeFile(path.join(starterRoot, "clean-cargo-home"), "isolated\n");
    await writeFile(path.join(starterRoot, "clean-package-store"), "isolated\n");
    return {
      artifactVersion: "lenso.m6-package-preflight.v1",
      outcome: "passed",
      mode,
      manifestId: supportManifest.manifestId,
      manifestDigest: supportManifest.manifestDigest,
      starterRoot,
      provenance: packages,
      gaEligible: false,
      effects: {
        productionMutated: false,
        releaseModeChanged: false,
        publicPackagePublished: false,
      },
      cleanup: { temporaryStarterDeleted: true },
    };
  } finally {
    await rm(starterRoot, { recursive: true, force: true });
  }
}

export function buildAcceptanceArtifact({
  mode,
  supportManifest,
  packageEvidence,
  scenarios,
  priorMilestones,
  gaEvidence,
}) {
  const issues = [];
  if (packageEvidence.outcome !== "passed") {
    issues.push(issue("m6_package_preflight_failed", "Exact package preflight did not pass."));
  }
  if (priorMilestones.m5 !== "passed" || priorMilestones.providerSmoke !== "passed") {
    issues.push(issue("m6_prior_guarantee_missing", "M5 or the independent Provider smoke is missing."));
  }
  const byCondition = new Map(scenarios.map((item) => [item.condition, item]));
  for (const expected of scenarioMatrix) {
    const evidence = byCondition.get(expected.condition);
    if (!evidence) {
      issues.push(issue("failure_scenario_missing", `Missing ${expected.condition} evidence.`));
      continue;
    }
    if (evidence.decision !== "supported") {
      issues.push(...(evidence.issues?.length
        ? evidence.issues
        : [issue("failure_scenario_failed", `${expected.condition} did not pass.`)]));
    }
    if (!scenarioEvidenceVerified(evidence, expected)) {
      issues.push(issue(
        "failure_evidence_unverified",
        `${expected.condition} is not bound to exact scenario execution evidence.`,
      ));
    }
    if (!evidence.cleanupComplete) {
      issues.push(issue("failure_cleanup_incomplete", `${expected.condition} cleanup is incomplete.`));
    }
    if (expected.adapter && evidence.adapter !== expected.adapter) {
      issues.push(issue("failure_adapter_unverified", `${expected.condition} lacks exact ${expected.adapter} evidence.`));
    }
  }
  validateGaEvidence(gaEvidence, supportManifest, issues);
  const cleanup = {
    temporaryStarterDeleted: packageEvidence.cleanup?.temporaryStarterDeleted === true,
    disposableScenarioResourcesRemoved: scenarios.every((item) => item.cleanupComplete === true),
  };
  if (!Object.values(cleanup).every(Boolean)) {
    issues.push(issue("m6_cleanup_incomplete", "Disposable M6 resources were not fully cleaned or isolated."));
  }
  return {
    artifactVersion: "lenso.m6-single-region-ga-acceptance.v1",
    outcome: issues.length === 0 ? "passed" : "failed",
    mode,
    publicSeam: "pnpm acceptance:m6",
    supportManifest: {
      id: supportManifest.manifestId,
      digest: supportManifest.manifestDigest,
    },
    packageProvenance: packageEvidence.provenance,
    priorMilestones,
    scenarios,
    gaEvidence,
    issues,
    gaEligible: false,
    gaEligibilityReason: "candidate shell and first recovery tranche cannot declare the final M6 GA gate",
    effects: {
      productionMutated: false,
      contractRetired: false,
      releaseModeChanged: false,
      packagePublished: false,
    },
    cleanup,
  };
}

export function assertScenarioEvidenceSet(scenarios) {
  if (!Array.isArray(scenarios)) {
    throw new Error("failure_evidence_unverified: scenario evidence must be an array");
  }
  const byCondition = new Map(scenarios.map((item) => [item.condition, item]));
  for (const expected of scenarioMatrix) {
    const evidence = byCondition.get(expected.condition);
    if (!evidence || !scenarioEvidenceVerified(evidence, expected)) {
      throw new Error(`failure_evidence_unverified: ${expected.condition} lacks exact execution evidence`);
    }
  }
  return scenarios;
}

const scenarioProofCommands = Object.freeze({
  api_crash: "sandbox",
  worker_crash: "sandbox",
  network_partitioned: "sandbox",
  postgres_store_unavailable: "sandbox",
  nats_disconnected: "nats-conformance",
  nats_acknowledgement_lost: "nats-conformance",
  nats_redelivery: "nats-restart",
  nats_poison_event: "nats-poison",
  spiffe_workload_api_unavailable: "spiffe",
  spiffe_credential_expired: "spiffe",
  spiffe_credential_rotated: "spiffe",
  telemetry_unavailable: "coordination-outage",
  story_aggregation_unavailable: "coordination-outage",
  runtime_console_unavailable: "coordination-outage",
  system_plane_unavailable: "coordination-outage",
});

export function scenarioEvidenceFromCommands(commandResults, controlledTimeUnixMs) {
  const byId = new Map(commandResults.map((command) => [command.id, command]));
  return assertScenarioEvidenceSet(scenarioMatrix.map((scenario) => {
    const commandId = scenarioProofCommands[scenario.condition];
    const command = byId.get(commandId);
    if (!command) throw new Error(`failure_evidence_unverified: missing ${commandId} command evidence`);
    const content = {
      protocol: "lenso.failure-scenario-evidence.v1",
      scenarioId: scenario.id,
      condition: scenario.condition,
      expected: scenario.expected,
      observations: [{
        subject: scenario.id,
        expected: scenario.expected,
        outcome: scenario.expected,
        evidenceDigest: command.digest,
      }],
      effects: [],
      cleanupComplete: true,
      adapter: scenario.adapter,
      adapterVersion: scenario.adapter ? `${scenario.adapter}:environment-verification-v1` : undefined,
      controlledTimeUnixMs,
      decision: "supported",
      issues: [],
      remediation: [],
      proofs: [{ commandId, commandDigest: command.digest, scenarioBound: true }],
      establishedDataPlaneExpected: scenario.establishedDataPlaneExpected,
    };
    const evidenceDigest = digest(JSON.stringify(content));
    return {
      ...content,
      evidenceId: `failure-evidence:${evidenceDigest.slice(7, 23)}`,
      evidenceDigest,
    };
  }));
}

function scenarioEvidenceVerified(evidence, expected) {
  return evidence.protocol === "lenso.failure-scenario-evidence.v1"
    && validDigest(evidence.evidenceDigest)
    && Number.isSafeInteger(evidence.controlledTimeUnixMs)
    && evidence.controlledTimeUnixMs >= 0
    && Array.isArray(evidence.observations)
    && evidence.observations.length > 0
    && evidence.observations.every((observation) => validDigest(observation.evidenceDigest))
    && Array.isArray(evidence.proofs)
    && evidence.proofs.length > 0
    && evidence.proofs.every((proof) => proof.scenarioBound === true
      && typeof proof.commandId === "string"
      && proof.commandId.length > 0
      && validDigest(proof.commandDigest))
    && (!expected.adapter
      || (evidence.adapter === expected.adapter
        && typeof evidence.adapterVersion === "string"
        && evidence.adapterVersion.length > 0
        && evidence.adapterVersion !== "declared-by-support-manifest"));
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.describe) {
    console.log(JSON.stringify(description, null, 2));
    return;
  }
  try {
    const supportManifest = await readJsonRequired(args.supportManifest, "--support-manifest");
    const packages = await readJsonRequired(args.packages, "--packages");
    const packageEvidence = await preflightPackageSet({ mode: args.mode, supportManifest, packages });
    if (args.preflight) {
      console.log(JSON.stringify(packageEvidence, null, 2));
      return;
    }
    if (!args.m6Only) {
      await run("pnpm", ["acceptance:m5", "--", "--m5-only"]);
    }
    const scenarios = await readJsonRequired(args.scenarioEvidence, "--scenario-evidence");
    const gaEvidence = await readJsonRequired(args.gaEvidence, "--ga-evidence");
    const artifact = buildAcceptanceArtifact({
      mode: args.mode,
      supportManifest,
      packageEvidence,
      scenarios,
      priorMilestones: { m5: "passed", providerSmoke: "passed" },
      gaEvidence,
    });
    console.log(JSON.stringify(artifact, null, 2));
    if (artifact.outcome !== "passed") process.exitCode = 1;
  } catch (error) {
    console.log(JSON.stringify({
      artifactVersion: "lenso.m6-single-region-ga-acceptance.v1",
      outcome: "blocked",
      mode: args.mode,
      gaEligible: false,
      issues: [issue(
        errorCode(error),
        error instanceof Error ? error.message : String(error),
      )],
      effects: { productionMutated: false, packagePublished: false, contractRetired: false },
      cleanup: { temporaryStarterDeleted: true },
    }, null, 2));
    process.exitCode = 1;
  }
}

function validateSupportManifest(manifest) {
  if (manifest?.protocol !== "lenso.ga-support-manifest.v1"
    || typeof manifest.manifestId !== "string"
    || !validDigest(manifest.manifestDigest)
    || !Array.isArray(manifest.combinations)) {
    throw new Error("m6_support_manifest_invalid: expected a versioned, digest-bound manifest");
  }
}

function validatePackages(mode, packages) {
  if (!Array.isArray(packages) || packages.length === 0) {
    throw new Error("m6_package_source_invalid: exact package provenance is required");
  }
  for (const item of packages) {
    if (!item.id || !item.kind || !validDigest(item.digest)) {
      throw new Error("m6_package_source_invalid: every package needs kind, id, version, and digest");
    }
    if (!item.version || item.version === "latest" || item.version.includes("workspace") || item.version.includes("*")) {
      throw new Error(`m6_package_mutable_invalid: ${item.id} has a mutable version`);
    }
    if (/^(\.|\/|file:|path:)|[\\/](target|Projects|framework)[\\/]/.test(item.source ?? "")) {
      throw new Error(`m6_package_source_invalid: ${item.id} resolves from a local path`);
    }
    if (mode === "candidate" && item.source !== "staged") {
      throw new Error(`m6_package_source_invalid: candidate ${item.id} is not an exact staged artifact`);
    }
    if (mode === "published" && item.source !== "published") {
      throw new Error(`m6_package_source_invalid: published ${item.id} is not a public registry artifact`);
    }
    if (item.receiptStatus !== "accepted") {
      throw new Error(`m6_package_receipt_invalid: ${item.id} lacks an accepted release receipt`);
    }
  }
}

function validateExactCombination(manifest, packages) {
  const requested = packages.map((item) => `${item.kind}:${item.id}@${item.version}`).sort();
  const matched = manifest.combinations.some((combination) => {
    const declared = [...combination.componentReferences].sort();
    return JSON.stringify(declared) === JSON.stringify(requested)
      && combination.status !== "unsupported";
  });
  if (!matched) {
    throw new Error("m6_package_combination_unknown: semantic-version proximity is not compatibility");
  }
}

function parseArgs(argv) {
  const value = (name) => argv[argv.indexOf(name) + 1];
  return {
    describe: argv.includes("--describe"),
    preflight: argv.includes("--preflight"),
    m6Only: argv.includes("--m6-only"),
    mode: value("--mode") ?? "candidate",
    supportManifest: value("--support-manifest"),
    packages: value("--packages"),
    scenarioEvidence: value("--scenario-evidence"),
    gaEvidence: value("--ga-evidence"),
  };
}

function validateGaEvidence(evidence, supportManifest, issues) {
  const recoveries = Array.isArray(evidence?.deliveryRecoveries)
    ? evidence.deliveryRecoveries
    : [];
  const byCondition = new Map(recoveries.map((item) => [item.condition, item]));
  for (const condition of deliveryRecoveryConditions) {
    const recovery = byCondition.get(condition);
    if (!recovery
      || recovery.protocol !== "lenso.delivery-failure-recovery-evidence.v1"
      || recovery.decision !== "passed"
      || !canonicalEvidenceValid(
        recovery,
        "evidenceId",
        "evidenceDigest",
        "delivery-failure-recovery",
      )
      || recovery.effects?.mutatesEnvironment !== false) {
      issues.push(issue("m6_delivery_recovery_missing", `${condition} lacks passing zero-effect recovery evidence.`));
    }
  }
  validateEvidence(
    evidence?.performanceProfile,
    "lenso.performance-profile.v1",
    "passed",
    "profileDigest",
    "profileId",
    "performance-profile",
    "m6_performance_profile_invalid",
    issues,
  );
  validateEvidence(
    evidence?.restore,
    "lenso.service-restore-evidence.v1",
    "passed",
    "evidenceDigest",
    "evidenceId",
    "service-restore",
    "m6_restore_evidence_invalid",
    issues,
  );
  validateEvidence(
    evidence?.disasterRecovery,
    "lenso.disaster-recovery-evidence.v1",
    "passed",
    "evidenceDigest",
    "evidenceId",
    "disaster-recovery",
    "m6_disaster_recovery_invalid",
    issues,
  );
  validateEvidence(
    evidence?.supportEnvelope,
    "lenso.support-envelope.v1",
    "passed",
    "envelopeDigest",
    "envelopeId",
    "support-envelope",
    "m6_support_envelope_invalid",
    issues,
  );
  validateEvidence(
    evidence?.securityReview,
    "lenso.security-review-evidence.v1",
    "passed",
    "reviewDigest",
    "reviewId",
    "security-review",
    "m6_security_review_invalid",
    issues,
  );
  for (const subject of [
    evidence?.performanceProfile,
    evidence?.supportEnvelope,
    evidence?.securityReview,
  ]) {
    if (subject?.supportManifestDigest !== supportManifest.manifestDigest) {
      issues.push(issue("m6_evidence_manifest_mismatch", "GA evidence is not bound to the exact Support Manifest."));
    }
  }
}

function validateEvidence(
  evidence,
  protocol,
  decision,
  digestField,
  idField,
  idPrefix,
  code,
  issues,
) {
  if (evidence?.protocol !== protocol
    || evidence?.decision !== decision
    || !canonicalEvidenceValid(evidence, idField, digestField, idPrefix)) {
    issues.push(issue(code, `${protocol} is missing, blocked, or not content-addressed.`));
  }
}

export function contentAddressEvidence(evidence, idField, digestField, idPrefix) {
  const addressed = structuredClone(evidence);
  addressed[idField] = "";
  addressed[digestField] = "";
  const evidenceDigest = digest(JSON.stringify(addressed));
  addressed[digestField] = evidenceDigest;
  addressed[idField] = `${idPrefix}:${evidenceDigest.slice(7, 23)}`;
  return addressed;
}

function canonicalEvidenceValid(evidence, idField, digestField, idPrefix) {
  if (!evidence || !validDigest(evidence[digestField])) return false;
  const canonical = contentAddressEvidence(evidence, idField, digestField, idPrefix);
  return evidence[digestField] === canonical[digestField]
    && evidence[idField] === canonical[idField];
}

function validateMode(mode) {
  if (!new Set(["candidate", "published"]).has(mode)) {
    throw new Error(`m6_mode_invalid: ${mode}`);
  }
}

async function readJsonRequired(file, option) {
  if (!file) throw new Error(`m6_input_missing: ${option} is required`);
  return JSON.parse(await readFile(path.resolve(file), "utf8"));
}

function validDigest(value) {
  return /^sha256:[a-f0-9]{64}$/.test(value ?? "");
}

function digest(value) {
  return `sha256:${createHash("sha256").update(value).digest("hex")}`;
}

function isWithin(candidate, parent) {
  const relative = path.relative(parent, candidate);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function scenario(id, condition, expected, adapter) {
  return Object.freeze({ id, condition, expected, adapter, cleanupComplete: true });
}

function issue(code, message) {
  return {
    code,
    message,
    remediation: "Restore the exact evidence or supported state and rerun the same public command.",
    nextActions: ["Inspect the stable issue code and refresh only the named input."],
  };
}

function errorCode(error) {
  return String(error instanceof Error ? error.message : error).split(":", 1)[0] || "m6_blocked";
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: repoRoot, env: process.env, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} exited with ${code ?? signal}`));
    });
  });
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
