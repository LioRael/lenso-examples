import { appendFileSync, readFileSync } from "node:fs";
import type { IncomingMessage } from "node:http";

import {
  actionTextField,
  actionTimestampField,
  adminAction,
  adminSchema,
  declarativeCustom,
  declarativePage,
  declarativeSection,
  defineKubernetesDeployment,
  defineModule,
  defineService,
  defineSchemaEntity,
  entityTable,
  getRoute,
  patchRoute,
  postRoute,
  runtimeFunction,
  serveService,
  textField,
  timestampField,
} from "@lenso/service-kit";
import type {
  ServedService,
  ServeServiceOptions,
} from "@lenso/service-kit";
import {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
} from "./contract.ts";

interface ProviderCoreOptions {
  bearerToken: string;
  serviceId: string;
  servicePrincipal: string;
  serviceRevision: string;
}

interface ProviderDeliveryOptions extends ServeServiceOptions {
  providerCore?: ProviderCoreOptions;
}

interface SupportTicket {
  assignee: string;
  created_at: string;
  id: string;
  priority: string;
  status: string;
  title: string;
  updated_at: string;
}

interface SupportTicketInput {
  assignee?: unknown;
  created_at?: unknown;
  priority?: unknown;
  status?: unknown;
  ticket_id?: unknown;
  title?: unknown;
  updated_at?: unknown;
}

interface ServeSupportTicketOptions {
  onReady?: (server: ServedService) => void;
  port?: number;
}

interface ServedSupportTicket extends ServedService {
  systemPlaneCoreUrl?: string;
}

interface AcceptanceRule {
  capability: string;
  operationIds: readonly string[];
  requiresIdempotency: boolean;
}

interface SurfaceStoryContext {
  correlationId?: string;
  segmentId?: string;
  storyId: string;
}

const tickets: SupportTicket[] = [
  {
    assignee: "support-lead",
    created_at: "2026-06-20T00:00:00Z",
    id: "ticket_1",
    priority: "normal",
    status: "open",
    title: "Cannot invite a teammate",
    updated_at: "2026-06-20T00:00:00Z",
  },
];
const acceptedCreates = new Map<
  string,
  { fingerprint: string; ticket: SupportTicket }
>();

const readCapability = "support_ticket.tickets.read";
const writeCapability = "support_ticket.tickets.write";
const escalateCapability = "support_ticket.tickets.escalate";
const supportTicketContractDigest = SUPPORT_TICKET_CONTRACT_DIGEST;
const supportTicketOperations = SUPPORT_TICKET_OPERATION_IDS;

const ticketsEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("title", { label: "Title" }),
    textField("status", { label: "Status" }),
    textField("priority", { label: "Priority" }),
    textField("assignee", { label: "Assignee" }),
    timestampField("created_at", { label: "Created At" }),
    timestampField("updated_at", { label: "Updated At" }),
  ],
  label: "Tickets",
  name: "tickets",
  readCapability,
});

const serviceCompatibility = {
  console_package_api: "1",
  providerProtocolVersion: "1",
  requiredHostFeatures: ["service.status"],
};

const serviceDeployment = {
  commands: ["pnpm --dir examples/support-ticket start"],
  ...defineKubernetesDeployment({
    ingressHost: "support-staging.example.com",
    port: 4110,
    replicas: 2,
    secrets: ["SUPPORT_TICKET_TOKEN"],
  }),
};

