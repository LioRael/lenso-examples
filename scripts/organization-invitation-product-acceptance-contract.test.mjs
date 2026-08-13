import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  assertExactPrerequisites,
  assertOwnershipBoundary,
  assertPinnedContractDigests,
  assertScenarioPath,
  assertScenarioSet,
  assertSharedEventContracts,
  assertSurfaceContract,
  forbiddenEvidence,
  loadAcceptanceFixture,
} from "./organization-invitation-product-acceptance-contract.mjs";
import { assertNoForbiddenEvidence } from "./support-desk-product-acceptance-runtime.mjs";

const notificationRoot = process.env.LENSO_NOTIFICATION_MODULE_ROOT?.trim();
const emailRoot = process.env.LENSO_EMAIL_PROVIDER_ROOT?.trim();

test("pins the exact integration prerequisites, Surface contract, and owners", async () => {
  const fixture = await loadAcceptanceFixture();
  assertExactPrerequisites(fixture);
  assertOwnershipBoundary(fixture);
  assertScenarioSet(fixture);
  assertPinnedContractDigests(fixture);
  if (notificationRoot) {
    await assertSurfaceContract({
      fixture,
      notificationRoot: path.resolve(notificationRoot),
    });
  }
});

test("Email Provider and Notification share exact committed Event schemas", async () => {
  const fixture = await loadAcceptanceFixture();
  if (!(emailRoot && notificationRoot)) {
    assertPinnedContractDigests(fixture);
    return;
  }
  const evidence = await assertSharedEventContracts({
    emailRoot: path.resolve(emailRoot),
    fixture,
    notificationRoot: path.resolve(notificationRoot),
  });
  assert.equal(evidence.length, 3);
  assert.ok(evidence.every(({ digest }) => /^sha256:[a-f0-9]{64}$/u.test(digest)));
});

test("truthful states reject accepted-as-delivered and automatic ambiguous retry", () => {
  assert.throws(
    () => assertScenarioPath(["queued", "attempting", "delivered"]),
    /forbidden attempting->delivered/u
  );
  assert.throws(
    () => assertScenarioPath(["queued", "attempting", "delivery_unknown", "attempting"]),
    /forbidden delivery_unknown->attempting/u
  );
});

test("receipt identity permits accepted and delivered evidence for one remote message", async () => {
  const fixture = await loadAcceptanceFixture();
  assert.deepEqual(fixture.receiptIdentity.immutableFields, [
    "source",
    "remoteId",
    "kind",
  ]);
  assert.equal(fixture.receiptIdentity.sameRemoteIdAcrossLifecycle, true);
});

test("browser evidence rejects every server-owned or sensitive delivery value", () => {
  const values = forbiddenEvidence({
    invitationToken: "acceptance-secret-invitation-token",
    providerBearer: "acceptance-secret-provider-bearer",
    recipient: "member-acceptance@example.test",
    smtpPassword: "acceptance-secret-smtp-password",
  });
  assert.doesNotThrow(() =>
    assertNoForbiddenEvidence("m***@example.test delivered", values)
  );
  for (const forbidden of values) {
    assert.throws(
      () => assertNoForbiddenEvidence(`evidence ${forbidden.value}`, values),
      new RegExp(forbidden.label, "u")
    );
  }
});
