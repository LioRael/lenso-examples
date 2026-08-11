#!/usr/bin/env node

import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";

const required = (name) => {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is required`);
  }
  return value;
};

const consoleBaseUrl = required("LENSO_ACCEPTANCE_CONSOLE_URL").replace(
  /\/+$/u,
  ""
);
const authorization = required("LENSO_ACCEPTANCE_CONSOLE_AUTHORIZATION");
const clientModule = required("LENSO_ACCEPTANCE_GENERATED_CLIENT");
const context = JSON.parse(required("LENSO_ACCEPTANCE_MANAGED_CONTEXT"));
const identity = JSON.parse(required("LENSO_ACCEPTANCE_MODULE_IDENTITY"));
const gatewayUrl = `${consoleBaseUrl}/api/console/v1/services/${encodeURIComponent(
  context.serviceId
)}/surface-gateway`;

const { createSupportTicketApi } = await import(pathToFileURL(clientModule));

const invokeGateway = async (request) => {
  const response = await fetch(gatewayUrl, {
    body: JSON.stringify(request),
    headers: {
      accept: "application/json",
      authorization,
      "content-type": "application/json",
    },
    method: "POST",
  });
  const text = await response.text();
  let body = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = text;
    }
  }
  return { body, status: response.status };
};

const acceptedRequests = [];
const client = {
  identity,
  managedServiceContext: context,
  surfaceApi: {
    invoke: async (request) => {
      const response = await invokeGateway(request);
      if (response.status < 200 || response.status >= 300) {
        throw new Error(
          `Surface Gateway ${request.operationId} returned ${response.status}: ${JSON.stringify(response.body)}`
        );
      }
      acceptedRequests.push(structuredClone(request));
      return response.body;
    },
  },
};

const api = createSupportTicketApi(client);
const requestOptions = {
  deadlineUnixMs: Date.now() + 30_000,
  idempotencyKey: "support-desk-list-0001",
  story: {
    correlationId: "corr-support-desk-acceptance",
    segmentId: "segment-support-ticket-crud",
    storyId: "support-desk.acceptance",
  },
  tenantId: "tenant-acceptance",
};

const before = await api.list({ limit: 100 }, requestOptions);
assert.ok(before.records.some((ticket) => ticket.id === "ticket_1"));

const created = await api.create(
  { priority: "high", title: "Acceptance ticket" },
  { ...requestOptions, idempotencyKey: "support-desk-create-0001" }
);
assert.equal(created.ticket.title, "Acceptance ticket");
assert.equal(created.ticket.priority, "high");

const replayed = await api.create(
  { priority: "high", title: "Acceptance ticket" },
  { ...requestOptions, idempotencyKey: "support-desk-create-0001" }
);
assert.equal(replayed.ticket.id, created.ticket.id);

const updated = await api.update(
  created.ticket.id,
  { title: "Acceptance ticket updated" },
  { ...requestOptions, idempotencyKey: "support-desk-update-0001" }
);
assert.equal(updated.ticket.title, "Acceptance ticket updated");

const closed = await api.close(created.ticket.id, {
  ...requestOptions,
  idempotencyKey: "support-desk-close-0001",
});
assert.equal(closed.ticket.status, "closed");

const after = await api.list({ limit: 100 }, requestOptions);
assert.ok(
  after.records.some(
    (ticket) =>
      ticket.id === created.ticket.id &&
      ticket.title === "Acceptance ticket updated" &&
      ticket.status === "closed"
  )
);

const acceptedCreateRequest = acceptedRequests.find(
  (request) => request.operationId === "support-ticket/http/POST:/tickets"
);
assert.ok(acceptedCreateRequest);
const wrongModuleReleaseDigest = `sha256:${"f".repeat(64)}`;
const wrongUiArtifactDigest = `sha256:${"e".repeat(64)}`;
assert.notEqual(wrongModuleReleaseDigest, identity.moduleReleaseDigest);
assert.notEqual(wrongUiArtifactDigest, identity.uiArtifactDigest);
const tamperVectors = [
  {
    label: "wrong-module-release",
    mutate: (request) => {
      request.moduleReleaseDigest = wrongModuleReleaseDigest;
    },
  },
  {
    label: "wrong-ui-artifact",
    mutate: (request) => {
      request.uiArtifactDigest = wrongUiArtifactDigest;
    },
  },
  {
    label: "wrong-delegated-actor",
    mutate: (request) => {
      request.context.delegatedActorSubject = "usr_tampered_actor";
    },
  },
  {
    label: "wrong-target-principal",
    mutate: (request) => {
      request.context.targetServicePrincipal = "svc.tampered-target";
    },
  },
];

for (const vector of tamperVectors) {
  const request = structuredClone(acceptedCreateRequest);
  vector.mutate(request);
  const response = await invokeGateway(request);
  assert.equal(response.status, 403, vector.label);
}

process.stdout.write(
  `${JSON.stringify({
    createdTicketId: created.ticket.id,
    finalStatus: closed.ticket.status,
    idempotentCreateReplay: true,
    operationCount: 4,
    positiveInvocationCount: acceptedRequests.length,
    rejectedTamperVectors: tamperVectors.map(({ label }) => label),
  })}\n`
);
