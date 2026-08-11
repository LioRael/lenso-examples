import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";

import { serveSupportTicketModule } from "./module.ts";
import {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
} from "./contract.ts";

const envNames = [
  "LENSO_ACCEPTANCE_EXPECTED_ACTOR",
  "LENSO_ACCEPTANCE_EXPECTED_ACTOR_FILE",
  "LENSO_ACCEPTANCE_EXPECTED_AUTHORITY_DIGEST",
  "LENSO_ACCEPTANCE_EXPECTED_STORY_ID",
  "LENSO_ACCEPTANCE_EXPECTED_TENANT_ID",
  "LENSO_ACCEPTANCE_OBSERVED_CONTEXT_FILE",
  "LENSO_SUPPORT_DESK_ACCEPTANCE",
  "LENSO_SERVICE_ID",
  "LENSO_SERVICE_PRINCIPAL",
  "LENSO_SERVICE_REVISION",
  "LENSO_SYSTEM_PLANE_BEARER_TOKEN",
] as const;

const originalEnv = Object.fromEntries(
  envNames.map((name) => [name, process.env[name]])
);

const contractDigest = SUPPORT_TICKET_CONTRACT_DIGEST;
const operationIds = SUPPORT_TICKET_OPERATION_IDS;

const surfaceHeaders = (
  operationId: string,
  capability: string
): Record<string, string> => ({
  "content-type": "application/json",
  "idempotency-key": `acceptance-${randomUUID()}`,
  "x-lenso-console-capability": capability,
  "x-lenso-console-contract-digest": contractDigest,
  "x-lenso-console-delegated-actor": "acceptance-operator",
  "x-lenso-console-delegated-authority": `sha256:${"a".repeat(64)}`,
  "x-lenso-console-operation-id": operationId,
  "x-lenso-console-service-id": "support-ticket",
  "x-lenso-console-story-context": JSON.stringify({
    correlationId: "correlation:acceptance",
    segmentId: "segment:surface",
    storyId: "support-desk.acceptance",
  }),
  "x-lenso-console-tenant-id": "tenant-acceptance",
  "x-lenso-deadline-unix-ms": String(Date.now() + 60_000),
});

const browserSurfaceHeaders = (
  operationId: string,
  capability: string
): Record<string, string> => {
  const headers = surfaceHeaders(operationId, capability);
  delete headers["x-lenso-console-story-context"];
  delete headers["x-lenso-console-tenant-id"];
  return headers;
};

const configureAcceptanceEnvironment = () => {
  process.env.LENSO_ACCEPTANCE_EXPECTED_ACTOR = "acceptance-operator";
  process.env.LENSO_ACCEPTANCE_EXPECTED_AUTHORITY_DIGEST =
    `sha256:${"a".repeat(64)}`;
  process.env.LENSO_ACCEPTANCE_EXPECTED_STORY_ID = "support-desk.acceptance";
  process.env.LENSO_ACCEPTANCE_EXPECTED_TENANT_ID = "tenant-acceptance";
  process.env.LENSO_SUPPORT_DESK_ACCEPTANCE = "1";
  process.env.LENSO_SERVICE_ID = "support-ticket";
};

afterEach(() => {
  for (const name of envNames) {
    const value = originalEnv[name];
    if (value === undefined) {
      delete process.env[name];
    } else {
      process.env[name] = value;
    }
  }
});

test("publishes the Provider Core identity only to the exact configured bearer", async () => {
  process.env.LENSO_SERVICE_ID = "support-ticket";
  process.env.LENSO_SERVICE_PRINCIPAL = "svc.support-ticket";
  process.env.LENSO_SERVICE_REVISION =
    "release:sha256:0123456789abcdef0123456789abcdef";
  process.env.LENSO_SYSTEM_PLANE_BEARER_TOKEN = "acceptance-core-token";

  const served = await serveSupportTicketModule({ port: 0 });

  try {
    // The ordinary examples lock stays on the last published Service Kit.
    // The product acceptance links the reviewed #529 package and exercises
    // this branch through Console's authenticated Core probe.
    if (!served.systemPlaneCoreUrl) {
      return;
    }
    assert.equal(
      served.systemPlaneCoreUrl,
      `${new URL(served.baseUrl).origin}/system-plane/v1`
    );

    const missing = await fetch(served.systemPlaneCoreUrl);
    assert.equal(missing.status, 401);
    const missingBody = await missing.json();

    const wrong = await fetch(served.systemPlaneCoreUrl, {
      headers: { authorization: "Bearer wrong-token" },
    });
    assert.equal(wrong.status, 401);
    assert.deepEqual(await wrong.json(), missingBody);

    const accepted = await fetch(served.systemPlaneCoreUrl, {
      headers: { authorization: "Bearer acceptance-core-token" },
    });
    assert.equal(accepted.status, 200);
    const document = await accepted.json();
    assert.deepEqual(document, {
      protocol: "lenso.system-plane.v1",
      serviceId: "support-ticket",
      servicePrincipal: "svc.support-ticket",
      serviceRevision:
        "release:sha256:0123456789abcdef0123456789abcdef",
    });
    assert.doesNotMatch(
      JSON.stringify([missingBody, document]),
      /acceptance-core-token/
    );
  } finally {
    await served.close();
  }
});

