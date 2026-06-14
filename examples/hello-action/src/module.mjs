import {
  defineRemoteModule,
  defineSchemaEntity,
  getRoute,
  postRoute,
  runtimeFunction,
  schemaAdmin,
  serveRemoteModule,
  textField,
  timestampField,
} from "@lenso/remote-module-kit";

const greetings = [
  {
    id: "greeting_1",
    message: "Hello from a remote Lenso module.",
    recipient: "example-user",
    sent_at: "2026-06-14T00:00:00Z",
  },
];

const writeCapability = "hello-action:greetings:write";

export const manifest = defineRemoteModule({
  admin: schemaAdmin([
    defineSchemaEntity({
      fields: [
        textField("id", { label: "ID" }),
        textField("recipient", { label: "Recipient" }),
        textField("message", { label: "Message" }),
        timestampField("sent_at", { label: "Sent At" }),
      ],
      label: "Greetings",
      name: "greetings",
      readCapability: "hello-action:greetings:read",
    }),
  ]),
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

const greetingDataSource = {
  detail: async (id) => greetings.find((greeting) => greeting.id === id),
  list: async ({ limit }) => ({
    next_cursor: null,
    records: greetings.slice(0, limit),
  }),
};

const sayHello = (name) => ({
  message: `Hello, ${name || "Lenso"}.`,
  module: manifest.name,
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
  serveRemoteModule(manifest, {
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
    onReady: options.onReady,
    port: options.port ?? 4100,
    runtime: {
      "hello-action.say-hello.v1": ({ input }) =>
        sayHello(input?.name ?? "runtime"),
      "hello-action.record-greeting.v1": ({ input }) => recordGreeting(input),
    },
  });
