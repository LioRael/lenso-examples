import grpc from "@grpc/grpc-js";
import protoLoader from "@grpc/proto-loader";
import {
  adminSchema,
  defineRemoteModule,
  defineSchemaEntity,
  getRoute,
  runtimeFunction,
  textField,
} from "@lenso/remote-module-kit";
import { fileURLToPath } from "node:url";

export const notes = [
  {
    id: "note_1",
    title: "First gRPC note",
    body: "Served through lenso.remote.v1.RemoteModule.",
  },
  {
    id: "note_2",
    title: "Host owned policy",
    body: "The host still owns auth, retries, stories, and operations.",
  },
];

const notesEntity = defineSchemaEntity({
  fields: [
    textField("id", { label: "ID" }),
    textField("title", { label: "Title" }),
    textField("body", { label: "Body" }),
  ],
  label: "Notes",
  name: "notes",
  readCapability: "grpc-notes:notes:read",
});

export const manifest = defineRemoteModule({
  admin: adminSchema([notesEntity]),
  capabilities: ["grpc-notes:notes:read", "grpc-notes:notes:write"],
  httpRoutes: [
    getRoute("/notes", {
      capability: "grpc-notes:notes:read",
      displayName: "List notes",
      storyTitle: "gRPC notes listed",
    }),
  ],
  name: "grpc-notes",
  runtimeFunctions: [
    runtimeFunction("grpc-notes.summarize.v1", {
      queue: "grpc-notes",
    }),
  ],
  version: "0.1.0",
});

const protoPath = fileURLToPath(
  new URL("../proto/lenso/remote/v1/remote_module.proto", import.meta.url)
);

const remoteModuleService = () => {
  const definition = protoLoader.loadSync(protoPath, {
    defaults: true,
    enums: String,
    keepCase: true,
    longs: String,
    oneofs: true,
  });
  return grpc.loadPackageDefinition(definition).lenso.remote.v1.RemoteModule;
};

const parsePayload = (call) =>
  JSON.parse(call.request.payload_json || "{}");

const sendPayload = (callback, payload) => {
  callback(null, { payload_json: JSON.stringify(payload) });
};

const grpcError = (code, message) => Object.assign(new Error(message), { code });

const unaryJson = (handler) => (call, callback) => {
  try {
    sendPayload(callback, handler(parsePayload(call)));
  } catch (error) {
    callback(
      error.code
        ? error
        : grpcError(grpc.status.INTERNAL, error.message || String(error))
    );
  }
};

const noteById = (id) => notes.find((note) => note.id === id);

const assertNotesEntity = (entity) => {
  if (entity !== "notes") {
    throw grpcError(grpc.status.NOT_FOUND, `admin entity ${entity} was not found`);
  }
};

const listNotes = ({ cursor, entity, limit = 50 }) => {
  assertNotesEntity(entity);
  const start = cursor
    ? Math.max(notes.findIndex((note) => note.id === cursor) + 1, 0)
    : 0;
  const records = notes.slice(start, start + Math.min(Math.max(limit, 1), 100));
  const next = start + records.length < notes.length ? records.at(-1)?.id : null;
  return { next_cursor: next, records };
};

const getNote = ({ entity, id }) => {
  assertNotesEntity(entity);
  return { record: noteById(id) ?? null };
};

const proxyNotes = ({ method, remote_path }) => {
  if (method !== "GET" || remote_path !== "/notes") {
    throw grpcError(
      grpc.status.NOT_FOUND,
      `${method} ${remote_path} was not declared`
    );
  }
  return { body: { notes }, status_code: 200 };
};

const summarizeNotes = ({ function_name, input = {}, request_id }) => {
  if (function_name !== "grpc-notes.summarize.v1") {
    throw grpcError(
      grpc.status.NOT_FOUND,
      `runtime function ${function_name} was not found`
    );
  }
  return {
    output: {
      count: notes.length,
      requested_by: input.requested_by ?? "smoke",
      request_id,
      titles: notes.map((note) => note.title),
    },
  };
};

export const createGrpcNotesClient = (baseUrl) => {
  const target = baseUrl.replace(/^grpc:\/\//u, "").replace(/^grpcs:\/\//u, "");
  return new (remoteModuleService())(
    target,
    grpc.credentials.createInsecure()
  );
};

export const serveGrpcNotesModule = async ({ onReady, port = 50051 } = {}) => {
  const server = new grpc.Server();
  server.addService(remoteModuleService().service, {
    GetManifest: unaryJson(() => manifest),
    GetAdminRecord: unaryJson(getNote),
    HandleEvent: unaryJson(() => ({ actions: [] })),
    InvokeAdminAction: unaryJson(() => {
      throw grpcError(grpc.status.NOT_FOUND, "admin actions are not declared");
    }),
    InvokeFunction: unaryJson(summarizeNotes),
    ListAdminRecords: unaryJson(listNotes),
    ProxyHttpRoute: unaryJson(proxyNotes),
  });

  return new Promise((resolve, reject) => {
    server.bindAsync(
      `127.0.0.1:${port}`,
      grpc.ServerCredentials.createInsecure(),
      (error, boundPort) => {
        if (error) {
          reject(error);
          return;
        }
        const baseUrl = `grpc://127.0.0.1:${boundPort}`;
        onReady?.({ baseUrl, port: boundPort });
        resolve({
          baseUrl,
          close: () =>
            new Promise((closeResolve, closeReject) => {
              server.tryShutdown((closeError) => {
                if (closeError) {
                  closeReject(closeError);
                  return;
                }
                closeResolve();
              });
            }),
          port: boundPort,
        });
      }
    );
  });
};
