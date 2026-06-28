import {
  actionTextField,
  actionTimestampField,
  adminAction,
  adminSchema,
  declarativeCustom,
  declarativePage,
  declarativeSection,
  defineModule,
  defineSchemaEntity,
  defineService,
  entityTable,
  getRoute,
  postRoute,
  runtimeFunction,
  serveService,
  textField,
  timestampField,
} from "@lenso/service-kit";

const greetings = [
  {
    id: "greeting_1",
    message: "Hello from a Lenso service-provided module.",
    recipient: "example-user",
    sent_at: "2026-06-14T00:00:00Z",
  },
];

const writeCapability = "hello-action:greetings:write";

const greetingsEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("recipient", { label: "Recipient" }),
    textField("message", { label: "Message" }),
    timestampField("sent_at", { label: "Sent At" }),
  ],
  label: "Greetings",
  name: "greetings",
  readCapability: "hello-action:greetings:read",
});

const serviceCompatibility = {
  console_package_api: "1",
  remote_protocol_version: "1",
  required_host_features: ["service.status"],
};

const serviceDeployment = {
  commands: ["pnpm --dir examples/hello-action start"],
  target: "local-node",
};

export const helloActionModule = defineModule({
  admin: declarativeCustom({
    actions: [
      adminAction("seed_greeting", {
        capability: writeCapability,
        inputFields: [
          actionTextField("recipient", {
            description: "Who should receive the seeded greeting.",
            label: "Recipient",
            required: true,
          }),
          actionTextField("message", {
            description: "Greeting message to store.",
            label: "Message",
            required: true,
          }),
          actionTimestampField("sent_at", { label: "Sent At" }),
        ],
        label: "Seed greeting",
      }),
    ],
    fallbackSchema: adminSchema([greetingsEntity]),
    pages: [
      declarativePage("greetings", {
        sections: [
          declarativeSection("records", {
            component: entityTable("greetings"),
            label: "Greeting records",
          }),
        ],
      }),
    ],
  }),
  capabilities: [
    "hello-action:greetings:read",
    "hello-action:hello:read",
    writeCapability,
  ],
  httpRoutes: [
    getRoute("/hello/{name}", {
      capability: "hello-action:hello:read",
      displayName: "Say hello",
      storyTitle: "Hello action request",
    }),
    postRoute("/greetings", {
      capability: writeCapability,
      displayName: "Record greeting",
      storyTitle: "Greeting recorded",
    }),
  ],
  name: "hello-action",
  runtimeFunctions: [
    runtimeFunction("hello-action.say-hello.v1", {
      queue: "hello-action",
    }),
    runtimeFunction("hello-action.record-greeting.v1", {
      queue: "hello-action",
    }),
  ],
  version: "0.1.0",
});

export const manifest = defineService({
  compatibility: serviceCompatibility,
  deployment: serviceDeployment,
  install: {
    services: [
      {
        command: "pnpm --dir examples/hello-action start",
        name: "hello-service",
      },
    ],
  },
  modules: [helloActionModule],
  name: "hello-service",
  requiredEnv: ["PORT"],
  statusPath: "/lenso/service/v1/status",
  transports: ["http"],
  version: "0.1.0",
});

const greetingDataSource = {
  detail: async (id) => greetings.find((greeting) => greeting.id === id),
  list: async ({ limit }) => ({
    next_cursor: null,
    records: greetings.slice(0, limit),
  }),
};

const sayHello = (name) => ({
  message: `Hello, ${name || "Lenso"}.`,
  module: helloActionModule.name,
});

const textOrDefault = (value, fallback) =>
  typeof value === "string" && value.trim() ? value.trim() : fallback;

const recordGreeting = (input = {}) => {
  const greeting = {
    id: `greeting_${greetings.length + 1}`,
    message: textOrDefault(input.message, "Hello from a mutable action."),
    recipient: textOrDefault(input.recipient, "example-user"),
    sent_at: textOrDefault(input.sent_at, new Date().toISOString()),
  };
  greetings.push(greeting);
  return greeting;
};

export const serveHelloActionModule = async (options = {}) =>
  serveService(manifest, {
    modules: {
      "hello-action": {
        actions: {
          seed_greeting: ({ input }) => ({ record: recordGreeting(input) }),
        },
        data: {
          greetings: greetingDataSource,
        },
        http: {
          "GET /hello/{name}": ({ params }) => sayHello(params.name),
          "POST /greetings": ({ body }) => ({
            body: { record: recordGreeting(body) },
            statusCode: 201,
          }),
        },
        runtime: {
          "hello-action.record-greeting.v1": ({ input }) =>
            recordGreeting(input),
          "hello-action.say-hello.v1": ({ input }) =>
            sayHello(input?.name ?? "runtime"),
        },
      },
    },
    onReady: options.onReady,
    port: options.port ?? 4100,
  });
