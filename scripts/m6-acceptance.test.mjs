import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildAcceptanceArtifact,
  contentAddressEvidence,
  preflightPackageSet,
  deliveryRecoveryConditions,
  scenarioEvidenceFromCommands,
  scenarioMatrix,
  receiptContent,
  supportManifestDigest,
} from "./m6-acceptance.mjs";

const digest = (value) => `sha256:${value.repeat(64)}`;
const hash = (value) => `sha256:${createHash("sha256").update(value).digest("hex")}`;
const receiptKeys = generateKeyPairSync("ed25519");
const receiptAuthority = "m6-test-verifier";
const receiptPublicKey = receiptKeys.publicKey.export({ type: "spki", format: "pem" });

function supportManifest() {
  const manifest = {
    protocol: "lenso.ga-support-manifest.v1",
    manifestId: "",
    manifestDigest: "",
    status: "candidate",
    components: [
      { kind: "cli", componentId: "@lenso/cli", version: "0.1.30", digest: digest("b") },
      { kind: "runtime", componentId: "lenso-service", version: "0.1.4", digest: digest("c") },
    ],
    manifestFormats: [{ kind: "service", version: "lenso.service.v2" }],
    stateVersions: ["service-store.v1"],
    adapterVersions: { postgresql: "18" },
    documentation: { version: "m6-ga", digest: digest("d") },
    combinations: [{
      combinationId: "candidate-1",
      componentReferences: ["cli:@lenso/cli@0.1.30", "runtime:lenso-service@0.1.4"],
      stateVersion: "service-store.v1",
      status: "candidate",
    }],
    upgradeEdges: [],
    evidenceReceiptAuthorities: Object.fromEntries([
      "lenso.delivery-failure-recovery-evidence.v1",
      "lenso.performance-profile.v1",
      "lenso.service-restore-evidence.v1",
      "lenso.disaster-recovery-evidence.v1",
      "lenso.support-envelope.v1",
      "lenso.security-review-evidence.v1",
    ].map((protocol) => [protocol, receiptAuthority])),
    receiptAuthorityPublicKeys: { [receiptAuthority]: receiptPublicKey },
  };
  manifest.manifestDigest = supportManifestDigest(manifest);
  manifest.manifestId = `ga-support:${manifest.manifestDigest.slice(7, 23)}`;
  return manifest;
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

function gaEvidence() {
  const manifestDigest = supportManifest().manifestDigest;
  const addressed = (value, idField, digestField, prefix) =>
    contentAddressEvidence(value, idField, digestField, prefix);
  const evidence = {
    deliveryRecoveries: deliveryRecoveryConditions.map((condition) => {
      const environment = new Set([
        "deployment_adapter_rejected",
        "operator_reconciliation_failed",
        "gateway_drift",
        "migration_failed",
      ]).has(condition);
      return addressed({
        protocol: "lenso.delivery-failure-recovery-evidence.v1",
        evidenceId: "",
        condition,
        scope: environment ? "environment_verification" : "deterministic",
        environmentObservation: environment ? {
          clusterIdentity: "kind:m6",
          apiServerVersion: "1.35",
          operatorVersion: "0.2.9",
          gatewayAdapterVersion: "1.4",
          usedRealApi: true,
          observedResourceVersion: "42",
          evidenceDigest: hash(`kubernetes:${condition}`),
        } : undefined,
        decision: "passed",
        evidenceDigest: "",
        effects: { mutatesEnvironment: false },
      }, "evidenceId", "evidenceDigest", "delivery-failure-recovery");
    }),
    performanceProfile: addressed({
      protocol: "lenso.performance-profile.v1",
      profileId: "",
      decision: "passed",
      profileDigest: "",
      supportManifestDigest: manifestDigest,
    }, "profileId", "profileDigest", "performance-profile"),
    restore: addressed({
      protocol: "lenso.service-restore-evidence.v1",
      evidenceId: "",
      decision: "passed",
      evidenceDigest: "",
    }, "evidenceId", "evidenceDigest", "service-restore"),
    disasterRecovery: addressed({
      protocol: "lenso.disaster-recovery-evidence.v1",
      evidenceId: "",
      decision: "passed",
      evidenceDigest: "",
    }, "evidenceId", "evidenceDigest", "disaster-recovery"),
    supportEnvelope: addressed({
      protocol: "lenso.support-envelope.v1",
      envelopeId: "",
      decision: "passed",
      envelopeDigest: "",
      supportManifestDigest: manifestDigest,
    }, "envelopeId", "envelopeDigest", "support-envelope"),
    securityReview: addressed({
      protocol: "lenso.security-review-evidence.v1",
      reviewId: "",
      decision: "passed",
      reviewDigest: "",
      supportManifestDigest: manifestDigest,
    }, "reviewId", "reviewDigest", "security-review"),
  };
  const artifactDigests = {
    "lenso.delivery-failure-recovery-evidence.v1": evidence.deliveryRecoveries.map((item) => item.evidenceDigest),
    "lenso.performance-profile.v1": [evidence.performanceProfile.profileDigest],
    "lenso.service-restore-evidence.v1": [evidence.restore.evidenceDigest],
    "lenso.disaster-recovery-evidence.v1": [evidence.disasterRecovery.evidenceDigest],
    "lenso.support-envelope.v1": [evidence.supportEnvelope.envelopeDigest],
    "lenso.security-review-evidence.v1": [evidence.securityReview.reviewDigest],
  };
  evidence.executionReceipts = Object.fromEntries(
    Object.entries(artifactDigests).map(([protocol, digests]) => [
      protocol,
      executionReceipt(protocol, digests),
    ]),
  );
  return evidence;
}

function executionReceipt(subjectProtocol, artifactDigests) {
  const base = {
    protocol: "lenso.m6-execution-receipt.v1",
    subjectProtocol,
    artifactDigests: [...artifactDigests].sort(),
    commandDigest: hash(`command:${subjectProtocol}`),
    authority: receiptAuthority,
    status: "accepted",
    cleanupComplete: true,
    productionMutated: false,
    observedAtUnixMs: 1_721_600_000_000,
  };
  const receiptDigest = hash(JSON.stringify(receiptContent(base)));
  return {
    ...base,
    receiptId: `m6-execution-receipt:${receiptDigest.slice(7, 23)}`,
    receiptDigest,
    signature: sign(null, Buffer.from(receiptDigest), receiptKeys.privateKey).toString("base64"),
  };
}

test("candidate preflight creates an isolated starter and can never claim GA", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "lenso-m6-test-"));
  try {
    const cliPackageRoot = path.join(root, "cli-package", "package");
    const cliArtifact = path.join(root, "lenso-cli-0.1.30.tgz");
    const runtimeArtifact = path.join(root, "lenso-service.crate");
    await mkdir(path.join(cliPackageRoot, "bin"), { recursive: true });
    await writeFile(path.join(cliPackageRoot, "package.json"), JSON.stringify({
      name: "@lenso/cli",
      version: "0.1.30",
      bin: { lenso: "bin/lenso.js" },
    }));
    await writeFile(
      path.join(cliPackageRoot, "bin", "lenso.js"),
      '#!/usr/bin/env node\nconsole.log("lenso 0.1.30");\n',
      { mode: 0o755 },
    );
    const packed = spawnSync("tar", ["-czf", cliArtifact, "-C", path.dirname(cliPackageRoot), "package"]);
    assert.equal(packed.status, 0, packed.stderr?.toString());
    await writeFile(runtimeArtifact, "immutable staged runtime artifact\n");
    const candidatePackages = [
      {
        id: "@lenso/cli",
        kind: "cli",
        version: "0.1.30",
        digest: hash(await readFile(cliArtifact)),
        source: "staged",
        receiptStatus: "accepted",
        artifactPath: cliArtifact,
      },
      {
        id: "lenso-service",
        kind: "runtime",
        version: "0.1.4",
        digest: hash(await readFile(runtimeArtifact)),
        source: "staged",
        receiptStatus: "accepted",
        artifactPath: runtimeArtifact,
      },
    ];
    const manifest = supportManifest();
    manifest.components = manifest.components.map((component) => ({
      ...component,
      digest: candidatePackages.find((item) => item.id === component.componentId).digest,
    }));
    manifest.manifestDigest = supportManifestDigest(manifest);
    manifest.manifestId = `ga-support:${manifest.manifestDigest.slice(7, 23)}`;
    const result = await preflightPackageSet({
      mode: "candidate",
      supportManifest: manifest,
      trustedManifestDigest: manifest.manifestDigest,
      packages: candidatePackages,
      temporaryRoot: root,
    });
    assert.equal(result.outcome, "passed");
    assert.equal(result.gaEligible, false);
    assert.equal(result.provenance.every((item) => item.digest.startsWith("sha256:")), true);
    assert.equal(result.effects.productionMutated, false);
    assert.equal(result.candidateTrace.outcome, "passed");
    assert.equal(result.candidateTrace.consumedDigests.length, 2);
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
        trustedManifestDigest: supportManifest().manifestDigest,
        packages: subject,
      }),
      /m6_package_(source|mutable|receipt)_invalid/,
    );
  }
});

