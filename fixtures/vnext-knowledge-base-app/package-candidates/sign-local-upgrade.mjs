import { createHash, generateKeyPairSync, sign, verify } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const [oldArchivePath, newArchivePath, newSourceRevision, outputPath] = process.argv.slice(2);
if (!oldArchivePath || !newArchivePath || !/^[a-f0-9]{40}$/.test(newSourceRevision ?? "") || !outputPath) {
  throw new Error("usage: node sign-local-upgrade.mjs OLD.tgz NEW.tgz NEW_COMMIT_SHA OUTPUT_DIR");
}

const pluginId = "lenso.reference.knowledge-excerpt";
const packageName = "@lenso/knowledge-excerpt";
const oldSourceRevision = "f2ec2cef9ffb725535fab6ca3fc1366f166b102a";
const oldDigest = "sha256:79f2c289da9939a6d67c7b9f3fe7ff6a52be9bb2ccdb7bc871d8ccb2d932e2c6";
const catalogId = "local-knowledge-excerpt-upgrade-candidate";
const keyId = "local-upgrade-candidate";
const context = Buffer.from("lenso.marketplace.package-snapshot.v1\0", "utf8");
const now = Math.floor(Date.now() / 1000);

function archive(path, version) {
  const bytes = readFileSync(path);
  const digest = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
  const manifest = JSON.parse(execFileSync("tar", ["-xOzf", path, "package/package.json"], {
    maxBuffer: 1024 * 1024,
  }).toString("utf8"));
  if (
    manifest.name !== packageName ||
    manifest.version !== version ||
    manifest.lenso?.pluginId !== pluginId ||
    manifest.lenso?.releaseVersion !== version ||
    manifest.lenso?.runtime !== "bun"
  ) {
    throw new Error(`archive manifest does not identify ${pluginId}@${version}`);
  }
  return digest;
}

const previousDigest = archive(resolve(oldArchivePath), "0.1.0");
if (previousDigest !== oldDigest) throw new Error("0.1.0 archive differs from its original local candidate");
const nextDigest = archive(resolve(newArchivePath), "0.1.1");

function release(version, sourceRevision, integrity, summary) {
  return {
    plugin_id: pluginId,
    version,
    publisher_id: "lenso-examples",
    title: "Knowledge Excerpt",
    summary,
    source_url: "https://github.com/LioRael/lenso-examples",
    source_revision: sourceRevision,
    license: "MIT",
    distributions: [{
      id: "npm",
      kind: "npm_package",
      package: packageName,
      version,
      integrity,
      registry_url: "https://registry.npmjs.org",
    }],
    availability: "listed",
  };
}

const previousRelease = release(
  "0.1.0",
  oldSourceRevision,
  previousDigest,
  "Deterministic knowledge excerpts with optional durable Jobs processing",
);
const nextRelease = release(
  "0.1.1",
  newSourceRevision,
  nextDigest,
  "Grapheme-safe knowledge excerpts with optional durable Jobs processing",
);
const { privateKey, publicKey } = generateKeyPairSync("ed25519");
const spki = publicKey.export({ format: "der", type: "spki" });
const rawKeyPrefix = Buffer.from("302a300506032b6570032100", "hex");
if (spki.length !== rawKeyPrefix.length + 32 || !spki.subarray(0, rawKeyPrefix.length).equals(rawKeyPrefix)) {
  throw new Error("unexpected Ed25519 public-key encoding");
}

const outputDir = resolve(outputPath);
mkdirSync(outputDir, { recursive: true });
for (const [revision, releases] of [[1, [previousRelease]], [2, [previousRelease, nextRelease]]]) {
  const payload = Buffer.from(JSON.stringify({
    schema: "lenso.marketplace.package-snapshot.v1",
    catalog_id: catalogId,
    revision,
    issued_at: now,
    expires_at: now + 6 * 24 * 60 * 60,
    releases,
  }) + "\n", "utf8");
  const signedBytes = Buffer.concat([context, Buffer.from(keyId), Buffer.from([0]), payload]);
  const signature = sign(null, signedBytes, privateKey);
  if (!verify(null, signedBytes, publicKey, signature)) throw new Error("signature self-check failed");
  writeFileSync(resolve(outputDir, `package-snapshot-r${revision}.json`), JSON.stringify({
    key_id: keyId,
    payload_base64: payload.toString("base64"),
    signature_base64: signature.toString("base64"),
  }) + "\n", { flag: "wx" });
}
writeFileSync(resolve(outputDir, "trust.json"), JSON.stringify({
  catalog_id: catalogId,
  key_id: keyId,
  public_key_hex: spki.subarray(rawKeyPrefix.length).toString("hex"),
}) + "\n", { flag: "wx" });
console.log(JSON.stringify({ old_digest: previousDigest, new_digest: nextDigest, catalog_id: catalogId }));
