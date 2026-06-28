#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { serveAccountProfileModule } from "../examples/account-profile/src/module.ts";

const repoRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const lensoCliManifest = path.resolve(repoRoot, "../lenso-cli/Cargo.toml");
const token =
  "dev-service:admin:runtime.stories.read,account_profile.profiles.read,account_profile.profiles.write,account_profile.organizations.read,account_profile.organizations.write";
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
const composeProjectName = `lenso_account_profile_smoke_${process.pid}`;
const accountProfileServer = await serveAccountProfileModule({ port: 0 });
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
    args: ["host", "init", hostRoot, "--name", "account-profile-smoke"],
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
    args: ["module", "install", "auth", "--repo-root", hostRoot],
    cwd: hostRoot,
  });
  await runLenso({
    args: [
      "service",
      "install",
      accountProfileServer.manifestUrl,
      "--repo-root",
      hostRoot,
    ],
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
  const accountProfile = modules.modules?.find(
    (module) => module.module_name === "account-profile"
  );
  assert(accountProfile, "account-profile module was not listed");
  assertEqual(accountProfile.status, "loaded", "account-profile load status");
  assertEqual(
    accountProfile.governance?.activation_state,
    "active",
    "account-profile activation state"
  );
  assert(
    accountProfile.dependencies?.includes("auth"),
    "account-profile auth dependency was not listed"
  );
  assert(
    accountProfile.manifest_lints?.every((lint) => lint.severity === "ok"),
    "account-profile manifest lints were not all ok"
  );

  const list = await fetchJson(
    `${apiBaseUrl}/admin/data/account-profile/profiles?limit=5`
  );
  assertEqual(list.data?.[0]?.id, "profile_auth_user_1", "admin data first profile");

  const created = await fetchJson(
    `${apiBaseUrl}/modules/account-profile/http/profiles`,
    {
      body: JSON.stringify({
        auth_user_id: "auth_user_2",
        contact_email: "sam@example.com",
        display_name: "Sam Ops",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  assertEqual(
    created.data?.profile?.id,
    "profile_auth_user_2",
    "proxy-created profile id"
  );

  const membership = await fetchJson(
    `${apiBaseUrl}/modules/account-profile/http/organizations/org_1/memberships`,
    {
      body: JSON.stringify({
        profile_id: "profile_auth_user_2",
        role: "admin",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  assertEqual(
    membership.data?.membership?.role,
    "admin",
    "proxy-created membership role"
  );

  await waitFor("account-profile runtime story", async () => {
    const stories = await fetchJson(`${apiBaseUrl}/admin/runtime/stories?limit=5`);
    return stories.data?.some(
      (story) =>
        story.title === "Organization membership added" &&
        story.status === "completed" &&
        story.services?.includes("account-profile")
    );
  });

  passed = true;
  console.log("account-profile host API smoke passed");
} finally {
  await stopChild(hostProcess);
  await accountProfileServer.close();
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