test("fails closed on missing Surface Gateway context only in acceptance mode", async () => {
  process.env.LENSO_SUPPORT_DESK_ACCEPTANCE = "1";
  process.env.LENSO_SERVICE_ID = "support-ticket";

  const guarded = await serveSupportTicketModule({ port: 0 });
  try {
    const response = await fetch(
      `${guarded.baseUrl}/modules/support-ticket/tickets`
    );
    assert.equal(response.status, 403);
    assert.deepEqual(await response.json(), {
      error: {
        code: "invalid_console_surface_context",
        message:
          "Support Ticket acceptance requires an exact Console Surface Gateway context",
      },
    });
  } finally {
    await guarded.close();
  }

  delete process.env.LENSO_SUPPORT_DESK_ACCEPTANCE;
  const normal = await serveSupportTicketModule({ port: 0 });
  try {
    const response = await fetch(
      `${normal.baseUrl}/modules/support-ticket/tickets`
    );
    assert.equal(response.status, 200);
  } finally {
    await normal.close();
  }
});

test("creates a new ticket for repeated ordinary POST requests outside acceptance", async () => {
  delete process.env.LENSO_SUPPORT_DESK_ACCEPTANCE;
  const served = await serveSupportTicketModule({ port: 0 });
  const ticketsUrl = `${served.baseUrl}/modules/support-ticket/tickets`;
  const headers = surfaceHeaders(
    operationIds.create,
    "support_ticket.tickets.write"
  );

  try {
    const first = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Ordinary repeated ticket" }),
      headers,
      method: "POST",
    });
    const second = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Ordinary repeated ticket" }),
      headers,
      method: "POST",
    });

    assert.equal(first.status, 201);
    assert.equal(second.status, 201);
    assert.notEqual((await first.json()).ticket.id, (await second.json()).ticket.id);
  } finally {
    await served.close();
  }
});

test("keeps acceptance idempotency scopes distinct when identity fields contain separators", async () => {
  configureAcceptanceEnvironment();
  const served = await serveSupportTicketModule({ port: 0 });
  const ticketsUrl = `${served.baseUrl}/modules/support-ticket/tickets`;
  const idempotencyKey = `acceptance-${randomUUID()}`;

  try {
    process.env.LENSO_ACCEPTANCE_EXPECTED_ACTOR = "b";
    process.env.LENSO_ACCEPTANCE_EXPECTED_TENANT_ID = "tenant:a";
    const firstHeaders = surfaceHeaders(
      operationIds.create,
      "support_ticket.tickets.write"
    );
    firstHeaders["idempotency-key"] = idempotencyKey;
    firstHeaders["x-lenso-console-delegated-actor"] = "b";
    firstHeaders["x-lenso-console-tenant-id"] = "tenant:a";
    const first = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "First separated identity" }),
      headers: firstHeaders,
      method: "POST",
    });

    process.env.LENSO_ACCEPTANCE_EXPECTED_ACTOR = "a:b";
    process.env.LENSO_ACCEPTANCE_EXPECTED_TENANT_ID = "tenant";
    const secondHeaders = surfaceHeaders(
      operationIds.create,
      "support_ticket.tickets.write"
    );
    secondHeaders["idempotency-key"] = idempotencyKey;
    secondHeaders["x-lenso-console-delegated-actor"] = "a:b";
    secondHeaders["x-lenso-console-tenant-id"] = "tenant";
    const second = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Second separated identity" }),
      headers: secondHeaders,
      method: "POST",
    });

    assert.equal(first.status, 201);
    assert.equal(second.status, 201);
    assert.notEqual((await first.json()).ticket.id, (await second.json()).ticket.id);
  } finally {
    await served.close();
  }
});

test("keeps Provider Core absent when no enrollment bearer is configured", async () => {
  delete process.env.LENSO_SYSTEM_PLANE_BEARER_TOKEN;
  const served = await serveSupportTicketModule({ port: 0 });
  try {
    assert.equal(served.systemPlaneCoreUrl, undefined);
    const response = await fetch(
      `${new URL(served.baseUrl).origin}/system-plane/v1`
    );
    assert.equal(response.status, 404);
  } finally {
    await served.close();
  }
});