export const supportTicketModule = defineModule({
  admin: declarativeCustom({
    actions: [
      {
        ...adminAction("assign_ticket", {
          capability: writeCapability,
          inputFields: [
            actionTextField("ticket_id", {
              label: "Ticket ID",
              required: true,
            }),
            actionTextField("assignee", { label: "Assignee", required: true }),
            actionTimestampField("updated_at", { label: "Updated At" }),
          ],
          label: "Assign ticket",
        }),
        operation: {
          operationId: "support-ticket/action/assign_ticket",
        },
      },
    ],
    fallbackSchema: adminSchema([ticketsEntity]),
    pages: [
      declarativePage("tickets", {
        sections: [
          declarativeSection("records", {
            component: entityTable("tickets"),
            label: "Support tickets",
          }),
        ],
      }),
    ],
  }),
  capabilities: [readCapability, writeCapability, escalateCapability],
  httpRoutes: [
    {
      ...getRoute("/tickets", {
        capability: readCapability,
        displayName: "List tickets",
        storyTitle: "Support tickets listed",
      }),
      operation: {
        operationId: supportTicketOperations.list,
        safeProbe: {
          expectStatus: 200,
          method: "GET",
          path: "/tickets",
        },
      },
    },
    getRoute("/tickets/{id}", {
      capability: readCapability,
      displayName: "Get ticket",
      storyTitle: "Support ticket viewed",
    }),
    {
      ...postRoute("/tickets", {
        capability: writeCapability,
        displayName: "Create ticket",
        storyTitle: "Support ticket created",
      }),
      operation: { operationId: supportTicketOperations.create },
    },
    {
      ...patchRoute("/tickets/{id}", {
        capability: writeCapability,
        displayName: "Update ticket",
        storyTitle: "Support ticket updated",
      }),
      operation: { operationId: supportTicketOperations.update },
    },
  ],
  name: "support-ticket",
  runtimeFunctions: [
    {
      ...runtimeFunction("support-ticket.escalate-ticket.v1", {
        queue: "support-ticket",
      }),
      operation: {
        operationId:
          "support-ticket/runtime/support-ticket.escalate-ticket.v1",
      },
    },
  ],
  version: "0.1.0",
});

export const supportNotificationModule = defineModule({
  capabilities: ["support_notification.notifications.send"],
  httpRoutes: [
    {
      ...postRoute("/notifications/ticket-update", {
        capability: "support_notification.notifications.send",
        displayName: "Send ticket update",
        storyTitle: "Support notification sent",
      }),
      operation: {
        operationId:
          "support-notification/http/POST:/notifications/ticket-update",
      },
    },
  ],
  name: "support-notification",
  runtimeFunctions: [
    runtimeFunction("support-notification.send-ticket-update.v1", {
      queue: "support-ticket",
    }),
  ],
  version: "0.1.0",
});

export const supportKnowledgeBaseModule = defineModule({
  capabilities: ["support_knowledge_base.articles.read"],
  httpRoutes: [
    getRoute("/articles/{id}", {
      capability: "support_knowledge_base.articles.read",
      displayName: "Get article",
      storyTitle: "Support article viewed",
    }),
  ],
  name: "support-knowledge-base",
  runtimeFunctions: [
    {
      ...runtimeFunction("support-knowledge-base.refresh-index.v1", {
        queue: "support-ticket",
      }),
      operation: {
        operationId:
          "support-knowledge-base/runtime/support-knowledge-base.refresh-index.v1",
      },
    },
  ],
  version: "0.1.0",
});

export const manifest = defineService({
  compatibility: serviceCompatibility,
  deployment: serviceDeployment,
  install: {
    services: [
      {
        command: "pnpm --dir examples/support-ticket start",
        name: "support-suite-provider",
      },
    ],
  },
  modules: [
    supportTicketModule,
    supportNotificationModule,
    supportKnowledgeBaseModule,
  ],
  name: "support-suite-provider",
  requiredEnv: [],
  statusPath: "/lenso/service/v1/status",
  transports: ["http"],
  version: "0.1.0",
});

const now = (): string => new Date().toISOString();

const textOrDefault = (value: unknown, fallback: string): string =>
  typeof value === "string" && value.trim() ? value.trim() : fallback;

const findTicket = (id: string) => tickets.find((ticket) => ticket.id === id);

const recordInput = (value: unknown): SupportTicketInput =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as SupportTicketInput)
    : {};

