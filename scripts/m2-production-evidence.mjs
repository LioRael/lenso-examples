import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const lensoRepo = path.resolve(
  repoRoot,
  process.env.LENSO_REPO ?? "../lenso",
);

requireApproval(
  "LENSO_NATS_TEST_INFRASTRUCTURE_APPROVED",
  "real NATS JetStream infrastructure",
);
requireApproval(
  "LENSO_SPIFFE_TEST_INFRASTRUCTURE_APPROVED",
  "real SPIFFE/SPIRE infrastructure",
);
if (!process.env.DATABASE_URL) {
  throw new Error(
    "M2 production evidence requires DATABASE_URL for durable NATS diagnostics",
  );
}
if (!process.env.SPIFFE_ENDPOINT_SOCKET) {
  throw new Error(
    "M2 production evidence requires SPIFFE_ENDPOINT_SOCKET for the Workload API",
  );
}

const natsEvidence = await run("cargo", [
  "test",
  "--locked",
  "-p",
  "lenso-autonomous-service",
  "--test",
  "transport_conformance",
  "jetstream_adapter_passes_real_environment_conformance",
  "--",
  "--exact",
]);
requireExecuted(natsEvidence, "NATS JetStream");
const natsBusinessEvidence = await run("cargo", [
  "test",
  "--manifest-path",
  "examples/support-system/Cargo.toml",
  "--locked",
  "--lib",
  "m2::production_tests::nats_jetstream_runs_same_support_module_behavior",
  "--",
  "--exact",
  "--nocapture",
], repoRoot);
requireExecuted(natsBusinessEvidence, "NATS support Module behavior");
const spiffeEvidence = await run("cargo", [
  "test",
  "--locked",
  "-p",
  "lenso-service",
  "--test",
  "spiffe_workload_identity",
  "spire_authenticates_real_http_and_rotates_without_plane_dependencies",
  "--",
  "--exact",
]);
requireExecuted(spiffeEvidence, "SPIFFE/SPIRE");

console.log(
  JSON.stringify(
    {
      artifactVersion: "lenso.m2-production-evidence.v1",
      outcome: "passed",
      approvalBoundary: "explicit_real_environment",
      transport: "nats_jetstream_real_environment",
      workloadIdentity: "spiffe_spire_real_environment",
      providerSemantics: "separate",
    },
    null,
    2,
  ),
);

function requireApproval(name, infrastructure) {
  if (process.env[name] !== "true") {
    throw new Error(
      `${infrastructure} is an Approval Boundary; set ${name}=true only after explicit authorization`,
    );
  }
}

function requireExecuted(output, integration) {
  if (output.includes("skipping") || !/test result: ok\. 1 passed;/.test(output)) {
    throw new Error(
      `${integration} did not produce one executed real-environment test result`,
    );
  }
}

function run(command, args, cwd = lensoRepo) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
      process.stdout.write(chunk);
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
      process.stderr.write(chunk);
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve(output);
      else reject(new Error(`${command} exited with ${code ?? signal}`));
    });
  });
}