test("accepts list, create, update, and close only with their exact forwarded context", async () => {
  configureAcceptanceEnvironment();
  const served = await serveSupportTicketModule({ port: 0 });
  const ticketsUrl = `${served.baseUrl}/modules/support-ticket/tickets`;

  try {
    const listHeaders = surfaceHeaders(
      operationIds.list,
      "support_ticket.tickets.read"
    );
    delete listHeaders["idempotency-key"];
    const listed = await fetch(ticketsUrl, {
      headers: listHeaders,
    });
    assert.equal(listed.status, 200);
    assert.equal((await listed.json()).records[0].id, "ticket_1");

    const createHeaders = surfaceHeaders(
      operationIds.create,
      "support_ticket.tickets.write"
    );
    const created = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Acceptance ticket" }),
      headers: createHeaders,
      method: "POST",
    });
    assert.equal(created.status, 201);
    const createdTicket = (await created.json()).ticket;
    assert.equal(createdTicket.title, "Acceptance ticket");

    const replayed = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Acceptance ticket" }),
      headers: createHeaders,
      method: "POST",
    });
    assert.equal(replayed.status, 201);
    assert.equal((await replayed.json()).ticket.id, createdTicket.id);

    const conflictingReplay = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Another ticket" }),
      headers: createHeaders,
      method: "POST",
    });
    assert.equal(conflictingReplay.status, 409);
    assert.equal((await conflictingReplay.json()).error.code, "idempotency_conflict");

    const updated = await fetch(`${ticketsUrl}/${createdTicket.id}`, {
      body: JSON.stringify({ priority: "high" }),
      headers: surfaceHeaders(
        operationIds.update,
        "support_ticket.tickets.write"
      ),
      method: "PATCH",
    });
    assert.equal(updated.status, 200);
    assert.equal((await updated.json()).ticket.priority, "high");

    const closed = await fetch(`${ticketsUrl}/${createdTicket.id}`, {
      body: JSON.stringify({ status: "closed" }),
      headers: surfaceHeaders(
        operationIds.close,
        "support_ticket.tickets.write"
      ),
      method: "PATCH",
    });
    assert.equal(closed.status, 200);
    assert.equal((await closed.json()).ticket.status, "closed");
  } finally {
    await served.close();
  }
});

test("accepts browser CRUD when optional tenant and Story context are absent", async () => {
  configureAcceptanceEnvironment();
  const served = await serveSupportTicketModule({ port: 0 });
  const ticketsUrl = `${served.baseUrl}/modules/support-ticket/tickets`;

  try {
    const listHeaders = browserSurfaceHeaders(
      operationIds.list,
      "support_ticket.tickets.read"
    );
    delete listHeaders["idempotency-key"];
    const listed = await fetch(ticketsUrl, { headers: listHeaders });
    assert.equal(listed.status, 200);

    const created = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Browser acceptance ticket" }),
      headers: browserSurfaceHeaders(
        operationIds.create,
        "support_ticket.tickets.write"
      ),
      method: "POST",
    });
    assert.equal(created.status, 201);
    const ticketId = (await created.json()).ticket.id;

    const updated = await fetch(`${ticketsUrl}/${ticketId}`, {
      body: JSON.stringify({ priority: "high" }),
      headers: browserSurfaceHeaders(
        operationIds.update,
        "support_ticket.tickets.write"
      ),
      method: "PATCH",
    });
    assert.equal(updated.status, 200);

    const closed = await fetch(`${ticketsUrl}/${ticketId}`, {
      body: JSON.stringify({ status: "closed" }),
      headers: browserSurfaceHeaders(
        operationIds.close,
        "support_ticket.tickets.write"
      ),
      method: "PATCH",
    });
    assert.equal(closed.status, 200);
    assert.equal((await closed.json()).ticket.status, "closed");
  } finally {
    await served.close();
  }
});

