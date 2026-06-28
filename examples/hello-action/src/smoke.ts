#!/usr/bin/env node
import { readFile } from "node:fs/promises";

import { serveHelloActionModule } from "./module.ts";

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
    catalogEntry.name !== "hello-service" ||
    catalogEntry.source !== "service" ||
    catalogEntry.baseUrl !== "http://127.0.0.1:4100/lenso/service/v1"
  ) {
    throw new Error("catalog entry does not describe hello-service");
  }

  const manifest = await fetchJson(server.manifestUrl);
  if (
    manifest.name !== "hello-service" ||
    manifest.protocol !== "lenso.service.v1" ||
    manifest.version !== catalogEntry.version
  ) {
    throw new Error("manifest did not return hello-service");
  }
  const moduleManifest = manifest.modules?.find(
    (module) => module.name === "hello-action"
  );
  if (!moduleManifest) {
    throw new Error("service manifest did not provide hello-action");
  }
  for (const capability of catalogEntry.capabilities) {
    if (!moduleManifest.capabilities.includes(capability)) {
      throw new Error(`hello-action is missing catalog capability ${capability}`);
    }
  }
  if (
    moduleManifest.admin?.kind !== "declarative_custom" ||
    !moduleManifest.admin.actions?.some((action) => action.name === "seed_greeting")
  ) {
    throw new Error("hello-action did not declare the seed_greeting admin action");
  }
  if (!moduleManifest.admin.fallback_schema?.entities?.length) {
    throw new Error("hello-action did not expose a fallback schema");
  }

  const status = await fetchJson(server.statusUrl ?? `${server.baseUrl}/status`);
  if (
    status.serviceName !== "hello-service" ||
    status.state !== "ready" ||
    !status.modules?.some((module) => module.name === "hello-action")
  ) {
    throw new Error("status endpoint did not return hello-service readiness");
  }

  const moduleBaseUrl = `${server.baseUrl}/modules/hello-action`;
  const hello = await fetchJson(`${moduleBaseUrl}/hello/Ada`);
  if (hello.message !== "Hello, Ada.") {
    throw new Error("HTTP route did not return the expected greeting");
  }

  const recordedByHttp = await fetchJson(`${moduleBaseUrl}/greetings`, {
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
    `${moduleBaseUrl}/runtime/functions/hello-action.say-hello.v1/invoke`,
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
    `${moduleBaseUrl}/runtime/functions/hello-action.record-greeting.v1/invoke`,
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

  const seededByAction = await fetchJson(
    `${moduleBaseUrl}/admin/actions/seed_greeting`,
    {
      body: JSON.stringify({
        message: "Hello from admin action.",
        recipient: "admin-action-user",
        sent_at: "2026-06-14T03:00:00Z",
      }),
      headers: { "content-type": "application/json" },
      method: "POST",
    }
  );
  if (seededByAction.result?.record?.recipient !== "admin-action-user") {
    throw new Error("admin action did not record the expected greeting");
  }

  const admin = await fetchJson(`${moduleBaseUrl}/admin/greetings`);
  const recipients = admin.records?.map((record) => record.recipient) ?? [];
  if (
    !recipients.includes("example-user") ||
    !recipients.includes("http-user") ||
    !recipients.includes("runtime-user") ||
    !recipients.includes("admin-action-user")
  ) {
    throw new Error("schema-admin endpoint did not return greetings");
  }

  console.log("Hello Action service smoke passed");
} finally {
  await server.close();
}
