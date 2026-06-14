#!/usr/bin/env node
import { spawn } from "node:child_process";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { serveHelloActionModule } from "./module.mjs";

const repoRoot = path.resolve(
  fileURLToPath(new URL("../../..", import.meta.url))
);
const runtimeConsoleRoot = path.resolve(
  process.env.LENSO_RUNTIME_CONSOLE_DIR ??
    path.join(repoRoot, "../lenso-runtime-console")
);
const pnpmCommand = process.platform === "win32" ? "pnpm.cmd" : "pnpm";

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

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

const ensureRuntimeConsoleCli = async () => {
  const packageJsonPath = path.join(runtimeConsoleRoot, "package.json");
  const cliBuildPath = path.join(
    runtimeConsoleRoot,
    "packages/console-package-cli/dist/index.mjs"
  );
  try {
    await access(packageJsonPath);
    await access(cliBuildPath);
  } catch {
    throw new Error(
      [
        `Runtime Console CLI is not ready at ${runtimeConsoleRoot}.`,
        "Set LENSO_RUNTIME_CONSOLE_DIR to a built lenso-runtime-console checkout,",
        "or run `pnpm --dir ../lenso-runtime-console build:local` first.",
      ].join(" ")
    );
  }
};

const runLenso = ({ args, cwd }) =>
  new Promise((resolve, reject) => {
    const childProcess = spawn(
      pnpmCommand,
      [
        "--pm-on-fail=ignore",
        "--dir",
        runtimeConsoleRoot,
        "exec",
        "lenso",
        ...args,
      ],
      {
        cwd,
        env: process.env,
        stdio: ["ignore", "pipe", "pipe"],
      }
    );
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
const server = await serveHelloActionModule({ port: 0 });
let passed = false;

try {
  await ensureRuntimeConsoleCli();

  const manifestUrl = server.manifestUrl;
  const catalogSummary = "Hello Action starter module";
  const catalogResult = await runLenso({
    args: [
      "module",
      "catalog",
      "add",
      manifestUrl,
      "--repo-root",
      hostRoot,
      "--summary",
      catalogSummary,
    ],
    cwd: hostRoot,
  });
  assert(
    catalogResult.stdout.includes("Added hello-action to module catalog."),
    "catalog add did not report hello-action"
  );

  const catalog = await readJson(
    path.join(hostRoot, ".lenso/module-catalog.json")
  );
  assertEqual(catalog.version, 1, "catalog version mismatch");
  assertEqual(catalog.modules?.length, 1, "catalog module count mismatch");
  const catalogModule = catalog.modules[0];
  assertEqual(catalogModule.name, "hello-action", "catalog module name");
  assertEqual(catalogModule.source, "remote", "catalog module source");
  assertEqual(catalogModule.version, "0.1.0", "catalog module version");
  assertEqual(
    catalogModule.manifestReference,
    manifestUrl,
    "catalog manifest reference"
  );
  assertEqual(catalogModule.baseUrl, server.baseUrl, "catalog base URL");
  assertEqual(catalogModule.summary, catalogSummary, "catalog summary");
  assertEqual(
    catalogModule.consolePackages?.length,
    0,
    "catalog console package count"
  );

  const addResult = await runLenso({
    args: ["module", "add", manifestUrl, "--repo-root", hostRoot],
    cwd: hostRoot,
  });
  assert(
    addResult.stdout.includes("Added remote module hello-action."),
    "module add did not report hello-action"
  );
  assert(
    addResult.stdout.includes("Console packages: 0"),
    "module add did not report zero console packages"
  );
  assert(
    !addResult.stdout.includes("lenso console-package apply-plan"),
    "module add should not request apply-plan for zero console packages"
  );
  assert(
    !addResult.stdout.includes("pnpm install"),
    "module add should not request pnpm install for zero console packages"
  );

  const envFile = await readFile(path.join(hostRoot, ".env"), "utf8");
  assertEqual(
    envFile,
    `REMOTE_MODULES=hello-action=${server.baseUrl}\n`,
    "REMOTE_MODULES env"
  );

  const installPlan = await readJson(
    path.join(hostRoot, ".lenso/console-package-install-plan.json")
  );
  assertEqual(installPlan.version, 1, "install plan version");
  assertEqual(installPlan.modules?.length, 1, "install plan module count");
  const modulePlan = installPlan.modules[0];
  assertEqual(modulePlan.moduleName, "hello-action", "install plan module");
  assertEqual(modulePlan.baseUrl, server.baseUrl, "install plan base URL");
  assertEqual(
    modulePlan.manifestReference,
    manifestUrl,
    "install plan manifest reference"
  );
  assertEqual(modulePlan.restartRequired, true, "install plan restart flag");
  assertEqual(
    modulePlan.consolePackages?.length,
    0,
    "install plan console package count"
  );

  passed = true;
  console.log("Hello Action host install smoke passed");
} finally {
  await server.close();
  if (hostRoot && (keepHostRoot || !passed)) {
    console.error(`Host smoke temp repo kept at ${hostRoot}`);
  } else {
    await rm(hostRoot, { force: true, recursive: true });
  }
}
