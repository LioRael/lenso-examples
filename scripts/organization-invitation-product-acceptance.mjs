#!/usr/bin/env node

import path from "node:path";

import {
  assertExactPrerequisites,
  assertOwnershipBoundary,
  assertScenarioSet,
  assertSharedEventContracts,
  assertSurfaceContract,
  loadAcceptanceFixture,
} from "./organization-invitation-product-acceptance-contract.mjs";

const root = path.resolve(import.meta.dirname, "..");
const resolveRoot = (environmentName, sibling) =>
  path.resolve(process.env[environmentName] ?? path.resolve(root, "..", sibling));

const fixture = await loadAcceptanceFixture();
assertExactPrerequisites(fixture);
assertOwnershipBoundary(fixture);
assertScenarioSet(fixture);

const notificationRoot = resolveRoot(
  "LENSO_NOTIFICATION_MODULE_ROOT",
  "lenso-notification-module"
);
const emailRoot = resolveRoot(
  "LENSO_EMAIL_PROVIDER_ROOT",
  "lenso-email-provider-service"
);
const contracts = await assertSharedEventContracts({
  emailRoot,
  fixture,
  notificationRoot,
});
await assertSurfaceContract({ fixture, notificationRoot });

process.stdout.write(
  `${JSON.stringify(
    {
      phase: "contract-ready",
      protocol: fixture.protocol,
      acceptanceId: fixture.id,
      prerequisites: fixture.components.framework.requiredPackages,
      contracts,
      nextAction:
        "run the Host, Provider, PostgreSQL, Console, and browser gates after the pinned packages are available",
    },
    null,
    2
  )}\n`
);
