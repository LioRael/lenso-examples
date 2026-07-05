import test from "node:test";
import assert from "node:assert/strict";

import {
  expectAuditLogDataSurface,
  expectLoadedModule,
} from "./support-ticket-host-api-smoke-contract.ts";

test("recognizes the audit-log schema-admin data surface", () => {
  const auditLog = expectLoadedModule(
    {
      modules: [
        {
          admin: {
            kind: "schema",
            entities: [
              {
                fields: [{ name: "event_name" }],
                label: "Audit Events",
                name: "events",
                read_capability: "audit_log.events.read",
              },
            ],
          },
          capabilities: ["audit_log.events.read"],
          governance: { activation_state: "active" },
          manifest_lints: [{ severity: "ok" }],
          module_name: "audit-log",
          status: "loaded",
        },
      ],
    },
    "audit-log",
  );

  expectAuditLogDataSurface(auditLog);
});

test("rejects an audit-log module without the events read capability", () => {
  const auditLog = expectLoadedModule(
    {
      modules: [
        {
          admin: {
            kind: "schema",
            entities: [
              {
                fields: [{ name: "event_name" }],
                label: "Audit Events",
                name: "events",
                read_capability: "audit_log.events.read",
              },
            ],
          },
          capabilities: [],
          governance: { activation_state: "active" },
          manifest_lints: [{ severity: "ok" }],
          module_name: "audit-log",
          status: "loaded",
        },
      ],
    },
    "audit-log",
  );

  assert.throws(
    () => expectAuditLogDataSurface(auditLog),
    /audit-log events read capability was not listed/,
  );
});
