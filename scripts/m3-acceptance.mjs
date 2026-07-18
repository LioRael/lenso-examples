import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cli = process.env.LENSO_CLI_BIN ?? "lenso";
const description = {
  artifactVersion: "lenso.m3-acceptance-description.v1",
  publicSeam: "support-system",
  workflow: {
    owner: "support-sla",
    services: ["support-ticket-service", "support-sla-service"],
    capabilities: [
      "child_workflow",
      "participant_restart",
      "controlled_timeout",
      "exactly_once_compensation",
      "version_pinning",
      "worker_mismatch_rejection",
    ],
  },
  evidence: [
    "delayed_federation",
    "late_evidence",
    "explicit_segment_gap",
    "workflow_reliability",
  ],
  planesWithheld: ["runtime_console", "story_aggregator", "system_plane"],
  priorGuarantees: "m2_acceptance",
  providerCompatibility: "independent_host_managed_smoke",
  externalWorkflowEngineRequired: false,
  kubernetesRequired: false,
  productionAuthorityRequired: false,
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
  let evidence;
  try {
    const output = await run(
      "node",
      ["scripts/support-system-smoke.mjs"],
      repoRoot,
      true,
      {
        DATABASE_URL: database.url,
        LENSO_CLI_BIN: cli,
        LENSO_SUPPORT_SMOKE_BIN: "support-system-m3-smoke",
        LENSO_SUPPORT_SMOKE_EVIDENCE: "M3_SMOKE_EVIDENCE",
      },
    );
    process.stdout.write(output);
    evidence = JSON.parse(output.match(/^M3_SMOKE_EVIDENCE=(.+)$/m)?.[1] ?? "null");
  } finally {
    await database.stop();
  }

  assert.equal(evidence.artifactVersion, "lenso.m3-support-system-smoke.v1");
  assert.equal(evidence.publicSeam, "support-system");
  assert.deepEqual(evidence.workflow.servicePath, description.workflow.services);
  assert.equal(evidence.workflow.childWorkflowVersion, "v1");
  assert.ok(evidence.workflow.participantRestarts >= 4);
  assert.equal(evidence.workflow.controlledTimeout, true);
  assert.deepEqual(evidence.workflow.compensationOrder, [
    "release_on_call",
    "withdraw_sla_acknowledgement",
  ]);
  assert.equal(evidence.workflow.completedEffects, 2);
  assert.equal(evidence.workflow.compensationEffects, 2);
  assert.equal(evidence.workflow.duplicateCompensationEffects, 2);
  assert.equal(evidence.workflow.finalState, "compensated");

  assert.equal(evidence.versioning.pinnedInstanceVersion, "v1");
  assert.equal(evidence.versioning.newInstanceVersion, "v2");
  assert.equal(evidence.versioning.migrationCompatibility, "safe");
  assert.equal(evidence.versioning.workerMismatchCompatibility, "blocked");
  assert.equal(
    evidence.versioning.workerMismatchError,
    "workflow_definition_version_unsupported",
  );
  assert.equal(evidence.versioning.workerMismatchMutatedState, false);

  assert.equal(evidence.planeIndependence.systemPlaneWithheld, true);
  assert.equal(evidence.planeIndependence.runtimeConsoleWithheld, true);
  assert.equal(evidence.planeIndependence.aggregationWithheldDuringExecution, true);
  assert.equal(evidence.planeIndependence.localEvidenceCaptured, true);
  assert.equal(evidence.federation.artifactVersion, "lenso.federated-runtime-story.v1");
  assert.equal(evidence.federation.storyId, "story_support_case_01");
  assert.ok(evidence.federation.initialSegmentCount > 0);
  assert.ok(evidence.federation.finalSegmentCount > evidence.federation.initialSegmentCount);
  assert.equal(evidence.federation.lateEvidenceAccepted, true);
  assert.ok(evidence.federation.gapKinds.includes("unreachable"));

  assert.equal(evidence.reliability.artifactVersion, "lenso.reliability-report.v1");
  assert.equal(evidence.reliability.contractId, "support-reliability");
  assert.equal(evidence.reliability.profile, "critical");
  assert.equal(evidence.reliability.state, "degraded");
  assert.equal(evidence.reliability.workflowBacklogCheck, "breached");
  assert.ok(evidence.reliability.issueCodes.includes("workflow_backlog_limit_exceeded"));
  assert.equal(evidence.reliability.reportsOnly, true);

  assert.equal(evidence.priorGuarantees.m2ArtifactVersion, "lenso.m2-support-system-smoke.v1");
  assert.equal(evidence.priorGuarantees.directCallPassed, true);
  assert.equal(evidence.priorGuarantees.eventBusinessEffects, 1);
  assert.equal(
    evidence.priorGuarantees.authenticatedServicePrincipal,
    "service:support-ticket-service",
  );
  assert.equal(evidence.priorGuarantees.delegatedActor, "user_01");
  assert.equal(evidence.priorGuarantees.tenantId, "tenant_01");

  await run("pnpm", ["smoke:support-ticket"], repoRoot, false, {
    LENSO_CLI_BIN: cli,
  });

  console.log(
    JSON.stringify(
      {
        artifactVersion: "lenso.m3-durable-processes-federated-evidence-acceptance.v1",
        outcome: "passed",
        publicSeam: description.publicSeam,
        workflow: evidence.workflow,
        versioning: evidence.versioning,
        planeIndependence: evidence.planeIndependence,
        federation: evidence.federation,
        reliability: evidence.reliability,
        priorGuarantees: evidence.priorGuarantees,
        providerSmoke: "passed",
        localRequirements: {
          kubernetes: false,
          externalWorkflowEngine: false,
          productionAuthority: false,
        },
      },
      null,
      2,
    ),
  );
}

async function startPostgres() {
  const dataDir = await mkdtemp(path.join(os.tmpdir(), "lenso-m3-postgres-"));
  const port = await reservePort();
  let started = false;
  try {
    await run(
      "initdb",
      [
        "-D",
        dataDir,
        "-U",
        "postgres",
        "-A",
        "trust",
        "--encoding=UTF8",
        "--no-locale",
      ],
      repoRoot,
    );
    await run(
      "pg_ctl",
      ["-D", dataDir, "-o", `-F -p ${port} -h 127.0.0.1`, "-w", "start"],
      repoRoot,
    );
    started = true;
    return {
      url: `postgres://postgres@127.0.0.1:${port}/postgres`,
      stop: async () => {
        await run(
          "pg_ctl",
          ["-D", dataDir, "-m", "immediate", "-w", "stop"],
          repoRoot,
        );
        await rm(dataDir, { recursive: true, force: true });
      },
    };
  } catch (error) {
    if (started) {
      await run(
        "pg_ctl",
        ["-D", dataDir, "-m", "immediate", "-w", "stop"],
        repoRoot,
      ).catch(() => {});
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
