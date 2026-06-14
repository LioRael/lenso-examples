#!/usr/bin/env node
import { readFile } from "node:fs/promises";

import { createClient } from "@lenso/ts-sdk";

import { serveHelloActionModule } from "./module.mjs";

const fetchJson = async (url, init) => {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  return response.json();
};

const server = await serveHelloActionModule({ port: 0 });

try {
  const catalogEntry = JSON.parse(
    await readFile(new URL("../catalog-entry.json", import.meta.url), "utf8")
  );
  if (
    catalogEntry.name !== "hello-action" ||
    catalogEntry.source !== "remote" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4100/lenso/module/v1"
  ) {
    throw new Error("catalog entry does not describe hello-action");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "hello-action" ||
    manifest.source !== "remote" ||
    manifest.version !== catalogEntry.version
  ) {
    throw new Error("manifest did not return hello-action");
  }
  for (const capability of catalogEntry.capabilities) {
    if (!manifest.capabilities.includes(capability)) {
      throw new Error(`manifest is missing catalog capability ${capability}`);
    }
  }

  const hello = await fetchJson(`${server.baseUrl}/hello/Ada`);
  if (hello.message !== "Hello, Ada.") {
    throw new Error("HTTP route did not return the expected greeting");
  }

  const recordedByHttp = await fetchJson(`${server.baseUrl}/greetings`, {
    body: JSON.stringify({
      message: "Hello from HTTP mutation.",
      recipient: "http-user",
      sent_at: "2026-06-14T01:00:00Z",
    }),
    headers: { "content-type": "application/json" },
    method: "POST",
  });
  if (recordedByHttp.record?.recipient !== "http-user") {
    throw new Error("HTTP mutation did not record the expected greeting");
  }

  const runtime = await fetchJson(
    `${server.baseUrl}/runtime/functions/hello-action.say-hello.v1/invoke`,
    {
      body: JSON.stringify({
        actor: { id: "example-smoke", kind: "service", scopes: [] },
        attempt: 1,
        correlation_id: "corr_example_smoke",
        function_name: "hello-action.say-hello.v1",
        function_run_id: "fnrun_example_smoke",
        input: { name: "Runtime" },
        request_id: "req_example_smoke",
        trace: { span_id: "span_example_smoke", trace_id: "trace_example_smoke" },
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (runtime.output?.message !== "Hello, Runtime.") {
    throw new Error("runtime function did not return the expected greeting");
  }

  const runtimeMutation = await fetchJson(
    `${server.baseUrl}/runtime/functions/hello-action.record-greeting.v1/invoke`,
    {
      body: JSON.stringify({
        actor: { id: "example-smoke", kind: "service", scopes: [] },
        attempt: 1,
        correlation_id: "corr_example_mutation_smoke",
        function_name: "hello-action.record-greeting.v1",
        function_run_id: "fnrun_example_mutation_smoke",
        input: {
          message: "Hello from runtime mutation.",
          recipient: "runtime-user",
          sent_at: "2026-06-14T02:00:00Z",
        },
        request_id: "req_example_mutation_smoke",
        trace: {
          span_id: "span_example_mutation_smoke",
          trace_id: "trace_example_mutation_smoke",
        },
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (runtimeMutation.output?.recipient !== "runtime-user") {
    throw new Error("runtime mutation did not record the expected greeting");
  }

  const admin = await fetchJson(`${server.baseUrl}/admin/greetings`);
  const recipients = admin.records?.map((record) => record.recipient) ?? [];
  if (
    !recipients.includes("example-user") ||
    !recipients.includes("http-user") ||
    !recipients.includes("runtime-user")
  ) {
    throw new Error("schema-admin endpoint did not return greetings");
  }

  const client = createClient({ baseUrl: "http://127.0.0.1:3000" });
  if (typeof client.identity.createUser !== "function") {
    throw new Error("@lenso/ts-sdk did not expose createClient");
  }

  console.log("Hello Action remote module smoke passed");
} finally {
  await server.close();
}