const createTicket = (input: SupportTicketInput = {}) => {
  const timestamp = textOrDefault(input.created_at, now());
  const ticket = {
    assignee: textOrDefault(input.assignee, "unassigned"),
    created_at: timestamp,
    id: `ticket_${tickets.length + 1}`,
    priority: textOrDefault(input.priority, "normal"),
    status: "open",
    title: textOrDefault(input.title, "Untitled support ticket"),
    updated_at: timestamp,
  };
  tickets.push(ticket);
  return ticket;
};

const createTicketIdempotently = (
  request: IncomingMessage | undefined,
  input: SupportTicketInput,
  idempotencyKey: string
) => {
  const actor = strictHeader(request, "x-lenso-console-delegated-actor") ?? "";
  const operationId =
    strictHeader(request, "x-lenso-console-operation-id") ?? "";
  const tenantId = strictHeader(request, "x-lenso-console-tenant-id") ?? "";
  const scope = JSON.stringify([
    tenantId,
    actor,
    operationId,
    idempotencyKey,
  ]);
  const fingerprint = JSON.stringify(
    Object.fromEntries(
      Object.entries(input).sort(([left], [right]) => left.localeCompare(right))
    )
  );
  const accepted = acceptedCreates.get(scope);
  if (accepted) {
    if (accepted.fingerprint !== fingerprint) {
      return {
        body: {
          error: {
            code: "idempotency_conflict",
            message: "The idempotency key was already used for another request",
          },
        },
        statusCode: 409,
      };
    }
    return {
      body: { ticket: structuredClone(accepted.ticket) },
      statusCode: 201,
    };
  }
  const ticket = createTicket(input);
  acceptedCreates.set(scope, { fingerprint, ticket: structuredClone(ticket) });
  return { body: { ticket }, statusCode: 201 };
};

const createTicketAfterAcceptanceValidation = (
  request: IncomingMessage | undefined,
  input: SupportTicketInput
) => {
  const idempotencyKey = strictHeader(request, "idempotency-key");
  if (
    process.env.LENSO_SUPPORT_DESK_ACCEPTANCE !== "1" ||
    !idempotencyKey
  ) {
    return { body: { ticket: createTicket(input) }, statusCode: 201 };
  }
  return createTicketIdempotently(request, input, idempotencyKey);
};

const updateTicket = (id: string, input: SupportTicketInput = {}) => {
  const ticket = findTicket(id);
  if (!ticket) {
    return undefined;
  }
  ticket.assignee = textOrDefault(input.assignee, ticket.assignee);
  ticket.priority = textOrDefault(input.priority, ticket.priority);
  ticket.status = textOrDefault(input.status, ticket.status);
  ticket.title = textOrDefault(input.title, ticket.title);
  ticket.updated_at = textOrDefault(input.updated_at, now());
  return ticket;
};

const assignTicket = (input: SupportTicketInput = {}) =>
  updateTicket(textOrDefault(input.ticket_id, ""), {
    assignee: input.assignee,
    updated_at: input.updated_at,
  });

const escalateTicket = (input: SupportTicketInput = {}) =>
  updateTicket(textOrDefault(input.ticket_id, ""), {
    priority: "high",
    status: "escalated",
    updated_at: input.updated_at,
  });

const ticketDataSource = {
  detail: async (id: string) => findTicket(id),
  list: async ({ limit }: { limit: number }) => ({
    next_cursor: null,
    records: tickets.slice(0, limit),
  }),
};

const providerCoreFromEnvironment = (): ProviderCoreOptions | undefined => {
  const bearerToken = process.env.LENSO_SYSTEM_PLANE_BEARER_TOKEN;
  if (!bearerToken?.trim()) {
    return undefined;
  }
  return {
    bearerToken,
    serviceId: process.env.LENSO_SERVICE_ID ?? "",
    servicePrincipal: process.env.LENSO_SERVICE_PRINCIPAL ?? "",
    serviceRevision: process.env.LENSO_SERVICE_REVISION ?? "",
  };
};

const acceptanceContextError = () => ({
  body: {
    error: {
      code: "invalid_console_surface_context",
      message:
        "Support Ticket acceptance requires an exact Console Surface Gateway context",
    },
  },
  statusCode: 403,
});

