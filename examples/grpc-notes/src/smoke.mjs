#!/usr/bin/env node
import { readFile } from "node:fs/promises";

import {
  createGrpcNotesClient,
  manifest,
  serveGrpcNotesModule,
} from "./module.mjs";

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

const callGrpc = (client, method, payload = {}) =>
  new Promise((resolve, reject) => {
    const call = client[method] ?? client[`${method[0].toLowerCase()}${method.slice(1)}`];
    call.call(
      client,
      { payload_json: JSON.stringify(payload) },
      (error, response) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(JSON.parse(response.payload_json || "{}"));
      }
    );
  });

const checkedInManifest = JSON.parse(
  await readFile(new URL("../lenso.module.json", import.meta.url), "utf8")
);
assert(
  JSON.stringify(checkedInManifest) === JSON.stringify(manifest),
  "lenso.module.json drifted from src/module.mjs"
);

const server = await serveGrpcNotesModule({ port: 0 });
const client = createGrpcNotesClient(server.baseUrl);

try {
  const manifestResponse = await callGrpc(client, "GetManifest");
  assert(manifestResponse.name === "grpc-notes", "manifest name mismatch");

  const list = await callGrpc(client, "ListAdminRecords", {
    entity: "notes",
    limit: 1,
  });
  assert(list.records?.[0]?.id === "note_1", "admin list did not return note_1");
  assert(list.next_cursor === "note_1", "admin list cursor mismatch");

  const detail = await callGrpc(client, "GetAdminRecord", {
    entity: "notes",
    id: "note_2",
  });
  assert(detail.record?.title === "Host owned policy", "admin detail mismatch");

  const proxy = await callGrpc(client, "ProxyHttpRoute", {
    declared_path: "/notes",
    headers: {},
    method: "GET",
    module_name: "grpc-notes",
    path_params: {},
    remote_path: "/notes",
    request_id: "req_grpc_notes_smoke",
    correlation_id: "corr_grpc_notes_smoke",
  });
  assert(proxy.status_code === 200, "proxy status mismatch");
  assert(proxy.body?.notes?.length === 2, "proxy body mismatch");

  const runtime = await callGrpc(client, "InvokeFunction", {
    actor: { id: "example-smoke", kind: "service", scopes: [] },
    attempt: 1,
    correlation_id: "corr_grpc_notes_runtime_smoke",
    function_name: "grpc-notes.summarize.v1",
    function_run_id: "fnrun_grpc_notes_smoke",
    input: { requested_by: "grpc-smoke" },
    request_id: "req_grpc_notes_runtime_smoke",
    trace: {
      span_id: "span_grpc_notes_runtime_smoke",
      trace_id: "trace_grpc_notes_runtime_smoke",
    },
  });
  assert(runtime.output?.count === 2, "runtime count mismatch");
  assert(
    runtime.output?.requested_by === "grpc-smoke",
    "runtime input echo mismatch"
  );

  console.log("gRPC Notes remote module smoke passed");
} finally {
  client.close();
  await server.close();
}
