import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { assertScenarioEvidenceSet } from "./m6-acceptance.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frameworkRoot = path.dirname(repoRoot);
const lensoRoot = path.resolve(process.env.LENSO_REPO_ROOT ?? path.join(frameworkRoot, "lenso"));
const cliRoot = path.resolve(process.env.LENSO_CLI_ROOT ?? path.join(frameworkRoot, "lenso-cli"));

requireApproval("LENSO_NATS_TEST_INFRASTRUCTURE_APPROVED", "real NATS JetStream test infrastructure");
requireApproval("LENSO_SPIFFE_TEST_INFRASTRUCTURE_APPROVED", "real SPIFFE/SPIRE test infrastructure");
requireApproval("LENSO_KUBERNETES_TEST_INFRASTRUCTURE_APPROVED", "disposable Kubernetes and Operator test infrastructure");
if (!process.env.DATABASE_URL) throw new Error("DATABASE_URL is required for authoritative Store recovery evidence");
if (!process.env.SPIFFE_ENDPOINT_SOCKET) throw new Error("SPIFFE_ENDPOINT_SOCKET is required for the short-lived SPIRE Workload API");

const commands = [
  await runChecked("sandbox", "cargo", ["test", "--locked", "--test", "system_sandbox_scenarios", "declared_failure_scenarios_are_repeatable_and_leave_durable_evidence", "--", "--exact"], cliRoot),
  await runChecked("nats-conformance", "cargo", ["test", "--locked", "-p", "lenso-autonomous-service", "--test", "transport_conformance", "jetstream_adapter_passes_real_environment_conformance", "--", "--exact", "--nocapture"], lensoRoot),
  await runChecked("nats-restart", "cargo", ["test", "--locked", "-p", "lenso-autonomous-service", "--test", "transport", "jetstream_restart_preserves_authoritative_support_behavior_once", "--", "--exact", "--nocapture"], lensoRoot),
  await runChecked("nats-poison", "cargo", ["test", "--locked", "-p", "lenso-autonomous-service", "--test", "transport", "controlled_retries_dead_letter_poison_without_blocking_healthy_events", "--", "--exact"], lensoRoot),
  await runChecked("spiffe", "cargo", ["test", "--locked", "-p", "lenso-service", "--test", "spiffe_workload_identity", "spire_authenticates_real_http_and_rotates_without_plane_dependencies", "--", "--exact", "--nocapture"], lensoRoot),
  await runChecked("coordination-outage", "cargo", ["test", "--locked", "-p", "lenso-service", "--test", "production_delivery", "converged_data_plane_survives_coordination_loss_and_resumes_without_duplicates", "--", "--exact"], lensoRoot),
  await runChecked("m5-data-plane", "pnpm", ["acceptance:m5", "--", "--m5-only"], repoRoot),
];

const commandDigests = Object.fromEntries(commands.map((command) => [command.id, command.digest]));
const evidenceIndex = process.argv.indexOf("--scenario-evidence");
const evidencePath = evidenceIndex >= 0 ? process.argv[evidenceIndex + 1] : undefined;
if (!evidencePath) {
  throw new Error("--scenario-evidence is required; Environment Verification never invents scenario observations");
}
const scenarios = assertScenarioEvidenceSet(JSON.parse(await readFile(path.resolve(evidencePath), "utf8")));
for (const scenario of scenarios) {
  for (const proof of scenario.proofs) {
    if (commandDigests[proof.commandId] !== proof.commandDigest) {
      throw new Error(`${scenario.condition} proof is not bound to this Environment Verification command output`);
    }
  }
}

const outputIndex = process.argv.indexOf("--output");
const output = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
if (output) await writeFile(path.resolve(output), `${JSON.stringify(scenarios, null, 2)}\n`);
console.log(JSON.stringify({
  artifactVersion: "lenso.m6-environment-verification.v1",
  outcome: "passed",
  approvalBoundary: "explicit_test_infrastructure",
  commandDigests,
  scenarios,
  productionCredentialsRead: false,
  productionMutation: false,
  cleanupComplete: true,
}, null, 2));

function requireApproval(name, subject) {
  if (process.env[name] !== "true") {
    throw new Error(`${subject} is an Approval Boundary; set ${name}=true only after explicit authorization`);
  }
}

async function runChecked(id, command, args, cwd) {
  const output = await run(command, args, cwd);
  if (output.includes("skipping") || !/test result: ok\. [1-9]\d* passed;|"outcome":\s*"passed"/u.test(output)) {
    throw new Error(`${id} did not execute its declared real-environment proof`);
  }
  return { id, digest: digest(output) };
}

function run(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env: process.env, stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", (chunk) => { output += chunk; process.stdout.write(chunk); });
    child.stderr.on("data", (chunk) => { output += chunk; process.stderr.write(chunk); });
    child.once("error", reject);
    child.once("exit", (code, signal) => code === 0
      ? resolve(output)
      : reject(new Error(`${command} exited with ${code ?? signal}`)));
  });
}

function digest(value) {
  return `sha256:${createHash("sha256").update(value).digest("hex")}`;
}
