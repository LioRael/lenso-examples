import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const description = {
  artifactVersion: "lenso.m4-acceptance-description.v1",
  publicSeam: "support-system",
  workflow: [
    "blocked_readiness",
    "corrected_readiness",
    "deterministic_plan",
    "identity_preserving_scaffold",
    "destination_expansion",
    "interrupted_resumable_backfill",
    "reconciliation",
    "behavior_verification",
    "quiescence_and_drain",
    "failed_provisional_rollback",
    "approval_boundary",
    "authority_commit",
    "stale_evidence_rejection",
    "post_commit_rollback_block",
  ],
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
  if (!process.argv.includes("--m4-only")) await run("pnpm", ["acceptance:m3"]);
  await runAcceptance();
}

async function runAcceptance() {
  const database = process.env.DATABASE_URL
    ? { url: process.env.DATABASE_URL, stop: async () => {} }
    : await startPostgres();
  let evidence;
  try {
    const businessOutput = await run(
      "node",
      ["scripts/support-system-smoke.mjs"],
      true,
      { LENSO_SUPPORT_SMOKE_EVIDENCE: "M4_BUSINESS_EVIDENCE" },
    );
    process.stdout.write(businessOutput);
    const businessEvidence = JSON.parse(
      businessOutput.match(/^M4_BUSINESS_EVIDENCE=(.+)$/m)?.[1] ?? "null",
    );
    assert.equal(businessEvidence.systemPlaneWithheld, true);
    assert.equal(businessEvidence.runtimeConsoleWithheld, true);
    const output = await run(
      "cargo",
      ["run", "--locked", "--manifest-path", "examples/support-system/Cargo.toml", "--bin", "support-system-m4-smoke"],
      true,
      {
        DATABASE_URL: database.url,
        M4_BUSINESS_EVIDENCE: JSON.stringify(businessEvidence),
      },
    );
    process.stdout.write(output);
    evidence = JSON.parse(output.match(/^M4_SMOKE_EVIDENCE=(.+)$/m)?.[1] ?? "null");
  } finally {
    await database.stop();
  }
  assert.equal(evidence.artifactVersion, "lenso.m4-safe-module-extraction-acceptance.v1");
  assert.equal(evidence.outcome, "passed");
  assert.equal(evidence.publicSeam, "support-system");
  assert.ok(evidence.blockedIssueCodes.length > 0);
  assert.equal(evidence.durableBackfillResumed, true);
  assert.equal(evidence.reconciliationMismatchBlocked, true);
  assert.equal(evidence.behaviorVerified, true);
  assert.equal(evidence.failedCutoverRolledBack, true);
  assert.equal(evidence.failedLinkedProbeKeptWritesPaused, true);
  assert.equal(evidence.staleApprovalRejected, true);
  assert.equal(evidence.authorityCommitted, true);
  assert.equal(evidence.postCommitFastRollbackBlocked, true);
  assert.equal(evidence.evidencePersisted, true);
  console.log(JSON.stringify(evidence, null, 2));
}

async function startPostgres() {
  const dataDir = await mkdtemp(path.join(os.tmpdir(), "lenso-m4-postgres-"));
  const port = await reservePort();
  let started = false;
  try {
    await run("initdb", ["-D", dataDir, "-U", "postgres", "-A", "trust", "--encoding=UTF8", "--no-locale"]);
    await run("pg_ctl", ["-D", dataDir, "-o", `-F -p ${port} -h 127.0.0.1`, "-w", "start"]);
    started = true;
    return {
      url: `postgres://postgres@127.0.0.1:${port}/postgres`,
      stop: async () => {
        await run("pg_ctl", ["-D", dataDir, "-m", "immediate", "-w", "stop"]);
        await rm(dataDir, { recursive: true, force: true });
      },
    };
  } catch (error) {
    if (started) await run("pg_ctl", ["-D", dataDir, "-m", "immediate", "-w", "stop"]).catch(() => {});
    await rm(dataDir, { recursive: true, force: true });
    throw error;
  }
}

function reservePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => (error ? reject(error) : resolve(address.port)));
    });
  });
}

function run(command, args, capture = false, extraEnv = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: repoRoot,
      env: { ...process.env, ...extraEnv },
      stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    });
    let stdout = "";
    child.stdout?.on("data", (chunk) => { stdout += chunk; });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve(stdout);
      else reject(new Error(`${command} exited with ${code ?? signal}`));
    });
  });
}