test("preflight rejects a self-certified support manifest", async () => {
  await assert.rejects(
    preflightPackageSet({
      mode: "candidate",
      supportManifest: supportManifest(),
      trustedManifestDigest: hash("different reviewed manifest"),
      packages: packages(),
    }),
    /m6_support_manifest_untrusted/,
  );
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

test("environment commands produce digest-bound evidence for every scenario", () => {
  const commands = [
    "sandbox",
    "nats-conformance",
    "nats-restart",
    "nats-poison",
    "spiffe",
    "coordination-outage",
  ].map((id, index) => ({ id, digest: digest(String(index + 1)) }));
  const scenarios = scenarioEvidenceFromCommands(commands, 1_721_600_000_000);
  assert.equal(scenarios.length, scenarioMatrix.length);
  assert.equal(scenarios.every((item) => item.decision === "supported"), true);
  assert.equal(scenarios.every((item) => item.proofs[0].scenarioBound === true), true);
  assert.equal(
    scenarios.find((item) => item.condition === "nats_redelivery").proofs[0].commandId,
    "nats-restart",
  );
  assert.equal(
    scenarios.find((item) => item.condition === "system_plane_unavailable").proofs[0].commandId,
    "coordination-outage",
  );
  assert.throws(
    () => scenarioEvidenceFromCommands(commands.filter((item) => item.id !== "spiffe"), 0),
    /failure_evidence_unverified: missing spiffe command evidence/,
  );
});

test("M6 artifact fails on one unexpected effect and keeps candidate gaEligible false", () => {
  const scenarios = verifiedScenarios();
  const passed = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios,
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
    gaEvidence: gaEvidence(),
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
    gaEvidence: gaEvidence(),
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
    gaEvidence: gaEvidence(),
  });
  assert.equal(artifact.outcome, "failed");
  assert.equal(artifact.issues.some((item) => item.code === "failure_evidence_unverified"), true);
});