test("rejects every incomplete or mismatched forwarded context field", async () => {
  configureAcceptanceEnvironment();
  const served = await serveSupportTicketModule({ port: 0 });
  const ticketsUrl = `${served.baseUrl}/modules/support-ticket/tickets`;
  const valid = surfaceHeaders(
    operationIds.list,
    "support_ticket.tickets.read"
  );
  const invalidCases: readonly (readonly [string, string | undefined])[] = [
    ["x-lenso-console-delegated-actor", undefined],
    ["x-lenso-console-delegated-actor", "another-operator"],
    ["x-lenso-console-delegated-authority", undefined],
    ["x-lenso-console-delegated-authority", `sha256:${"b".repeat(64)}`],
    ["x-lenso-console-service-id", "another-service"],
    ["x-lenso-console-contract-digest", `sha256:${"b".repeat(64)}`],
    ["x-lenso-console-operation-id", operationIds.create],
    ["x-lenso-console-capability", "support_ticket.tickets.write"],
    ["x-lenso-deadline-unix-ms", String(Date.now() - 1)],
    ["x-lenso-console-tenant-id", "another-tenant"],
    [
      "x-lenso-console-story-context",
      JSON.stringify({ storyId: "another-story" }),
    ],
    [
      "x-lenso-console-story-context",
      JSON.stringify({ storyId: "support-desk.acceptance", unknown: true }),
    ],
  ];

  try {
    for (const [name, value] of invalidCases) {
      const headers = { ...valid };
      if (value === undefined) delete headers[name];
      else headers[name] = value;
      const response = await fetch(ticketsUrl, { headers });
      assert.equal(response.status, 403, name);
      assert.deepEqual(await response.json(), {
        error: {
          code: "invalid_console_surface_context",
          message:
            "Support Ticket acceptance requires an exact Console Surface Gateway context",
        },
      });
    }

    const writeHeaders = surfaceHeaders(
      operationIds.create,
      "support_ticket.tickets.write"
    );
    delete writeHeaders["idempotency-key"];
    const writeWithoutIdempotency = await fetch(ticketsUrl, {
      body: JSON.stringify({ title: "Missing idempotency" }),
      headers: writeHeaders,
      method: "POST",
    });
    assert.equal(writeWithoutIdempotency.status, 403);
  } finally {
    await served.close();
  }
});

test("records rejected provider attempts so acceptance can prove Gateway denial", async () => {
  configureAcceptanceEnvironment();
  const temporaryRoot = await mkdtemp(
    path.join(tmpdir(), "lenso-support-observation-")
  );
  const observationFile = path.join(temporaryRoot, "surface-context.jsonl");
  await writeFile(observationFile, "", { mode: 0o600 });
  process.env.LENSO_ACCEPTANCE_OBSERVED_CONTEXT_FILE = observationFile;
  const served = await serveSupportTicketModule({ port: 0 });

  try {
    const headers = surfaceHeaders(
      operationIds.list,
      "support_ticket.tickets.read"
    );
    headers["x-lenso-console-delegated-actor"] = "tampered-operator";
    const response = await fetch(
      `${served.baseUrl}/modules/support-ticket/tickets`,
      { headers }
    );
    assert.equal(response.status, 403);

    const attempts = (await readFile(observationFile, "utf8"))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    assert.equal(attempts.length, 1);
    assert.equal(attempts[0].accepted, false);
    assert.equal(attempts[0].operationId, operationIds.list);
  } finally {
    await served.close();
    await rm(temporaryRoot, { recursive: true, force: true });
  }
});

test("requires exact expected acceptance identities from the environment", async () => {
  configureAcceptanceEnvironment();
  const served = await serveSupportTicketModule({ port: 0 });
  try {
    for (const name of [
      "LENSO_ACCEPTANCE_EXPECTED_ACTOR",
      "LENSO_ACCEPTANCE_EXPECTED_AUTHORITY_DIGEST",
      "LENSO_ACCEPTANCE_EXPECTED_STORY_ID",
      "LENSO_ACCEPTANCE_EXPECTED_TENANT_ID",
      "LENSO_SERVICE_ID",
    ]) {
      const expected = process.env[name];
      delete process.env[name];
      const response = await fetch(
        `${served.baseUrl}/modules/support-ticket/tickets`,
        {
          headers: surfaceHeaders(
            operationIds.list,
            "support_ticket.tickets.read"
          ),
        }
      );
      assert.equal(response.status, 403, name);
      process.env[name] = expected;
    }
  } finally {
    await served.close();
  }
});

test("binds the forwarded actor to the Console session identity written after bootstrap", async () => {
  configureAcceptanceEnvironment();
  delete process.env.LENSO_ACCEPTANCE_EXPECTED_ACTOR;
  const temporaryRoot = await mkdtemp(
    path.join(tmpdir(), "lenso-support-actor-")
  );
  const actorFile = path.join(temporaryRoot, "console-actor");
  process.env.LENSO_ACCEPTANCE_EXPECTED_ACTOR_FILE = actorFile;
  const served = await serveSupportTicketModule({ port: 0 });

  try {
    await writeFile(actorFile, "acceptance-operator\n", { mode: 0o600 });
    const headers = surfaceHeaders(
      operationIds.list,
      "support_ticket.tickets.read"
    );
    delete headers["idempotency-key"];
    const accepted = await fetch(
      `${served.baseUrl}/modules/support-ticket/tickets`,
      { headers }
    );
    assert.equal(accepted.status, 200);

    await writeFile(actorFile, "another-operator\n", { mode: 0o600 });
    const rejected = await fetch(
      `${served.baseUrl}/modules/support-ticket/tickets`,
      { headers }
    );
    assert.equal(rejected.status, 403);
  } finally {
    await served.close();
    await rm(temporaryRoot, { recursive: true, force: true });
  }
});
