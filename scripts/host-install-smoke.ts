#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { serveHelloActionModule } from "../examples/hello-action/src/module.ts";
import { serveAccountProfileModule } from "../examples/account-profile/src/module.ts";
import { serveSupportTicketModule } from "../examples/support-ticket/src/module.ts";

const repoRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const lensoCliManifest = path.resolve(repoRoot, "../lenso-cli/Cargo.toml");

const examples = {
  "account-profile": {
    catalogSummary: "Account Profile auth boundary module",
    serve: serveAccountProfileModule,
  },
  "hello-action": {
    catalogSummary: "Hello Action starter module",
    serve: serveHelloActionModule,
  },
  "support-ticket": {
    catalogSummary: "Support Ticket agent-ready module",
    serve: serveSupportTicketModule,
  },
};

const exampleName = process.argv[2];
const example = examples[exampleName];

if (!example) {
  throw new Error(
    `Usage: node scripts/host-install-smoke.ts ${Object.keys(examples).join("|")}`
  );
}

const assertEqual = (actual, expected, message) => {
  if (actual !== expected) {
    throw new Error(
      `${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(
        actual
      )}`
    );
  }
};

const readJson = async (filePath) =>
  JSON.parse(await readFile(filePath, "utf8"));

const runLenso = ({ args, cwd }) =>
  new Promise((resolve, reject) => {
    const command = process.env.LENSO_CLI ?? "cargo";
    const commandArgs = process.env.LENSO_CLI
      ? args
      : [
          "run",
          "--quiet",
          "--manifest-path",
          lensoCliManifest,
          "--",
          ...args,
        ];
    const childProcess = spawn(command, commandArgs, {
      cwd,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";

    childProcess.stdout.on("data", (chunk) => {
      stdout += String(chunk);
    });
    childProcess.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });
    childProcess.on("error", reject);
    childProcess.on("close", (code) => {
      if (code === 0) {
        resolve({ stderr, stdout });
        return;
      }
      reject(
        new Error(
          [
            `lenso ${args.join(" ")} failed with exit code ${code}`,
            stdout.trim(),
            stderr.trim(),
          ]
            .filter(Boolean)
            .join("\n")
        )
      );
    });
  });

const hostRoot = await mkdtemp(path.join(tmpdir(), "lenso-host-smoke-"));
const keepHostRoot =
  process.env.LENSO_KEEP_HOST_SMOKE === "1" ||
  process.env.LENSO_KEEP_HOST_SMOKE === "true";
const server = await example.serve({ port: 0 });
let passed = false;

try {
  const manifestUrl = server.manifestUrl;
  await runLenso({
    args: [
      "module",
      "catalog",
      "add",
      manifestUrl,
      "--repo-root",
      hostRoot,
      "--summary",
      example.catalogSummary,
    ],
    cwd: hostRoot,
  });

  const catalog = await readJson(
    path.join(hostRoot, ".lenso/module-catalog.json")
  );
  assertEqual(catalog.version, 1, "catalog version");
  assertEqual(catalog.modules?.length, 1, "catalog module count");
  assertEqual(catalog.modules[0]?.name, exampleName, "catalog module name");
  assertEqual(catalog.modules[0]?.source, "remote", "catalog source");
  assertEqual(catalog.modules[0]?.baseUrl, server.baseUrl, "catalog base URL");

  await runLenso({
    args: ["module", "install", manifestUrl, "--repo-root", hostRoot],
    cwd: hostRoot,
  });

  const envFile = await readFile(path.join(hostRoot, ".env"), "utf8");
  assertEqual(envFile, `REMOTE_MODULES=${exampleName}=${server.baseUrl}\n`, ".env");

  const installReceipt = await readJson(
    path.join(hostRoot, ".lenso/module-installs.json")
  );
  assertEqual(installReceipt.version, 1, "install receipt version");
  assertEqual(
    installReceipt.modules?.[0]?.moduleName,
    exampleName,
    "install receipt module"
  );
  assertEqual(
    installReceipt.modules?.[0]?.baseUrl,
    server.baseUrl,
    "install receipt base URL"
  );

  passed = true;
  console.log(`${exampleName} host install smoke passed`);
} finally {
  await server.close();
  if (hostRoot && (keepHostRoot || !passed)) {
    console.error(`Host smoke temp repo kept at ${hostRoot}`);
  } else {
    await rm(hostRoot, { force: true, recursive: true });
  }
}