test("M6 artifact requires recovery, performance, restore, DR, envelope, and security evidence", () => {
  const evidence = gaEvidence();
  evidence.deliveryRecoveries.pop();
  evidence.securityReview.decision = "blocked";
  const artifact = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios: verifiedScenarios(),
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
    gaEvidence: evidence,
  });
  assert.equal(artifact.outcome, "failed");
  assert.equal(artifact.issues.some((item) => item.code === "m6_delivery_recovery_missing"), true);
  assert.equal(artifact.issues.some((item) => item.code === "m6_security_review_invalid"), true);
});

test("M6 artifact rejects evidence changed after content addressing", () => {
  const evidence = gaEvidence();
  evidence.restore.decision = "blocked";
  evidence.performanceProfile.supportManifestDigest = digest("9");
  const artifact = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios: verifiedScenarios(),
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
    gaEvidence: evidence,
  });
  assert.equal(artifact.outcome, "failed");
  assert.equal(artifact.issues.some((item) => item.code === "m6_restore_evidence_invalid"), true);
  assert.equal(artifact.issues.some((item) => item.code === "m6_performance_profile_invalid"), true);
});

test("M6 artifact rejects forged receipts and mock-only Kubernetes recovery", () => {
  const evidence = gaEvidence();
  evidence.executionReceipts["lenso.service-restore-evidence.v1"].signature =
    Buffer.from("forged").toString("base64");
  const gateway = evidence.deliveryRecoveries.find(
    (item) => item.condition === "gateway_drift",
  );
  gateway.environmentObservation.usedRealApi = false;
  const artifact = buildAcceptanceArtifact({
    mode: "candidate",
    supportManifest: supportManifest(),
    packageEvidence: { outcome: "passed", provenance: packages(), cleanup: { temporaryStarterDeleted: true } },
    scenarios: verifiedScenarios(),
    priorMilestones: { m5: "passed", providerSmoke: "passed" },
    gaEvidence: evidence,
  });
  assert.equal(artifact.outcome, "failed");
  assert.equal(artifact.issues.some((item) => item.code === "m6_execution_receipt_invalid"), true);
  assert.equal(artifact.issues.some((item) => item.code === "m6_delivery_recovery_missing"), true);
});
