import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildAcceptanceArtifact,
  preflightPackageSet,
  scenarioMatrix,
} from "./m6-acceptance.mjs";

const digest = (value) => `sha256:${value.repeat(64)}`;

function supportManifest() {
  return {
    protocol: "lenso.ga-support-manifest.v1",
    manifestId: "ga-support:test",
    manifestDigest: digest("a"),
    status: "candidate",
    combinations: [{
      combinationId: "candidate-1",
      componentReferences: ["cli:@lenso/cli@0.1.30", "runtime:lenso-service@0.1.4"],
      stateVersion: "service-store.v1",
      status: "candidate",
    }],
  };
}

function packages() {
  return [
    { id: "@lenso/cli", kind: "cli", version: "0.1.30", digest: digest("b"), source: "staged", receiptStatus: "accepted" },
    { id: "lenso-service", kind: "runtime", version: "0.1.4", digest: digest("c"), source: "staged", receiptStatus: "accepted" },
  ];
}

function verifiedScenarios() {
  return scenarioMatrix.map((item) => ({
    ...item,
    protocol: "lenso.failure-scenario-evidence.v1",
    evidenceDigest: digest("e"),
    controlledTimeUnixMs: 1_721_600_000_000,
    decision: "supported",
    observations: [{ subject: item.id, outcome: item.expected, evidenceDigest: digest("d") }],
    proofs: [{ commandId: `${item.id}-proof`, commandDigest: digest("f"), scenarioBound: true }],
    adapterVersion: item.adapter ? "test-adapter-1" : undefined,
  }));
}

test("candidate preflight creates an isolated starter and can never claim GA", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "lenso-m6-test-"));
  try {
    const result = await preflightPackageSet({
      mode: "candidate",
      supportManifest: supportManifest(),
      packages: packages(),
      temporaryRoot: root,
    });
    assert.equal(result.outcome, "passed");
    assert.equal(result.gaEligible, false);
    assert.equal(result.provenance.every((item) => item.digest.startsWith("sha256:")), true);
    assert.equal(result.effects.productionMutated, false);
    assert.equal(result.cleanup.temporaryStarterDeleted, true);
    await assert.rejects(readFile(result.starterRoot), /ENOENT/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("published mode rejects staged, mutable, local, or receipt-pending packages", async () => {
  for (const mutation of [
    { source: "staged" },
    { source: "../lenso/target/debug/lenso" },
    { version: "latest", source: "published" },
    { receiptStatus: "pending", source: "published" },
  ]) {
    const subject = packages().map((item) => ({ ...item, source: "published" }));
    Object.assign(subject[0], mutation);
    await assert.rejects(
      preflightPackageSet({
        mode: "published",
        supportManifest: supportManifest(),
        packages: subject,
      }),
      /m6_package_(source|mutable|receipt)_invalid/,
    );
  }
});

test("the first recovery matrix covers process, Store, NATS, SPIFFE, and plane outages", () => {
  assert.deepEqual(scenarioMatrix.map((item) => item.condition), [
    "api_crash",
    "worker_crash",
    "network_partitioned",
    "postgres_store_unavailable",
    "nats_disconnected",
    "nats_acknowledgement_lost",
    "nats_redelivery",
    "nats_poison_event",
    "spiffe_workload_api_unavailable",
    "spiffe_credential_expired",
    "spiffe_credential_rotated",
    "telemetry_unavailable",
    "story_aggregation_unavailable",
    "runtime_console_unavailable",
    "system_plane_unavailable",
  ]);
  assert.equal(scenarioMatrix.every((item) => item.cleanupComplete), true);
});

test("M6 artifact fails on one unexpected effect and keeps candidate gaEligible false", () => {
  const scenarios = verifiedScenarios();
  const passed = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios,
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
  });
  assert.equal(passed.outcome, "passed");
  assert.equal(passed.gaEligible, false);
  assert.equal(passed.artifactVersion, "lenso.m6-single-region-ga-acceptance.v1");

  scenarios[3].decision = "unsupported";
  scenarios[3].issues = [{ code: "failure_unexpected_outcome" }];
  const failed = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios,
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
  });
  assert.equal(failed.outcome, "failed");
  assert.equal(failed.gaEligible, false);
  assert.equal(failed.issues[0].code, "failure_unexpected_outcome");
});

test("M6 artifact rejects scenario labels that are not bound to real evidence", () => {
  const scenarios = scenarioMatrix.map((item) => ({
    ...item,
    decision: "supported",
    observations: [{ subject: item.id, outcome: item.expected, evidenceDigest: digest("d") }],
  }));
  const artifact = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios,
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
  });
  assert.equal(artifact.outcome, "failed");
  assert.equal(artifact.issues.some((item) => item.code === "failure_evidence_unverified"), true);
});
