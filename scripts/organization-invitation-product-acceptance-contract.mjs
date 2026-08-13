import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const fixtureRoot = path.join(
  root,
  "fixtures",
  "acceptance",
  "organization-invitation"
);

export const REQUIRED_LENSO_VERSION = "0.3.44";
export const REQUIRED_SERVICE_KIT_VERSION = "0.6.1";
export const NOTIFICATION_MODULE_ID = "lenso/notification";
export const EMAIL_PROVIDER_MODULE_ID = "lenso/email-delivery";
export const NOTIFICATION_SURFACE_PATH = "/notifications/deliveries";

export const digestBytes = (bytes) =>
  `sha256:${createHash("sha256").update(bytes).digest("hex")}`;

export const readJson = async (file) =>
  JSON.parse(await readFile(file, "utf8"));

export const loadAcceptanceFixture = () =>
  readJson(path.join(fixtureRoot, "lenso.acceptance.json"));

export const assertExactPrerequisites = (fixture) => {
  assert.equal(fixture.protocol, "lenso.product-acceptance.v1");
  assert.equal(fixture.id, "organization-invitation-email");
  assert.equal(
    fixture.components.framework.requiredPackages.lenso,
    REQUIRED_LENSO_VERSION
  );
  assert.equal(
    fixture.components.framework.requiredPackages["@lenso/service-kit"],
    REQUIRED_SERVICE_KIT_VERSION
  );
};

export const assertOwnershipBoundary = (fixture) => {
  const { emailProvider, notification, organization } = fixture.components;
  assert.equal(organization.delivery, "linked");
  assert.equal(organization.feature, "notification");
  assert.equal(notification.delivery, "linked");
  assert.equal(notification.moduleId, NOTIFICATION_MODULE_ID);
  assert.equal(notification.surfacePath, NOTIFICATION_SURFACE_PATH);
  assert.equal(emailProvider.delivery, "service");
  assert.equal(emailProvider.moduleId, EMAIL_PROVIDER_MODULE_ID);
  assert.equal(emailProvider.responsibilityProfile, "provider");
  assert.equal(fixture.surface.ownerModuleId, NOTIFICATION_MODULE_ID);
  assert.notEqual(fixture.surface.ownerModuleId, EMAIL_PROVIDER_MODULE_ID);
};

const allowedTransitions = new Set([
  "queued->attempting",
  "attempting->accepted",
  "accepted->delivered",
  "attempting->retry_scheduled",
  "retry_scheduled->attempting",
  "attempting->failed",
  "attempting->delivery_unknown",
]);

export const assertScenarioPath = (states) => {
  assert.ok(Array.isArray(states) && states.length >= 2);
  for (let index = 1; index < states.length; index += 1) {
    const transition = `${states[index - 1]}->${states[index]}`;
    assert.ok(allowedTransitions.has(transition), `forbidden ${transition}`);
  }
  if (states.includes("accepted")) {
    assert.ok(
      states.indexOf("delivered") > states.indexOf("accepted"),
      "accepted must not be projected as delivered"
    );
  }
  if (states.includes("delivery_unknown")) {
    assert.equal(states.at(-1), "delivery_unknown");
  }
};

export const assertScenarioSet = (fixture) => {
  for (const states of Object.values(fixture.scenarios)) {
    assertScenarioPath(states);
  }
  assert.deepEqual(fixture.scenarios.success, [
    "queued",
    "attempting",
    "accepted",
    "delivered",
  ]);
  assert.equal(fixture.scenarios.retry.filter((state) => state === "attempting").length, 2);
  assert.equal(fixture.scenarios.permanent.at(-1), "failed");
  assert.equal(fixture.scenarios.ambiguous.at(-1), "delivery_unknown");
  assert.deepEqual(fixture.receiptIdentity.immutableFields, [
    "source",
    "remoteId",
    "kind",
  ]);
  assert.equal(fixture.receiptIdentity.sameRemoteIdAcrossLifecycle, true);
};

export const assertSharedEventContracts = async ({ emailRoot, fixture, notificationRoot }) => {
  assert.deepEqual(fixture.contracts, [...fixture.contracts].sort());
  assert.deepEqual(Object.keys(fixture.contractDigests), fixture.contracts);
  const evidence = [];
  for (const contract of fixture.contracts) {
    const notificationPath = path.join(notificationRoot, "contracts", "events", contract);
    const emailPath = path.join(emailRoot, "contracts", contract);
    const [notificationBytes, emailBytes] = await Promise.all([
      readFile(notificationPath),
      readFile(emailPath),
    ]);
    assert.deepEqual(
      emailBytes,
      notificationBytes,
      `${contract} must be byte-identical across owners`
    );
    const schema = JSON.parse(notificationBytes.toString("utf8"));
    assert.equal(schema.title, contract.replace(".schema.json", ""));
    assert.equal(schema.additionalProperties, false);
    const digest = digestBytes(notificationBytes);
    assert.equal(digest, fixture.contractDigests[contract]);
    evidence.push({ contract, digest });
  }
  return evidence;
};

export const assertPinnedContractDigests = (fixture) => {
  assert.deepEqual(fixture.contracts, [...fixture.contracts].sort());
  assert.deepEqual(Object.keys(fixture.contractDigests), fixture.contracts);
  for (const contract of fixture.contracts) {
    assert.match(fixture.contractDigests[contract], /^sha256:[a-f0-9]{64}$/u);
  }
};

export const assertSurfaceContract = async ({ fixture, notificationRoot }) => {
  const manifest = await readJson(
    path.join(notificationRoot, "packages", "notification-console", "console-module.json")
  );
  assert.equal(manifest.moduleId, NOTIFICATION_MODULE_ID);
  assert.ok(manifest.surfaces.some((surface) => surface.path === NOTIFICATION_SURFACE_PATH));
  const grantTemplate = await readJson(
    path.join(notificationRoot, "release", "surface-api-grant.template.json")
  );
  assert.equal(grantTemplate.contractDigest, fixture.surface.contractDigest);
  assert.deepEqual(grantTemplate.operationIds, fixture.surface.operations);
  const contract = await readJson(
    path.join(
      notificationRoot,
      "packages",
      "notification-console",
      "src",
      "notification-business-api.v1.json"
    )
  );
  assert.equal(digestBytes(await readFile(
    path.join(
      notificationRoot,
      "packages",
      "notification-console",
      "src",
      "notification-business-api.v1.json"
    )
  )), fixture.surface.contractDigest);
  assert.deepEqual(
    contract.components.schemas.DeliveryDetail.allOf[1].required,
    ["attempts", "receipts", "retry_requests", "open_in_story_correlation_id"]
  );
  assert.ok(contract.components.schemas.Delivery.required.includes("redacted_preview"));
  assert.ok(contract.components.schemas.Delivery.required.includes("content_digest"));
  assert.equal(
    contract.paths["/deliveries/{id}"].get["x-lenso-idempotency"],
    "idempotent"
  );
  assert.equal(
    contract.paths["/deliveries/{id}/retry"].post["x-lenso-idempotency"],
    "requires_key"
  );
};

export const forbiddenEvidence = ({ invitationToken, providerBearer, recipient, smtpPassword }) => [
  { label: "invitation token", value: invitationToken },
  { label: "full recipient", value: recipient },
  { label: "Provider bearer", value: providerBearer },
  { label: "SMTP credential", value: smtpPassword },
];
