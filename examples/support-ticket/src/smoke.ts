#!/usr/bin/env node
import { readFile } from "node:fs/promises";

import { serveSupportTicketModule } from "./module.ts";

const fetchJson = async (url, init) => {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  return response.json();
};

const invokeRuntime = (server, input) =>
  fetchJson(
    `${server.baseUrl}/modules/support-ticket/runtime/functions/support-ticket.escalate-ticket.v1/invoke`,
    {
      body: JSON.stringify({
        actor: { id: "support-smoke", kind: "service", scopes: [] },
        attempt: 1,
        correlation_id: "corr_support_ticket_smoke",
        function_name: "support-ticket.escalate-ticket.v1",
        function_run_id: "fnrun_support_ticket_smoke",
        input,
        request_id: "req_support_ticket_smoke",
        trace: {
          span_id: "span_support_ticket_smoke",
          trace_id: "trace_support_ticket_smoke",
        },
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );

const server = await serveSupportTicketModule({ port: 0 });

try {
  const catalogEntry = JSON.parse(
    await readFile(new URL("../catalog-entry.json", import.meta.url), "utf8")
  );
  if (
    catalogEntry.name !== "support-service" ||
    catalogEntry.source !== "service" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4110/lenso/service/v1"
  ) {
    throw new Error("catalog entry does not describe support-service");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "support-service" ||
    manifest.protocol !== "lenso.service.v1" ||
    manifest.version !== catalogEntry.version
  ) {
    throw new Error("manifest did not return support-service");
  }
  const moduleManifest = manifest.modules?.find(
    (module) => module.name === "support-ticket"
  );
  if (!moduleManifest) {
    throw new Error("service manifest did not provide support-ticket");
  }
  for (const capability of catalogEntry.capabilities) {
    if (!moduleManifest.capabilities.includes(capability)) {
      throw new Error(
        `support-ticket module is missing catalog capability ${capability}`
      );
    }
  }
  if (
    moduleManifest.admin?.kind !== "declarative_custom" ||
    !moduleManifest.admin.actions?.some(
      (action) => action.name === "assign_ticket"
    )
  ) {
    throw new Error("support-ticket did not declare the assign_ticket action");
  }
  if (!moduleManifest.admin.fallback_schema?.entities?.length) {
    throw new Error("support-ticket did not expose tickets as fallback schema");
  }
  if (manifest.status_path !== "/lenso/service/v1/status") {
    throw new Error("manifest did not declare the service status path");
  }

  const statusUrl = server.statusUrl ?? `${server.baseUrl}/status`;
  const status = await fetchJson(statusUrl);
  if (
    status.serviceName !== "support-service" ||
    status.state !== "ready" ||
    !status.modules?.some((module) => module.name === "support-ticket")
  ) {
    throw new Error("status endpoint did not return support-service readiness");
  }

  const moduleBaseUrl = `${server.baseUrl}/modules/support-ticket`;
  const created = await fetchJson(`${moduleBaseUrl}/tickets`, {
    body: JSON.stringify({
      assignee: "triage",
      priority: "normal",
      title: "Billing export failed",
    }),
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  if (created.ticket?.id !== "ticket_2") {
    throw new Error("HTTP create route did not create ticket_2");
  }

  const assigned = await fetchJson(
    `${moduleBaseUrl}/admin/actions/assign_ticket`,
    {
      body: JSON.stringify({
        assignee: "alex",
        ticket_id: "ticket_2",
        updated_at: "2026-06-20T01:00:00Z",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (assigned.result?.ticket?.assignee !== "alex") {
    throw new Error("admin action did not assign ticket_2");
  }

  const escalated = await invokeRuntime(server, {
    ticket_id: "ticket_2",
    updated_at: "2026-06-20T02:00:00Z",
  });
  if (
    escalated.output?.priority !== "high" ||
    escalated.output?.status !== "escalated"
  ) {
    throw new Error("runtime function did not escalate ticket_2");
  }

  const ticket = await fetchJson(`${moduleBaseUrl}/tickets/ticket_2`);
  if (
    ticket.ticket?.title !== "Billing export failed" ||
    ticket.ticket?.assignee !== "alex"
  ) {
    throw new Error("HTTP detail route did not return ticket_2");
  }

  const admin = await fetchJson(`${moduleBaseUrl}/admin/tickets`);
  const ids = admin.records?.map((record) => record.id) ?? [];
  if (!ids.includes("ticket_1") || !ids.includes("ticket_2")) {
    throw new Error("schema-admin endpoint did not return tickets");
  }

  console.log("Support Ticket service smoke passed");
} finally {
  await server.close();
}
