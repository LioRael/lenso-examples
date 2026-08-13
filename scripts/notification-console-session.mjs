#!/usr/bin/env node

import assert from "node:assert/strict";
import { generateKeyPairSync, randomBytes } from "node:crypto";
import { createServer } from "node:http";
import { mkdir, mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  buildEnrollmentExchange,
  digestJson,
  publicEd25519KeyBase64url,
} from "./support-desk-product-acceptance-contract.mjs";
import {
  fetchJson,
  freePort,
  prepareProviderSandbox,
  runCommand,
  spawnManaged,
  startStaticServer,
  terminateManaged,
  waitForHttp,
} from "./support-desk-product-acceptance-runtime.mjs";

const root = path.resolve(import.meta.dirname, "..");
const required = (name) => {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
};
const consoleRoot = path.resolve(required("LENSO_CONSOLE_ROOT"));
const frameworkRoot = path.resolve(required("LENSO_FRAMEWORK_ROOT"));
const notificationRoot = path.resolve(required("LENSO_NOTIFICATION_MODULE_ROOT"));
const hostOrigin = required("LENSO_NOTIFICATION_HOST_ORIGIN").replace(/\/+$/u, "");
const databaseUrl = required("LENSO_ACCEPTANCE_DATABASE_URL");
if (!new URL(databaseUrl).pathname.includes("acceptance")) {
  throw new Error("LENSO_ACCEPTANCE_DATABASE_URL must name an acceptance database");
}

const temporaryRoot = await mkdtemp(
  path.join(tmpdir(), "lenso-notification-console-session-")
);
const cleanup = [];
let cleanupPromise;
const stop = async () => {
  cleanupPromise ??= (async () => {
    for (const dispose of cleanup.reverse()) await dispose().catch(() => {});
  })();
  return cleanupPromise;
};
process.once("SIGINT", () => void stop().finally(() => process.exit(0)));
process.once("SIGTERM", () => void stop().finally(() => process.exit(0)));
process.once("uncaughtException", (error) => {
  console.error(error);
  void stop().finally(() => process.exit(1));
});
process.once("unhandledRejection", (error) => {
  console.error(error);
  void stop().finally(() => process.exit(1));
});

const postJson = (url, authorization, body, expected = [200]) =>
  fetchJson(
    url,
    {
      body: JSON.stringify(body),
      headers: {
        accept: "application/json",
        authorization,
        "content-type": "application/json",
      },
      method: "POST",
    },
    expected
  );

const artifactRoot = path.join(
  notificationRoot,
  "dist",
  "notification-console-artifact"
);
const releaseRoot = path.join(
  notificationRoot,
  "dist",
  "notification-release"
);
const artifactIndex = JSON.parse(
  await readFile(path.join(artifactRoot, "artifact-index.json"), "utf8")
);
const artifact = artifactIndex.artifacts?.find(
  (value) => value.moduleId === "lenso/notification"
);
assert.ok(artifact, "Notification Console artifact is missing");
const grant = JSON.parse(
  await readFile(path.join(releaseRoot, "surface-api-grant.json"), "utf8")
);
assert.equal(grant.artifactDigest, artifact.artifactDigest);
assert.equal(grant.moduleReleaseDigest, artifact.moduleReleaseDigest);

const artifactServer = await startStaticServer(artifactRoot);
cleanup.push(() => artifactServer.close());
artifact.locator = `${artifactServer.baseUrl}/${artifact.artifactFile}`;

const serviceId = "notification-host";
const servicePrincipal = "svc.notification-host";
const systemId = "notification-acceptance";
const systemPlaneToken = randomBytes(32).toString("base64url");
const corePort = await freePort();
const providerRoot = path.join(temporaryRoot, "managed-host-core");
await prepareProviderSandbox({ frameworkRoot, sourceRoot: root, targetRoot: providerRoot });
const coreProcess = spawnManaged("node", ["src/server.ts"], {
  cwd: providerRoot,
  env: {
    LENSO_SERVICE_ID: serviceId,
    LENSO_SERVICE_PRINCIPAL: servicePrincipal,
    LENSO_SERVICE_REVISION: "1",
    LENSO_SYSTEM_PLANE_BEARER_TOKEN: systemPlaneToken,
    PORT: String(corePort),
  },
  label: "notification-managed-host-core",
});
cleanup.push(() => terminateManaged(coreProcess));
const coreOrigin = `http://127.0.0.1:${corePort}`;
await waitForHttp(`${coreOrigin}/lenso/service/v1/status`, 60_000);

