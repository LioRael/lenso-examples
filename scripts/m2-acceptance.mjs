import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = path.join(repoRoot, "examples", "support-system");
const cli = process.env.LENSO_CLI_BIN ?? "lenso";
const deterministicScenarios = [
  "duplicate",
  "delayed",
  "reordered",
  "poison",
  "producer_restart",
  "consumer_restart",
  "transport_interruption",
  "dead_letter_replay",
  "identity_expiry",
  "wrong_audience",
  "missing_tenant",
  "circuit_open",
  "bulkhead",
  "overload",
  "deadline",
  "fallback",
];
const description = {
  artifactVersion: "lenso.m2-acceptance-description.v1",
  publicSeam: "support-system",
  deterministicScenarios,
  productionEvidence: {
    transport: "nats_jetstream_real_environment",
    workloadIdentity: "spiffe_spire_real_environment",
    approvalBoundary: true,
  },
  providerCompatibility: "independent_host_managed_smoke",
};

if (process.argv.includes("--describe")) {
  console.log(JSON.stringify(description, null, 2));
} else {
  await runAcceptance();
}

async function runAcceptance() {
  const database = process.env.DATABASE_URL
    ? { url: process.env.DATABASE_URL, stop: async () => {} }
    : await startPostgres();
  let smokeEvidence;
  try {
    const smokeOutput = await run(
      "node",
      ["scripts/support-system-smoke.mjs"],
      repoRoot,
      true,
      {
        DATABASE_URL: database.url,
        LENSO_CLI_BIN: cli,
        LENSO_SUPPORT_SMOKE_BIN: "support-system-m2-smoke",
        LENSO_SUPPORT_SMOKE_EVIDENCE: "M2_SMOKE_EVIDENCE",
      },
    );
    process.stdout.write(smokeOutput);
    smokeEvidence = JSON.parse(
      smokeOutput.match(/^M2_SMOKE_EVIDENCE=(.+)$/m)?.[1] ?? "null",
    );
  } finally {
    await database.stop();
  }

  assert.equal(smokeEvidence.artifactVersion, "lenso.m2-support-system-smoke.v1");
  assert.equal(smokeEvidence.systemPlaneWithheld, true);
  assert.equal(smokeEvidence.runtimeConsoleWithheld, true);
  assert.equal(smokeEvidence.eventFlow.adapter, "local");
  assert.equal(smokeEvidence.eventFlow.businessEffects, 1);
  assert.equal(
    smokeEvidence.eventFlow.authenticatedServicePrincipal,
    "service:support-ticket-service",
  );
  assert.equal(smokeEvidence.eventFlow.delegatedActor, "user_01");
  assert.equal(smokeEvidence.eventFlow.tenantId, "tenant_01");
  assert.ok(smokeEvidence.eventFlow.localEvidenceRecords > 0);
  assert.equal(smokeEvidence.eventFlow.serviceLocalEvidenceFiles, 2);
  assert.equal(smokeEvidence.eventFlow.systemPlaneWithheld, true);
  assert.equal(smokeEvidence.eventFlow.runtimeConsoleWithheld, true);
  assert.equal(smokeEvidence.eventFlow.cleanupCompleted, true);
  assert.equal(smokeEvidence.callPolicy.circuitRecovered, true);
  assert.equal(smokeEvidence.callPolicy.fallbackHandler, "support.cached_sla");

  const deadlineResult = await runScenario("deadline-timeout");
  const repeatedDeadline = await runScenario("deadline-timeout");
  assert.deepEqual(repeatedDeadline, deadlineResult);
  assert.equal(deadlineResult.artifactVersion, "lenso.failure-scenario-result.v1");
  assert.equal(deadlineResult.outcome, "deadline_exceeded");
  assert.equal(deadlineResult.attempts, 1);
  assert.equal(deadlineResult.retryDecision.attempted, false);
  assert.equal(deadlineResult.retryDecision.reason, "deadline_exhausted");
  assert.equal(deadlineResult.cleanup.completed, true);

  const scenarios = [
    ...smokeEvidence.eventFlow.scenarios,
    ...smokeEvidence.callPolicy.scenarios,
  ].map((scenario) =>
    scenario.scenarioId === "deadline"
      ? {
          scenarioId: "deadline",
          outcome: deadlineResult.outcome,
          attempts: deadlineResult.attempts,
          retryDecision: deadlineResult.retryDecision,
          cleanup: deadlineResult.cleanup,
        }
      : scenario,
  );
  assert.deepEqual(
    scenarios.map(({ scenarioId }) => scenarioId),
    deterministicScenarios,
  );
  assert.ok(scenarios.every(({ outcome }) => typeof outcome === "string" && outcome.length > 0));

  await run("pnpm", ["smoke:support-ticket"], repoRoot, false, {
    LENSO_CLI_BIN: cli,
  });

  console.log(
    JSON.stringify(
      {
        artifactVersion: "lenso.m2-reliable-communication-acceptance.v1",
        outcome: "passed",
        publicSeam: description.publicSeam,
        directServiceCalls: {
          systemPlaneWithheld: smokeEvidence.systemPlaneWithheld,
          runtimeConsoleWithheld: smokeEvidence.runtimeConsoleWithheld,
          successfulStorySegments: smokeEvidence.successfulStorySegments,
        },
        asyncEvents: smokeEvidence.eventFlow,
        scenarios,
        providerSmoke: "passed",
        productionEvidence: {
          ...description.productionEvidence,
          status: "separate_approval_boundary",
        },
      },
      null,
      2,
    ),
  );
}

async function runScenario(scenarioId) {
  const output = await run(
    cli,
    ["system", "dev", "--scenario", scenarioId, "--json"],
    fixtureRoot,
    true,
    { LENSO_CLI_BIN: cli },
  );
  return JSON.parse(output);
}

async function startPostgres() {
  const dataDir = await mkdtemp(path.join(os.tmpdir(), "lenso-m2-postgres-"));
  const port = await reservePort();
  let started = false;
  try {
    await run("initdb", ["-D", dataDir, "-U", "postgres", "-A", "trust", "--encoding=UTF8", "--no-locale"], repoRoot);
    await run(
      "pg_ctl",
      ["-D", dataDir, "-o", `-F -p ${port} -h 127.0.0.1`, "-w", "start"],
      repoRoot,
    );
    started = true;
    return {
      url: `postgres://postgres@127.0.0.1:${port}/postgres`,
      stop: async () => {
        await run("pg_ctl", ["-D", dataDir, "-m", "immediate", "-w", "stop"], repoRoot);
        await rm(dataDir, { recursive: true, force: true });
      },
    };
  } catch (error) {
    if (started) {
      await run("pg_ctl", ["-D", dataDir, "-m", "immediate", "-w", "stop"], repoRoot).catch(() => {});
    }
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
      server.close((error) => {
        if (error) reject(error);
        else resolve(address.port);
      });
    });
  });
}

function run(command, args, cwd, capture = false, extraEnv = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: { ...process.env, ...extraEnv },
      stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    });
    let stdout = "";
    child.stdout?.on("data", (chunk) => {
      stdout += chunk;
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve(stdout);
      else reject(new Error(`${command} exited with ${code ?? signal}`));
    });
  });
}