const strictText = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() === value && value
    ? value
    : undefined;

const strictHeader = (
  request: IncomingMessage | undefined,
  name: string
): string | undefined => {
  return strictText(request?.headers?.[name]);
};

const parseStoryContext = (
  value: string | undefined
): SurfaceStoryContext | undefined => {
  if (!value) return undefined;
  try {
    const story = JSON.parse(value) as unknown;
    if (!story || typeof story !== "object" || Array.isArray(story)) {
      return undefined;
    }
    const storyRecord = story as Record<string, unknown>;
    const allowed = new Set(["storyId", "segmentId", "correlationId"]);
    if (Object.keys(storyRecord).some((key) => !allowed.has(key))) {
      return undefined;
    }
    for (const key of ["storyId", "segmentId", "correlationId"]) {
      if (
        (key === "storyId" || Object.hasOwn(storyRecord, key)) &&
        (typeof storyRecord[key] !== "string" ||
          storyRecord[key].trim() !== storyRecord[key] ||
          !storyRecord[key])
      ) {
        return undefined;
      }
    }
    return storyRecord as unknown as SurfaceStoryContext;
  } catch {
    return undefined;
  }
};

const strictEnvironment = (name: string): string | undefined => {
  return strictText(process.env[name]);
};

const expectedAcceptanceActor = (): string | undefined => {
  const configured = strictEnvironment("LENSO_ACCEPTANCE_EXPECTED_ACTOR");
  if (configured) return configured;
  const actorFile = strictEnvironment(
    "LENSO_ACCEPTANCE_EXPECTED_ACTOR_FILE"
  );
  if (!actorFile) return undefined;
  try {
    return strictText(readFileSync(actorFile, "utf8").trim());
  } catch {
    return undefined;
  }
};

const validateAcceptanceSurfaceContext = (
  request: IncomingMessage | undefined,
  { capability, operationIds, requiresIdempotency }: AcceptanceRule
) => {
  if (process.env.LENSO_SUPPORT_DESK_ACCEPTANCE !== "1") {
    return undefined;
  }
  const authority = strictHeader(
    request,
    "x-lenso-console-delegated-authority"
  );
  const operationId = strictHeader(request, "x-lenso-console-operation-id");
  const deadline = strictHeader(request, "x-lenso-deadline-unix-ms");
  const deadlineUnixMs = Number(deadline);
  const expectedActor = expectedAcceptanceActor();
  const expectedAuthority = strictEnvironment(
    "LENSO_ACCEPTANCE_EXPECTED_AUTHORITY_DIGEST"
  );
  const expectedTenant = strictEnvironment(
    "LENSO_ACCEPTANCE_EXPECTED_TENANT_ID"
  );
  const expectedStoryId = strictEnvironment(
    "LENSO_ACCEPTANCE_EXPECTED_STORY_ID"
  );
  const expectedServiceId = strictEnvironment("LENSO_SERVICE_ID");
  const storyHeader = request?.headers?.["x-lenso-console-story-context"];
  const tenantHeader = request?.headers?.["x-lenso-console-tenant-id"];
  const story = parseStoryContext(strictText(storyHeader));
  const valid =
    Boolean(
      expectedActor && expectedServiceId && expectedStoryId && expectedTenant
    ) &&
    strictHeader(request, "x-lenso-console-delegated-actor") ===
      expectedActor &&
    /^sha256:[0-9a-f]{64}$/.test(expectedAuthority ?? "") &&
    authority === expectedAuthority &&
    strictHeader(request, "x-lenso-console-service-id") ===
      expectedServiceId &&
    strictHeader(request, "x-lenso-console-contract-digest") ===
      supportTicketContractDigest &&
    operationId !== undefined &&
    operationIds.includes(operationId) &&
    strictHeader(request, "x-lenso-console-capability") === capability &&
    /^[1-9][0-9]*$/.test(deadline ?? "") &&
    Number.isSafeInteger(deadlineUnixMs) &&
    deadlineUnixMs > Date.now() &&
    (tenantHeader === undefined || strictText(tenantHeader) === expectedTenant) &&
    (!requiresIdempotency || Boolean(strictHeader(request, "idempotency-key"))) &&
    (storyHeader === undefined || story?.storyId === expectedStoryId);
  const observationFile = strictEnvironment(
    "LENSO_ACCEPTANCE_OBSERVED_CONTEXT_FILE"
  );
  if (observationFile) {
    appendFileSync(
      observationFile,
      `${JSON.stringify({
        accepted: valid,
        actor: strictHeader(request, "x-lenso-console-delegated-actor"),
        authority,
        capability: strictHeader(request, "x-lenso-console-capability"),
        contractDigest: strictHeader(
          request,
          "x-lenso-console-contract-digest"
        ),
        deadlineUnixMs,
        idempotencyKey: strictHeader(request, "idempotency-key"),
        observedAtUnixMs: Date.now(),
        operationId,
        serviceId: strictHeader(request, "x-lenso-console-service-id"),
        story,
        tenantId: strictText(tenantHeader),
      })}\n`,
      { encoding: "utf8" }
    );
  }
  if (!valid) {
    return acceptanceContextError();
  }
  return undefined;
};

