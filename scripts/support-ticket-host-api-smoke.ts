#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { serveSupportTicketModule } from "../examples/support-ticket/src/module.ts";

const repoRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const lensoCliManifest = path.resolve(repoRoot, "../lenso-cli/Cargo.toml");
const token =
  "dev-service:admin:runtime.stories.read,support_ticket.tickets.read,support_ticket.tickets.write,support_ticket.tickets.escalate";
const hostReadyTimeoutMs = Number(
  process.env.LENSO_HOST_API_SMOKE_READY_TIMEOUT_MS ?? "240000"
);

const assertEqual = (actual, expected, message) => {
  if (actual !== expected) {
    throw new Error(
      `${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(
        actual
      )}`
    );
  }
};

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

const lensoCommand = () => {
  if (process.env.LENSO_CLI) {
    return { args: [], command: process.env.LENSO_CLI };
  }
  return {
    args: ["run", "--quiet", "--manifest-path", lensoCliManifest, "--"],
    command: "cargo",
  };
};

const run = ({ args, cwd, env = {} }) =>
  new Promise((resolve, reject) => {
    const child = spawn(args[0], args.slice(1), {
      cwd,
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += String(chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve({ stderr, stdout });
        return;
      }
      reject(
        new Error(
          [`${args.join(" ")} failed with exit code ${code}`, stdout, stderr]
            .map((part) => part.trim())
            .filter(Boolean)
            .join("\n")
        )
      );
    });
  });

const runLenso = ({ args, cwd, env }) => {
  const command = lensoCommand();
  return run({ args: [command.command, ...command.args, ...args], cwd, env });
};

const spawnLenso = ({ args, cwd, env }) => {
  const command = lensoCommand();
  return spawn(command.command, [...command.args, ...args], {
    cwd,
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
};

const freePort = () =>
  new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => resolve(address.port));
    });
  });

const fetchJson = async (url, init = {}) => {
  const response = await fetch(url, {
    ...init,
    headers: {
      authorization: `Bearer ${token}`,
      ...(init.headers ?? {}),
    },
  });
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}: ${await response.text()}`);
  }
  return response.json();
};

const waitFor = async (description, fn, { timeoutMs = 90_000 } = {}) => {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const result = await fn();
      if (result) {
        return result;
      }
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(
    `${description} timed out${lastError ? `: ${lastError.message}` : ""}`
  );
};

const tail = (value, maxLength = 8000) =>
  value.length > maxLength ? value.slice(-maxLength) : value;

const stopChild = (child) =>
  new Promise((resolve) => {
    if (!child || child.exitCode !== null) {
      resolve();
      return;
    }
    child.once("close", resolve);
    child.kill("SIGINT");
    setTimeout(() => {
      if (child.exitCode === null) {
        child.kill("SIGKILL");
      }
    }, 5000).unref();
  });

const hostRoot = await mkdtemp(path.join(tmpdir(), "lenso-host-api-smoke-"));
const keepHostRoot =
  process.env.LENSO_KEEP_HOST_SMOKE === "1" ||
  process.env.LENSO_KEEP_HOST_SMOKE === "true";
const composeProjectName = `lenso_host_api_smoke_${process.pid}`;
const supportServer = await serveSupportTicketModule({ port: 0 });
let hostProcess;
let passed = false;

try {
  const httpPort = await freePort();
  const postgresPort = await freePort();
  // ponytail: free-port probes can race; expose env overrides if this ever flakes.
  const hostEnv = {
    COMPOSE_PROJECT_NAME: composeProjectName,
    HTTP_PORT: String(httpPort),
    POSTGRES_HOST_PORT: String(postgresPort),
  };

  await runLenso({
    args: ["host", "init", hostRoot, "--name", "support-ticket-smoke"],
    cwd: repoRoot,
  });

  const envExample = await readFile(path.join(hostRoot, ".env.example"), "utf8");
  await writeFile(
    path.join(hostRoot, ".env"),
    envExample
      .replace("POSTGRES_HOST_PORT=5432", `POSTGRES_HOST_PORT=${postgresPort}`)
      .replace("HTTP_PORT=3000", `HTTP_PORT=${httpPort}`)
  );

  await runLenso({
    args: ["module", "install", supportServer.manifestUrl, "--repo-root", hostRoot],
    cwd: hostRoot,
  });

  hostProcess = spawnLenso({
    args: ["serve", "--repo-root", hostRoot],
    cwd: hostRoot,
    env: hostEnv,
  });
  let hostLog = "";
  hostProcess.stdout.on("data", (chunk) => {
    hostLog += String(chunk);
  });
  hostProcess.stderr.on("data", (chunk) => {
    hostLog += String(chunk);
  });
  hostProcess.on("exit", (code) => {
    if (!passed && code !== null && code !== 0) {
      console.error(hostLog.trim());
    }
  });

  const apiBaseUrl = `http://127.0.0.1:${httpPort}`;
  await waitFor(
    "host readiness",
    async () => {
      if (hostProcess.exitCode !== null || hostProcess.signalCode) {
        throw new Error(`host exited before ready\n${tail(hostLog)}`);
      }
      const response = await fetch(`${apiBaseUrl}/readyz`);
      return response.ok;
    },
    { timeoutMs: hostReadyTimeoutMs }
  ).catch((error) => {
    throw new Error(`${error.message}\nHost log:\n${tail(hostLog)}`);
  });

  const modules = await fetchJson(`${apiBaseUrl}/admin/data/modules`);
  const supportTicket = modules.modules?.find(
    (module) => module.module_name === "support-ticket"
  );
  assert(supportTicket, "support-ticket module was not listed");
  assertEqual(supportTicket.status, "loaded", "support-ticket load status");
  assertEqual(
    supportTicket.governance?.activation_state,
    "active",
    "support-ticket activation state"
  );
  assert(
    supportTicket.manifest_lints?.every((lint) => lint.severity === "ok"),
    "support-ticket manifest lints were not all ok"
  );

  const list = await fetchJson(
    `${apiBaseUrl}/admin/data/support-ticket/tickets?limit=5`
  );
  assertEqual(list.data?.[0]?.id, "ticket_1", "admin data first ticket");

  const created = await fetchJson(`${apiBaseUrl}/modules/support-ticket/http/tickets`, {
    body: JSON.stringify({
      assignee: "api-smoke",
      priority: "high",
      title: "Smoke-created ticket",
    }),
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  assertEqual(created.data?.ticket?.id, "ticket_2", "proxy-created ticket id");

  await waitFor("support-ticket runtime story", async () => {
    const stories = await fetchJson(`${apiBaseUrl}/admin/runtime/stories?limit=5`);
    return stories.data?.some(
      (story) =>
        story.title === "Support ticket created" &&
        story.status === "completed" &&
        story.services?.includes("support-ticket")
    );
  });

  passed = true;
  console.log("support-ticket host API smoke passed");
} finally {
  await stopChild(hostProcess);
  await supportServer.close();
  await run({
    args: ["docker", "compose", "down", "-v"],
    cwd: hostRoot,
    env: { COMPOSE_PROJECT_NAME: composeProjectName },
  }).catch(() => {});
  if (keepHostRoot || !passed) {
    console.error(`Host API smoke temp repo kept at ${hostRoot}`);
  } else {
    await rm(hostRoot, { force: true, recursive: true });
  }
}
