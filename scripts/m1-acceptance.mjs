import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = path.join(repoRoot, "examples", "support-system");
const cli = process.env.LENSO_CLI_BIN ?? "lenso";
const sandboxState = path.join(
  fixtureRoot,
  ".lenso",
  "system-sandbox",
  "support-platform",
  "state.json",
);

const smokeOutput = await run("node", ["scripts/support-system-smoke.mjs"], repoRoot, true);
process.stdout.write(smokeOutput);
const smokeEvidence = JSON.parse(
  smokeOutput.match(/^M1_SMOKE_EVIDENCE=(.+)$/m)?.[1] ?? "null",
);
assert.equal(smokeEvidence.unsafeRetryDecision, "operation_retry_safety_unknown");
assert.equal(smokeEvidence.unsafeRetryAttempts, 1);
assert.equal(smokeEvidence.systemPlaneWithheld, true);
assert.equal(smokeEvidence.runtimeConsoleWithheld, true);
assert.ok(smokeEvidence.successfulStorySegments >= 4);

const scenarios = [
  {
    scenarioId: "deadline-timeout",
    outcome: "deadline_exceeded",
    retryReason: "deadline_exhausted",
  },
  {
    scenarioId: "support-ticket-api-crash",
    outcome: "workload_crashed",
    retryReason: "unsafe_operation",
  },
  {
    scenarioId: "support-ticket-api-partial-unavailability",
    outcome: "partial_unavailability_observed",
    retryReason: "unsafe_operation",
  },
];
const results = [];

for (const expected of scenarios) {
  const result = await runScenario(expected.scenarioId);
  const repeated = await runScenario(expected.scenarioId);
  assert.deepEqual(repeated, result);
  assert.equal(result.artifactVersion, "lenso.failure-scenario-result.v1");
  assert.equal(result.scenarioId, expected.scenarioId);
  assert.equal(result.outcome, expected.outcome);
  assert.equal(result.attempts, 1);
  assert.equal(result.retryDecision.attempted, false);
  assert.equal(result.retryDecision.reason, expected.retryReason);
  assert.equal(result.cleanup.completed, true);
  assert.equal(result.cleanup.sandboxStateRemoved, true);
  await assertMissing(path.dirname(sandboxState), `Sandbox state remained after ${expected.scenarioId}`);

  const storyPath = path.join(
    fixtureRoot,
    ".lenso",
    "system-sandbox-results",
    "support-platform",
    expected.scenarioId,
    "story-segment.json",
  );
  const story = JSON.parse(await readFile(storyPath, "utf8"));
  assert.equal(story.artifactVersion, "lenso.story-segment.v1");
  assert.deepEqual(story.result, result);
  results.push(result);
}

await run("pnpm", ["smoke:support-ticket"], repoRoot);

console.log(
  JSON.stringify(
    {
      artifactVersion: "lenso.m1-developer-preview-acceptance.v1",
      outcome: "passed",
      directServiceCalls: {
        generatedClient: true,
        systemPlaneWithheld: smokeEvidence.systemPlaneWithheld,
        runtimeConsoleWithheld: smokeEvidence.runtimeConsoleWithheld,
        successfulStorySegments: smokeEvidence.successfulStorySegments,
        unsafeRetryDecision: smokeEvidence.unsafeRetryDecision,
        unsafeRetryAttempts: smokeEvidence.unsafeRetryAttempts,
      },
      scenarios: results.map(({ scenarioId, outcome, attempts, retryDecision, cleanup }) => ({
        scenarioId,
        outcome,
        attempts,
        retryDecision,
        cleanup,
      })),
      providerSmoke: "passed",
    },
    null,
    2,
  ),
);

function run(command, args, cwd, capture = false) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: { ...process.env, LENSO_CLI_BIN: cli },
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

async function runScenario(scenarioId) {
  const output = await run(
    cli,
    ["system", "dev", "--scenario", scenarioId, "--json"],
    fixtureRoot,
    true,
  );
  return JSON.parse(output);
}

async function assertMissing(target, message) {
  await access(target).then(
    () => {
      throw new Error(message);
    },
    () => {},
  );
}