export const serveSupportTicketModule = async (
  options: ServeSupportTicketOptions = {}
): Promise<ServedSupportTicket> => {
  const delivery: ProviderDeliveryOptions = {
    modules: {
      "support-knowledge-base": {
        http: {
          "GET /articles/{id}": ({ params }) => ({
            article: {
              id: params.id,
              title: "Invite teammates",
            },
          }),
        },
        runtime: {
          "support-knowledge-base.refresh-index.v1": () => ({
            indexed: true,
          }),
        },
      },
      "support-notification": {
        http: {
          "POST /notifications/ticket-update": ({ body }) => ({
            delivered: true,
            ticket_id: recordInput(body).ticket_id,
          }),
        },
        runtime: {
          "support-notification.send-ticket-update.v1": ({ input }) => ({
            delivered: true,
            ticket_id: recordInput(input).ticket_id,
          }),
        },
      },
      "support-ticket": {
        actions: {
          assign_ticket: ({ input }) => ({
            ticket: assignTicket(recordInput(input)),
          }),
        },
        data: {
          tickets: ticketDataSource,
        },
        http: {
          "GET /tickets": ({ request }) =>
            validateAcceptanceSurfaceContext(request, {
              capability: readCapability,
              operationIds: [supportTicketOperations.list],
              requiresIdempotency: false,
            }) ?? {
              next_cursor: null,
              records: tickets,
            },
          "GET /tickets/{id}": ({ params }) => ({
            ticket: findTicket(params.id ?? ""),
          }),
          "PATCH /tickets/{id}": ({ body, params, request }) =>
            validateAcceptanceSurfaceContext(request, {
              capability: writeCapability,
              operationIds: [
                supportTicketOperations.update,
                supportTicketOperations.close,
              ],
              requiresIdempotency: true,
            }) ?? { ticket: updateTicket(params.id ?? "", recordInput(body)) },
          "POST /tickets": ({ body, request }) =>
            validateAcceptanceSurfaceContext(request, {
              capability: writeCapability,
              operationIds: [supportTicketOperations.create],
              requiresIdempotency: true,
            }) ?? createTicketAfterAcceptanceValidation(request, recordInput(body)),
        },
        runtime: {
          "support-ticket.escalate-ticket.v1": ({ input }) =>
            escalateTicket(recordInput(input)),
        },
      },
    },
    onReady: options.onReady,
    port: options.port ?? 4110,
    providerCore: providerCoreFromEnvironment(),
    status: {
      checks: [
        { name: "support-knowledge-base", status: "ok" },
        { name: "support-notification", status: "ok" },
        { name: "support-ticket", status: "ok" },
      ],
    },
  };
  return (await serveService(manifest, delivery)) as ServedSupportTicket;
};
