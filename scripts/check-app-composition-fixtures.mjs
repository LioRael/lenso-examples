import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const root = path.join(process.cwd(), "fixtures/launchpad/support-desk-composition");
const composition = JSON.parse(
  fs.readFileSync(path.join(root, "lenso.app.json"), "utf8"),
);
const workspace = JSON.parse(
  fs.readFileSync(path.join(root, "lenso.workspace.json"), "utf8"),
);

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function digestJson(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function productContractDigest(module) {
  return digestJson({
    protocol: "lenso.module-product-contract.v1",
    id: module.moduleId,
    version: module.release.version,
    owner: module.release.owner,
    businessContributions: module.release.businessContributions,
    dependencies: module.dependencies.map((dependency) => dependency.requirement),
  });
}

function moduleById(id) {
  return composition.modules.find((module) => module.moduleId === id);
}

assert(composition.protocol === "lenso.app-composition.v1", "composition protocol");
assert(composition.appId === "support-desk", "composition app id");
assert(composition.revision === 1, "initial composition revision");
assert(/^sha256:[0-9a-f]{64}$/.test(composition.contentDigest), "composition digest");
assert(
  digestJson({
    protocol: composition.protocol,
    appId: composition.appId,
    revision: composition.revision,
    modules: composition.modules,
    provenance: composition.provenance,
  }) === composition.contentDigest,
  "composition digest matches content",
);
assert(composition.modules.length === 3, "exact Support Desk Module selection");

const auth = moduleById("auth");
const supportApi = moduleById("support-api");
const notificationWorker = moduleById("notification-worker");
assert(auth?.implementation?.kind === "linked", "auth is linked");
for (const module of composition.modules) {
  assert(
    module.release.contentDigest === productContractDigest(module),
    `${module.moduleId} release digest matches Product Contract`,
  );
}
assert(supportApi?.implementation?.serviceReference === "service:support-api", "Support API binding");
assert(
  notificationWorker?.implementation?.serviceReference === "service:notification-worker",
  "notification binding",
);
assert(auth.release.businessContributions.length > 0, "auth product contribution");
assert(supportApi.release.contentDigest.startsWith("sha256:"), "Support API release digest");
assert(
  supportApi.dependencies[0].moduleId === "auth" &&
    supportApi.dependencies[0].contentDigest === auth.release.contentDigest,
  "Support API dependency selection",
);
assert(
  notificationWorker.dependencies[0].moduleId === "support-api" &&
    notificationWorker.dependencies[0].contentDigest === supportApi.release.contentDigest,
  "notification dependency selection",
);

const serviceNames = new Set(workspace.services.map((service) => service.name));
for (const module of composition.modules) {
  const reference = module.implementation.serviceReference;
  if (reference) {
    assert(!reference.includes("://") && !reference.includes("@"), "deployment-neutral Service Reference");
    const serviceName = reference.slice("service:".length).split("/").at(-1);
    assert(serviceNames.has(serviceName), `workspace connection for ${serviceName}`);
  }
}
assert(!fs.existsSync(path.join(root, "lenso.system.json")), "no second System lock");
assert(!fs.existsSync(path.join(root, "lenso.system-sandbox.json")), "no sandbox overlay");
assert(!fs.existsSync(path.join(root, ".lenso/app-change-plan.json")), "no App Change Plan overlay");

console.log("exact Support Desk App Composition fixture: ready");