const proxy = createServer(async (request, response) => {
  try {
    const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
    const upstreamOrigin = requestUrl.pathname.startsWith("/lenso/service/v1/") ||
      requestUrl.pathname === "/system-plane/v1"
      ? coreOrigin
      : hostOrigin;
    const body = request.method === "GET" || request.method === "HEAD"
      ? undefined
      : await new Promise((resolve, reject) => {
          const chunks = [];
          request.on("data", (chunk) => chunks.push(chunk));
          request.once("end", () => resolve(Buffer.concat(chunks)));
          request.once("error", reject);
        });
    const headers = new Headers();
    for (const [name, value] of Object.entries(request.headers)) {
      if (value !== undefined && name !== "host" && name !== "content-length") {
        headers.set(name, Array.isArray(value) ? value.join(", ") : value);
      }
    }
    const upstream = await fetch(`${upstreamOrigin}${requestUrl.pathname}${requestUrl.search}`, {
      body,
      headers,
      method: request.method,
      redirect: "manual",
    });
    const bytes = Buffer.from(await upstream.arrayBuffer());
    response.writeHead(
      upstream.status,
      Object.fromEntries(
        [...upstream.headers].filter(([name]) =>
          !["content-encoding", "content-length", "transfer-encoding"].includes(name)
        )
      )
    );
    response.end(bytes);
  } catch (error) {
    response.writeHead(502, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: String(error) }));
  }
});
await new Promise((resolve, reject) => {
  proxy.once("error", reject);
  proxy.listen(0, "127.0.0.1", resolve);
});
cleanup.push(() => new Promise((resolve) => proxy.close(resolve)));
const proxyAddress = proxy.address();
assert.ok(proxyAddress && typeof proxyAddress !== "string");
const managedOrigin = `http://127.0.0.1:${proxyAddress.port}`;

const consoleKeys = generateKeyPairSync("ed25519");
const serviceKeys = generateKeyPairSync("ed25519");
const consoleKeyId = "notification-console-key";
const serviceKeyId = "notification-host-key";
const consolePrincipal = "svc.lenso-console";
const policy = {
  digest: digestJson({ policy: "notification-management", revision: 1 }),
  policyId: "notification-management",
  revision: 1,
};
const enrollmentPolicy = {
  policyDigest: policy.digest,
  policyId: policy.policyId,
  policyRevision: String(policy.revision),
};
const now = Date.now();
const exchange = buildEnrollmentExchange({
  consoleKeyId,
  consolePrivateKey: consoleKeys.privateKey,
  consoleServicePrincipal: consolePrincipal,
  expiresAtUnixMs: now + 60 * 60_000,
  issuedAtUnixMs: now,
  managedServiceId: serviceId,
  managedServicePrincipal: servicePrincipal,
  managedServiceRevision: "1",
  policy: enrollmentPolicy,
  serviceKeyId,
  servicePrivateKey: serviceKeys.privateKey,
  systemId,
});
const receiptDigest = exchange.receipt.signature.subjectDigest;
const enrollmentTrust = {
  consoleAuthorityKeys: [
    {
      consoleServicePrincipal: consolePrincipal,
      keyId: consoleKeyId,
      publicKeyBase64url: publicEd25519KeyBase64url(consoleKeys.publicKey),
    },
  ],
  managedServiceKeys: [
    {
      baseUrl: managedOrigin,
      keyId: serviceKeyId,
      managedServiceId: serviceId,
      managedServicePrincipal: servicePrincipal,
      publicKeyBase64url: publicEd25519KeyBase64url(serviceKeys.publicKey),
      systemId,
      systemPlaneBearerToken: systemPlaneToken,
    },
  ],
};

const topology = {
  protocol: "lenso.system.v2",
  systemId,
  services: [
    {
      serviceId,
      servicePrincipal,
      revision: 1,
      workloads: [{ workloadId: "notification-host-api", role: "api" }],
    },
  ],
  modules: [
    {
      moduleId: "lenso/notification",
      delivery: "linked",
      serviceId,
      moduleReleaseDigest: artifact.moduleReleaseDigest,
      consoleUiArtifactDigest: artifact.artifactDigest,
      surfaceApiGrant: grant,
      runtimeStatus: "active",
    },
  ],
  adapters: [],
};
const topologyDigest = digestJson(topology);
const connectRequest = {
  systemId,
  topologyDigest,
  topology,
  managementBinding: {
    systemId,
    topologyDigest,
    serviceIds: [serviceId],
    adapterIds: [],
    permissions: [
      "console.module.business.read",
      "console.module.business.write",
    ],
    policy,
  },
};

