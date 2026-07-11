#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFile, lstat, mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";

const canonical = (value) => {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
};
const digest = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
const url = new URL(process.env.INTEGRATION_SET_URL ?? "");
const expectedDigest = process.env.INTEGRATION_SET_SHA256 ?? "";
assert.match(expectedDigest, /^sha256:[0-9a-f]{64}$/u);
assert.equal(url.origin, "https://raw.githubusercontent.com");
assert.match(url.pathname, /^\/LioRael\/lenso-release\/[0-9a-f]{40}\/.+\.json$/u, "integration set URL must be commit-addressed");
const response = await fetch(url, { redirect: "error" });
assert.equal(response.ok, true, `integration set download failed: ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
assert.equal(digest(bytes), expectedDigest, "integration set bytes do not match the reviewed digest");
const integration = JSON.parse(bytes.toString("utf8"));
assert.equal(integration.schema, "lenso.integration-set.v1");
assert.match(integration.baseSystemVersion, /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/u);
const repositories = integration.repositories;
assert.ok(repositories && Object.keys(repositories).length > 0);
for (const [repository, commit] of Object.entries(repositories)) {
  assert.match(repository, /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u);
  assert.match(commit, /^[0-9a-f]{40}$/u);
}
const identity = { schema: integration.schema, baseSystemVersion: integration.baseSystemVersion, repositories };
assert.equal(integration.integrationSetId, digest(Buffer.from(canonical(identity))), "integration set ID is invalid");
const runtimeCommit = repositories["LioRael/lenso-runtime-console"];
assert.match(runtimeCommit ?? "", /^[0-9a-f]{40}$/u, "integration set must pin Runtime Console");

const root = path.resolve(import.meta.dirname, "..");
const runtime = path.resolve(root, "../lenso-runtime-console");
try {
  assert.equal((await lstat(runtime)).isSymbolicLink(), false, "refusing to replace a linked Runtime Console path");
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
await rm(runtime, { recursive: true, force: true });
await mkdir(runtime, { recursive: true });
execFileSync("git", ["init"], { cwd: runtime, stdio: "inherit" });
execFileSync("git", ["fetch", "--depth=1", "https://github.com/LioRael/lenso-runtime-console.git", runtimeCommit], { cwd: runtime, stdio: "inherit" });
execFileSync("git", ["checkout", "--detach", "FETCH_HEAD"], { cwd: runtime, stdio: "inherit" });
assert.equal(execFileSync("git", ["rev-parse", "HEAD"], { cwd: runtime, encoding: "utf8" }).trim(), runtimeCommit);
await appendFile(path.join(root, "pnpm-workspace.yaml"), "\noverrides:\n  '@lenso/remote-module-kit': link:../lenso-runtime-console/packages/remote-module-kit\n  '@lenso/service-kit': link:../lenso-runtime-console/packages/service-kit\n");
if (process.env.GITHUB_STEP_SUMMARY) await writeFile(process.env.GITHUB_STEP_SUMMARY, `Integration set: ${integration.integrationSetId}\nDigest: ${expectedDigest}\n`, { flag: "a" });
