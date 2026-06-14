#!/usr/bin/env node
import { createClient } from "@lenso/ts-sdk";

import { serveHelloActionModule } from "./module.mjs";

const server = await serveHelloActionModule({ port: 0 });

try {
  const manifest = await fetch(server.manifestUrl).then((response) =>
    response.json()
  );
  if (manifest.name !== "hello-action") {
    throw new Error("manifest did not return hello-action");
  }

  const hello = await fetch(`${server.baseUrl}/hello/Ada`).then((response) =>
    response.json()
  );
  if (hello.message !== "Hello, Ada.") {
    throw new Error("HTTP route did not return the expected greeting");
  }

  const runtime = await fetch(
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
  ).then((response) => response.json());
  if (runtime.output?.message !== "Hello, Runtime.") {
    throw new Error("runtime function did not return the expected greeting");
  }

  const admin = await fetch(`${server.baseUrl}/admin/greetings`).then(
    (response) => response.json()
  );
  if (admin.records?.[0]?.recipient !== "example-user") {
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