const consolePort = await freePort();
const consoleUrl = `http://127.0.0.1:${consolePort}`;
const recoveryToken = randomBytes(32).toString("base64url");
const operatorIdentifier = "notification-operator@example.test";
const operatorCredential = `${randomBytes(24).toString("base64url")}Aa9!`;
const consoleEnvironment = {
  CARGO_TARGET_DIR:
    process.env.LENSO_CONSOLE_CARGO_TARGET_DIR ??
    "/private/tmp/lenso-console-notification-target",
  CONSOLE_ARTIFACT_ROOT: path.join(temporaryRoot, "console-artifact-store"),
  DATABASE_URL: databaseUrl,
  HTTP_HOST: "127.0.0.1",
  HTTP_PORT: String(consolePort),
  LENSO_COMPOSITION_PROFILE: "core",
  LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST: JSON.stringify(enrollmentTrust),
  LENSO_MODULE_PLATFORM_STORY_ENABLED: "false",
  SERVICE_NAME: "lenso-console",
  VITE_API_AUTH_TOKEN: undefined,
};
await mkdir(consoleEnvironment.CONSOLE_ARTIFACT_ROOT, { recursive: true });
await runCommand("pnpm", ["service:migrate"], {
  cwd: consoleRoot,
  env: { ...consoleEnvironment, CONSOLE_RECOVERY_MODE: "normal" },
  label: "notification-console-migrate",
});
let consoleProcess = spawnManaged("pnpm", ["service:serve"], {
  cwd: consoleRoot,
  env: {
    ...consoleEnvironment,
    CONSOLE_BOOTSTRAP_RECOVERY_TOKEN: recoveryToken,
    CONSOLE_RECOVERY_MODE: "restore",
  },
  label: "notification-console-recovery",
});
await waitForHttp(`${consoleUrl}/health/ready`, 180_000);
const bootstrap = await fetchJson(
  `${consoleUrl}/bootstrap/v1/recovery`,
  {
    body: JSON.stringify({ identifier: operatorIdentifier, password: operatorCredential }),
    headers: {
      accept: "application/json",
      "content-type": "application/json",
      "x-lenso-console-recovery-token": recoveryToken,
    },
    method: "POST",
  },
  [200]
);
await terminateManaged(consoleProcess);
consoleProcess = spawnManaged("pnpm", ["service:serve"], {
  cwd: consoleRoot,
  env: {
    ...consoleEnvironment,
    CONSOLE_BOOTSTRAP_RECOVERY_TOKEN: "",
    CONSOLE_RECOVERY_MODE: "normal",
  },
  label: "notification-console",
});
cleanup.push(() => terminateManaged(consoleProcess));
await waitForHttp(`${consoleUrl}/health/ready`, 180_000);

const operatorAuthorization = `Bearer ${bootstrap.body.token}`;
const artifactAuthorization = `Bearer dev-user:notification-artifact-${randomBytes(8).toString("hex")}:console.artifacts.manage`;
await postJson(
  `${consoleUrl}/api/console/v1/artifacts/reconcile`,
  artifactAuthorization,
  {
    artifacts: [
      {
        module_id: artifact.moduleId,
        module_release_digest: artifact.moduleReleaseDigest,
        locator: artifact.locator,
        digest: artifact.artifactDigest,
        format: artifact.format,
        protocol_major: 1,
        entry: artifact.entry,
        entries: artifact.entries,
        style_assets: artifact.styleAssets,
        manifest: artifact.manifest,
        requested_permissions: artifact.requestedPermissions,
      },
    ],
    candidate_lock_digest: artifact.moduleReleaseDigest,
    console_service_id: "lenso-console",
    effect_id: "notification-console-acceptance",
    kind: "console_composition",
    theme_bundles: [],
  }
);
await postJson(
  `${consoleUrl}/api/console/v1/enrollment-receipts`,
  operatorAuthorization,
  { baseUrl: managedOrigin, ...exchange },
  [201]
);
const connected = await postJson(
  `${consoleUrl}/api/console/v1/system/connect`,
  operatorAuthorization,
  connectRequest
);
assert.equal(connected.body.status, "connected");

process.stdout.write(
  `${JSON.stringify({
    phase: "ready",
    consoleUrl,
    directRoute: `${consoleUrl}/notifications/deliveries`,
    operatorIdentifier,
    operatorPassword: operatorCredential,
    moduleReleaseDigest: artifact.moduleReleaseDigest,
    artifactDigest: artifact.artifactDigest,
    receiptDigest,
    temporaryRoot,
  })}\n`
);
await new Promise(() => {});
