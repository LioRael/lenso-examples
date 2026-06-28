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

function expectModuleNames(manifest, expected) {
  const names = manifest.modules.map((module) => module.name).sort();
  if (JSON.stringify(names) !== JSON.stringify(expected)) {
    throw new Error(
      `Expected modules ${expected.join(", ")}, got ${names.join(", ")}`
    );
  }
}

const invokeRuntime = (server, moduleName, functionName, input) =>
  fetchJson(
    `${server.baseUrl}/modules/${moduleName}/runtime/functions/${functionName}/invoke`,
    {
      body: JSON.stringify({
        actor: { id: "support-smoke", kind: "service", scopes: [] },
        attempt: 1,
        correlation_id: "corr_support_ticket_smoke",
        function_name: functionName,
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
    catalogEntry.name !== "support-ticket" ||
    catalogEntry.source !== "service" ||
    catalogEntry.providedBy !== "support-suite-provider" ||
    catalogEntry.serviceManifest !==
      "http://127.0.0.1:4110/lenso/service/v1/manifest" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4110/lenso/service/v1"
  ) {
    throw new Error("catalog entry does not describe support-ticket");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "support-suite-provider" ||
    manifest.protocol !== "lenso.service.v1" ||
    manifest.version !== catalogEntry.version
  ) {
    throw new Error("manifest did not return support-suite-provider");
  }
  expectModuleNames(manifest, [
    "support-knowledge-base",
    "support-notification",
    "support-ticket",
  ]);
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
  const listRoute = moduleManifest.http_routes?.find(
    (route) => route.method === "GET" && route.path === "/tickets"
  );
  if (
    listRoute?.operation?.operationId !== "support-ticket/http/GET:/tickets"
  ) {
    throw new Error("support-ticket did not declare the tickets HTTP operation");
  }
  if (
    listRoute.operation.safeProbe?.method !== "GET" ||
    listRoute.operation.safeProbe?.path !== "/tickets" ||
    listRoute.operation.safeProbe?.expectStatus !== 200
  ) {
    throw new Error("tickets HTTP operation did not declare a safe probe");
  }
  const assignAction = moduleManifest.admin.actions.find(
    (action) => action.name === "assign_ticket"
  );
  if (
    assignAction?.operation?.operationId !==
    "support-ticket/action/assign_ticket"
  ) {
    throw new Error("assign_ticket did not declare operation metadata");
  }
  const escalateFunction = moduleManifest.runtime?.functions?.find(
    (runtimeFunction) =>
      runtimeFunction.name === "support-ticket.escalate-ticket.v1"
  );
  if (
    escalateFunction?.operation?.operationId !==
    "support-ticket/runtime/support-ticket.escalate-ticket.v1"
  ) {
    throw new Error("escalate runtime function did not declare operation metadata");
  }

  const statusUrl = server.statusUrl ?? `${server.baseUrl}/status`;
  const status = await fetchJson(statusUrl);
  if (
    status.serviceName !== "support-suite-provider" ||
    status.state !== "ready"
  ) {
    throw new Error(
      "status endpoint did not return support-suite-provider readiness"
    );
  }
  expectModuleNames(status, [
    "support-knowledge-base",
    "support-notification",
    "support-ticket",
  ]);

  const moduleBaseUrl = `${server.baseUrl}/modules/support-ticket`;
  const listed = await fetchJson(`${moduleBaseUrl}/tickets`);
  if (!listed.records?.some((record) => record.id === "ticket_1")) {
    throw new Error("HTTP list route did not return ticket_1");
  }

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

  const escalated = await invokeRuntime(
    server,
    "support-ticket",
    "support-ticket.escalate-ticket.v1",
    {
      ticket_id: "ticket_2",
      updated_at: "2026-06-20T02:00:00Z",
    }
  );
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

  const notification = await invokeRuntime(
    server,
    "support-notification",
    "support-notification.send-ticket-update.v1",
    { ticket_id: "ticket_2" }
  );
  if (
    notification.output?.delivered !== true ||
    notification.output?.ticket_id !== "ticket_2"
  ) {
    throw new Error("support-notification did not send ticket update");
  }

  const article = await fetchJson(
    `${server.baseUrl}/modules/support-knowledge-base/articles/invite-teammates`
  );
  if (article.article?.title !== "Invite teammates") {
    throw new Error("support-knowledge-base did not return article");
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
