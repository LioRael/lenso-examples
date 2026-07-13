import { spawn } from "node:child_process";
import { access, readFile, rm } from "node:fs/promises";
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

await run("cargo", ["build", "--manifest-path", "Cargo.toml", "--bins"], fixtureRoot);
await rm(path.dirname(sandboxState), { recursive: true, force: true });

const sandbox = spawn(cli, ["system", "dev"], {
  cwd: fixtureRoot,
  env: process.env,
  stdio: ["ignore", "pipe", "pipe"],
});
let output = "";
let smokeEvidence;
sandbox.stdout.on("data", (chunk) => {
  output += chunk;
  process.stdout.write(chunk);
});
sandbox.stderr.on("data", (chunk) => {
  output += chunk;
  process.stderr.write(chunk);
});

try {
  await waitForReady();
  await assertSandboxState();
  const smokeOutput = await run("./target/debug/support-system-smoke", [], fixtureRoot, true);
  process.stdout.write(smokeOutput);
  smokeEvidence = JSON.parse(smokeOutput);
  if (
    smokeEvidence.unsafeRetryDecision !== "operation_retry_safety_unknown" ||
    smokeEvidence.unsafeRetryAttempts !== 1
  ) {
    throw new Error("unsafe transient failure did not prove one-attempt retry suppression");
  }
  if (
    !smokeEvidence.systemPlaneWithheld ||
    !smokeEvidence.runtimeConsoleWithheld ||
    smokeEvidence.successfulStorySegments < 4
  ) {
    throw new Error("plane-independent call or successful Story Segment evidence is incomplete");
  }
} finally {
  if (sandbox.exitCode === null) {
    sandbox.kill("SIGINT");
  }
  await waitForExit(sandbox);
}

if (sandbox.exitCode !== 0) {
  throw new Error(`System Sandbox exited with ${sandbox.exitCode}\n${output}`);
}
await access(path.dirname(sandboxState)).then(
  () => {
    throw new Error("System Sandbox did not clean its owned state");
  },
  () => {},
);
console.log("Support System direct-Service smoke passed");
console.log(`M1_SMOKE_EVIDENCE=${JSON.stringify(smokeEvidence)}`);

async function assertSandboxState() {
  const state = JSON.parse(await readFile(sandboxState, "utf8"));
  if (state.phase !== "ready") {
    throw new Error(`unexpected Sandbox phase: ${state.phase}`);
  }
  const identities = state.workloads.map(({ identity }) => identity);
  if (
    !identities.every((identity) =>
      identity.startsWith("local-dev://support-platform/"),
    )
  ) {
    throw new Error("Sandbox did not expose development Workload Identity");
  }
  if (state.endpoints.length !== 2) {
    throw new Error("Sandbox did not publish both Service endpoints");
  }
  if (
    state.workloads.length !== 6 ||
    state.workloads.some(
      ({ serviceId }) =>
        serviceId !== "support-ticket-service" && serviceId !== "support-sla-service",
    )
  ) {
    throw new Error("A Host, Provider, Runtime Console, or other process entered the Data Plane");
  }
}

async function waitForReady() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (output.includes("System Sandbox support-platform: ready")) return;
    if (sandbox.exitCode !== null) {
      throw new Error(`System Sandbox exited before readiness\n${output}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for System Sandbox\n${output}`);
}

function run(command, args, cwd, capture = false) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: process.env,
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

function waitForExit(child) {
  if (child.exitCode !== null) return Promise.resolve();
  return new Promise((resolve) => child.once("exit", resolve));
}
