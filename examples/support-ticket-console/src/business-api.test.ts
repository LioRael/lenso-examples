import type {
  ConsoleClient,
  ConsoleSha256Digest,
  ManagedServiceContext,
  SurfaceOperationRequest,
} from "@lenso/console-module-api";
import { describe, expect, test } from "vitest";

import {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
  createSupportTicketApi,
  supportTicketContract,
} from "./business-api";

const digest =
  "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" as ConsoleSha256Digest;

const managedServiceContext: ManagedServiceContext = {
  callerModuleId: "support/tickets",
  capabilities: [
    "console.module.business.read",
    "console.module.business.write",
  ],
  delegatedActorSubject: "operator-1",
  delegatedAuthorityDigest: digest,
  environmentId: "production",
  serviceId: "support-api",
  systemId: "support-desk",
  targetServicePrincipal: "svc.support-api",
};

const createFakeClient = (
  requests: SurfaceOperationRequest<unknown>[]
): ConsoleClient => ({
  capabilities: {
    has: () => true,
    list: () => managedServiceContext.capabilities,
  },
  identity: {
    moduleId: "support/tickets",
    moduleReleaseDigest: digest,
    uiArtifactDigest: digest,
  },
  inventory: () => {
    throw new Error("unused");
  },
  managedServiceContext,
  navigate: () => {},
  readConfig: () => {
    throw new Error("unused");
  },
  resolveActionContributions: () => {
    throw new Error("unused");
  },
  surfaceApi: {
    invoke: <Input, Output>(request: SurfaceOperationRequest<Input>) => {
      requests.push(request as SurfaceOperationRequest<unknown>);
      return Promise.resolve({
        contractDigest: request.contractDigest,
        moduleId: request.moduleId,
        operationId: request.operationId,
        output: { ticket: { id: "ticket-1" } } as Output,
        protocol: "lenso.console-surface-gateway.v1",
        requestContext: request.requestContext,
      });
    },
  },
  writeConfig: () => {
    throw new Error("unused");
  },
});

describe("Support Ticket generated Business API client", () => {
  test("sends only the contract operation, typed input, and preserved request context", async () => {
    const requests: SurfaceOperationRequest<unknown>[] = [];
    const client = createFakeClient(requests);
    const api = createSupportTicketApi(client);

    await api.create(
      { priority: "high", title: "Export is stuck" },
      {
        story: { correlationId: "corr-1", storyId: "story-1" },
        tenantId: "tenant-1",
      }
    );

    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({
      contractDigest: SUPPORT_TICKET_CONTRACT_DIGEST,
      input: { priority: "high", title: "Export is stuck" },
      operationId: SUPPORT_TICKET_OPERATION_IDS.create,
      protocol: "lenso.console-surface-gateway.v1",
      requestContext: {
        story: { correlationId: "corr-1", storyId: "story-1" },
        tenantId: "tenant-1",
      },
    });
    expect(requests[0]).not.toHaveProperty("url");
    expect(requests[0]).not.toHaveProperty("method");
    expect(requests[0]?.requestContext.idempotencyKey).toBeTruthy();
    expect(requests[0]?.requestContext.deadlineUnixMs).toBeGreaterThan(
      Date.now()
    );
  });

  test("keeps close as a distinct stable operation", async () => {
    const requests: SurfaceOperationRequest<unknown>[] = [];
    const api = createSupportTicketApi(createFakeClient(requests));

    await api.close("ticket-1", { idempotencyKey: "close-1" });

    expect(requests[0]).toMatchObject({
      input: { ticketId: "ticket-1" },
      operationId: SUPPORT_TICKET_OPERATION_IDS.close,
      requestContext: { idempotencyKey: "close-1" },
    });
  });

  test("covers list and update through the same generated operation client", async () => {
    const requests: SurfaceOperationRequest<unknown>[] = [];
    const api = createSupportTicketApi(createFakeClient(requests));

    await api.list({ cursor: "cursor-1", limit: 25 });
    await api.update("ticket-1", { status: "pending", title: "Waiting" });

    expect(requests).toMatchObject([
      {
        input: { cursor: "cursor-1", limit: 25 },
        operationId: SUPPORT_TICKET_OPERATION_IDS.list,
      },
      {
        input: { status: "pending", ticketId: "ticket-1", title: "Waiting" },
        operationId: SUPPORT_TICKET_OPERATION_IDS.update,
      },
    ]);
  });

  test("keeps grant-denied detail and Module-denied restricted detail as distinct contract operations", async () => {
    const requests: SurfaceOperationRequest<unknown>[] = [];
    const api = createSupportTicketApi(createFakeClient(requests));

    await api.detail("ticket-1");
    await api.restrictedDetail("ticket-1");

    expect(requests).toMatchObject([
      {
        input: { ticketId: "ticket-1" },
        operationId: SUPPORT_TICKET_OPERATION_IDS.detail,
      },
      {
        input: { ticketId: "ticket-1" },
        operationId: SUPPORT_TICKET_OPERATION_IDS.restrictedDetail,
      },
    ]);
    expect(
      supportTicketContract.paths["/tickets/{ticketId}"].get.operationId
    ).toBe(SUPPORT_TICKET_OPERATION_IDS.detail);
    expect(
      supportTicketContract.paths["/tickets/{ticketId}/restricted"].get
        .operationId
    ).toBe(SUPPORT_TICKET_OPERATION_IDS.restrictedDetail);
    expect(
      supportTicketContract.paths["/tickets/{ticketId}/restricted"].get
        .responses["403"]
    ).toEqual({ $ref: "#/components/responses/ProblemDetails" });
    expect(
      supportTicketContract.components.responses.ProblemDetails.content[
        "application/problem+json"
      ].schema
    ).toEqual({ $ref: "#/components/schemas/ProblemDetails" });
  });
});
