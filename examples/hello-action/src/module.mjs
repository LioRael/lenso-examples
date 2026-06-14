import {
  defineRemoteModule,
  defineSchemaEntity,
  getRoute,
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
  capabilities: ["hello-action:greetings:read", "hello-action:hello:read"],
  httpRoutes: [
    getRoute("/hello/{name}", {
      capability: "hello-action:hello:read",
      displayName: "Say hello",
      storyTitle: "Hello action request",
    }),
  ],
  name: "hello-action",
  runtimeFunctions: [
    runtimeFunction("hello-action.say-hello.v1", {
      queue: "hello-action",
    }),
  ],
  version: "0.1.0",
});

const greetingDataSource = {
  detail: async (id) => greetings.find((greeting) => greeting.id === id),
  list: async () => ({
    next_cursor: null,
    records: greetings,
  }),
};

const sayHello = (name) => ({
  message: `Hello, ${name || "Lenso"}.`,
  module: manifest.name,
});

export const serveHelloActionModule = async (options = {}) =>
  serveRemoteModule(manifest, {
    data: {
      greetings: greetingDataSource,
    },
    http: {
      "GET /hello/{name}": ({ params }) => sayHello(params.name),
    },
    onReady: options.onReady,
    port: options.port ?? 4100,
    runtime: {
      "hello-action.say-hello.v1": ({ input }) =>
        sayHello(input?.name ?? "runtime"),
    },
  });
