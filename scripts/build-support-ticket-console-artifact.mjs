/* eslint-disable func-style, sort-keys, unicorn/no-array-sort */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import react from "@vitejs/plugin-react";
import { build } from "vite";

const createTemporaryRoot = async (moduleId) => {
  const safeId = moduleId.replaceAll(/[^a-z0-9_-]+/giu, "-");
  const path = join(tmpdir(), `lenso-console-module-${safeId}-${process.pid}`);
  await rm(path, { force: true, recursive: true });
  await mkdir(path, { recursive: true });
  return path;
};

const isDigest = (value) =>
  typeof value === "string" && /^sha256:[a-f0-9]{64}$/u.test(value);

const root = resolve(import.meta.dirname, "..");
const outputRoot = resolve(
  process.env.LENSO_SUPPORT_TICKET_CONSOLE_ARTIFACT_DIR ??
    join(root, "dist", "support-ticket-console-artifact")
);
const releaseDigest = process.env.LENSO_SUPPORT_TICKET_MODULE_RELEASE_DIGEST;
const locatorBase = process.env.LENSO_CONSOLE_MODULE_ARTIFACT_BASE_URL;

async function listFiles(directory, prefix = "") {
  const files = [];
  for (const entry of await readdir(join(directory, prefix), {
    withFileTypes: true,
  })) {
    const relative = prefix ? join(prefix, entry.name) : entry.name;
    if (entry.isDirectory()) {
      files.push(...(await listFiles(directory, relative)));
    } else if (entry.isFile()) {
      files.push(relative.replaceAll("\\", "/"));
    }
  }
  return files;
}

const module = {
  entry: "examples/support-ticket-console/src/index.tsx",
  id: "support/tickets",
  manifest: "examples/support-ticket-console/console-module.json",
};

if (!isDigest(releaseDigest)) {
  throw new Error(
    "LENSO_SUPPORT_TICKET_MODULE_RELEASE_DIGEST must be a sha256:<64 hex> digest"
  );
}

await rm(outputRoot, { force: true, recursive: true });
await mkdir(outputRoot, { recursive: true });

const manifest = JSON.parse(
  await readFile(join(root, module.manifest), "utf-8")
);
if (manifest.moduleId !== module.id) {
  throw new Error(`manifest identity mismatch for ${module.id}`);
}
const temporaryRoot = await createTemporaryRoot(module.id);
const packageRoot = join(temporaryRoot, "package");
let artifact;
try {
  await build({
    build: {
      emptyOutDir: true,
      lib: {
        entry: join(root, module.entry),
        fileName: () => "index.js",
        formats: ["es"],
      },
      outDir: join(packageRoot, "dist"),
      rolldownOptions: {
        output: {
          assetFileNames: "assets/[name][extname]",
          chunkFileNames: "chunks/[name]-[hash].js",
          entryFileNames: "index.js",
        },
      },
    },
    configFile: false,
    plugins: [react()],
    publicDir: false,
    resolve: {
      alias: [
        {
          find: /^react\/jsx-runtime$/u,
          replacement: join(root, "scripts", "console-react-runtime-shim.mjs"),
        },
        {
          find: /^react$/u,
          replacement: join(root, "scripts", "console-react-runtime-shim.mjs"),
        },
      ],
    },
    root,
  });

  const archiveName = `${module.id.replaceAll("/", "-")}.tar.gz`;
  const archivePath = join(outputRoot, archiveName);
  execFileSync("tar", ["-czf", archivePath, "-C", temporaryRoot, "package"]);
  const bytes = await readFile(archivePath);
  const artifactDigest = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  const locator = locatorBase
    ? `${locatorBase.replace(/\/+$/u, "")}/${archiveName}`
    : null;
  const distRoot = join(packageRoot, "dist");
  const outputFiles = await listFiles(distRoot);
  const styleAssets = outputFiles
    .filter((file) => file.endsWith(".css"))
    .sort()
    .map((path, order) => ({ order, path }));
  const entries = [
    { name: "module", path: "index.js" },
    ...styleAssets.map((asset) => ({
      name: `style-${asset.order}`,
      path: asset.path,
    })),
  ];
  artifact = {
    artifactDigest,
    artifactFile: archiveName,
    entries,
    entry: "index.js",
    format: "console_ui_esm",
    locator,
    manifest,
    moduleId: module.id,
    moduleReleaseDigest: releaseDigest,
    requestedPermissions: [],
    styleAssets,
  };
} finally {
  await rm(temporaryRoot, { force: true, recursive: true });
}

await writeFile(
  join(outputRoot, "artifact-index.json"),
  `${JSON.stringify({ artifacts: [artifact] }, null, 2)}\n`,
  "utf-8"
);
console.log(
  `Built the Support Ticket Console UI ESM artifact in ${outputRoot}`
);
