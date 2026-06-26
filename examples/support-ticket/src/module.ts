import {
  actionTextField,
  actionTimestampField,
  adminAction,
  adminSchema,
  declarativeCustom,
  declarativePage,
  declarativeSection,
  defineRemoteModule,
  defineSchemaEntity,
  entityTable,
  getRoute,
  patchRoute,
  postRoute,
  runtimeFunction,
  serveRemoteModule,
  textField,
  timestampField,
} from "@lenso/remote-module-kit";

const tickets = [
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

const readCapability = "support_ticket.tickets.read";
const writeCapability = "support_ticket.tickets.write";
const escalateCapability = "support_ticket.tickets.escalate";
const statusCapability = "support_ticket.service.status";

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
  remote_protocol_version: "1",
  required_host_features: ["service.status"],
};

const serviceMetadata = {
  deployment: {
    commands: ["pnpm --dir examples/support-ticket start"],
    target: "container-paas",
  },
  name: "api",
  required_env: ["PORT"],
  status_path: "/lenso/module/v1/status",
  transports: ["http"],
  version: "0.1.0",
};

const serviceStatus = (baseUrl = "http://127.0.0.1:4110/lenso/module/v1") => ({
  checks: [{ name: "service", status: "ok" }],
  manifestUrl: `${baseUrl}/manifest`,
  moduleName: "support-ticket",
  protocolVersion: "1",
  serviceName: "api",
  state: "ready",
  transports: ["http"],
  version: "0.1.0",
});

export const manifest = {
  ...defineRemoteModule({
    admin: declarativeCustom({
      actions: [
        adminAction("assign_ticket", {
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
    capabilities: [
      readCapability,
      writeCapability,
      escalateCapability,
      statusCapability,
    ],
    httpRoutes: [
      getRoute("/status", {
        capability: statusCapability,
        displayName: "Service status",
        storyTitle: "Support ticket service status",
      }),
      getRoute("/tickets/{id}", {
        capability: readCapability,
        displayName: "Get ticket",
        storyTitle: "Support ticket viewed",
      }),
      postRoute("/tickets", {
        capability: writeCapability,
        displayName: "Create ticket",
        storyTitle: "Support ticket created",
      }),
      patchRoute("/tickets/{id}", {
        capability: writeCapability,
        displayName: "Update ticket",
        storyTitle: "Support ticket updated",
      }),
    ],
    name: "support-ticket",
    runtimeFunctions: [
      runtimeFunction("support-ticket.escalate-ticket.v1", {
        queue: "support-ticket",
      }),
    ],
    version: "0.1.0",
  }),
  compatibility: serviceCompatibility,
  service: serviceMetadata,
};

const now = () => new Date().toISOString();

const textOrDefault = (value, fallback) =>
  typeof value === "string" && value.trim() ? value.trim() : fallback;

const findTicket = (id) => tickets.find((ticket) => ticket.id === id);

const createTicket = (input = {}) => {
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

const updateTicket = (id, input = {}) => {
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

const assignTicket = (input = {}) =>
  updateTicket(input.ticket_id, {
    assignee: input.assignee,
    updated_at: input.updated_at,
  });

const escalateTicket = (input = {}) =>
  updateTicket(input.ticket_id, {
    priority: "high",
    status: "escalated",
    updated_at: input.updated_at,
  });

const ticketDataSource = {
  detail: async (id) => findTicket(id),
  list: async ({ limit }) => ({
    next_cursor: null,
    records: tickets.slice(0, limit),
  }),
};

export const serveSupportTicketModule = async (options = {}) =>
  serveRemoteModule(manifest, {
    actions: {
      assign_ticket: ({ input }) => ({ ticket: assignTicket(input) }),
    },
    data: {
      tickets: ticketDataSource,
    },
    http: {
      "GET /status": ({ url }) =>
        serviceStatus(`${url.origin}/lenso/module/v1`),
      "GET /tickets/{id}": ({ params }) => ({ ticket: findTicket(params.id) }),
      "PATCH /tickets/{id}": ({ body, params }) => ({
        ticket: updateTicket(params.id, body),
      }),
      "POST /tickets": ({ body }) => ({
        body: { ticket: createTicket(body) },
        statusCode: 201,
      }),
    },
    onReady: options.onReady,
    port: options.port ?? 4110,
    runtime: {
      "support-ticket.escalate-ticket.v1": ({ input }) =>
        escalateTicket(input),
    },
  });
