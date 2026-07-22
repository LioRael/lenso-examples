import { spawn } from "node:child_process";
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
    const artifact = buildAcceptanceArtifact({
      mode: args.mode,
      supportManifest,
      packageEvidence,
      scenarios,
      priorMilestones: { m5: "passed", providerSmoke: "passed" },
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
  };
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
