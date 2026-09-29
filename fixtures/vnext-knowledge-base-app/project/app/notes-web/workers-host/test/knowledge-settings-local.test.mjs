import assert from "node:assert/strict";
import test from "node:test";
import { createKnowledgeSettingsLocalWorkerAdapter } from "../knowledge-settings-local.mjs";

const CAPABILITY = "lenso.http.endpoint@1";
const DESCRIPTOR_DIGEST = "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe";
const ARTIFACT_DIGEST = `sha256:${"a".repeat(64)}`;
const WORLD = "lenso:knowledge-settings-local@1.0.0/plugin";
const coreModule = new WebAssembly.Module(Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]));

function plan() {
  return {
    schema_version: 4,
    terminal_policy: { kind: "required_path" },
    execution_lanes: [{ id: "main" }],
    capability_bindings: [],
    plugin_instances: [{
      authoring_version: 2,
      runtime_profile: "lenso.wasm-component@1",
      instance_key: "lenso.reference.knowledge-settings/default",
      package_id: "lenso.reference.knowledge-settings",
      entrypoint: "plugin",
      configuration: "{}",
      provided_capabilities: [{
        capability_id: CAPABILITY,
        descriptor_version: "1.1.0",
        operations: ["describe", "handle"],
        operation_kinds: {},
        default_admission: null,
        operation_admissions: {},
        event_admission: null,
        cross_lane_transfer: false,
      }],
      required_capabilities: [],
      required_target_capabilities: ["request", "wasm-component", "workers"],
      execution_class: "lenso.wasm-component@1",
      package_revision: ARTIFACT_DIGEST,
      restart_policy: {
        mode: "never", max_attempts: 0,
        window: { secs: 0, nanos: 0 },
        backoff: { secs: 0, nanos: 0 },
        jitter: { secs: 0, nanos: 0 },
        stability: { secs: 0, nanos: 0 },
      },
      criticality: "non_critical",
      execution_lane: "main",
    }],
  };
}

function guest({
  prepareResult = { schema: "lenso.knowledge-settings-command.v1", kind: "read" },
  prepareError = null,
  completeResponse = {
    status: 200,
    headers: [{ name: "content-type", value: "application/json" }],
    body: btoa('{"excerpt_limit":48,"revision":2}'),
  },
} = {}) {
  let created = 0;
  const prepared = [];
  const completed = [];
  return {
    instantiate(getCoreModule, imports) {
      assert.equal(getCoreModule("guest.core.wasm"), coreModule);
      assert.deepEqual(imports, {});
      created++;
      return {
        describe: () => JSON.stringify({
          abi: "lenso.json-request@1",
          capabilities: [{
            capability_id: CAPABILITY,
            descriptor_version: "1.1.0",
            descriptor_digest: DESCRIPTOR_DIGEST,
            request_operations: ["describe", "handle"],
          }],
        }),
        invoke: () => { throw new Error("direct handle must not run"); },
        prepareSettings: (input) => {
          prepared.push(JSON.parse(input));
          if (prepareError !== null)
            throw Object.assign(new Error("Guest domain rejection"), { payload: prepareError });
          return JSON.stringify(prepareResult);
        },
        completeSettings: (input) => {
          completed.push(JSON.parse(input));
          return JSON.stringify(completeResponse);
        },
      };
    },
    get created() { return created; },
    prepared,
    completed,
  };
}

function options(loader, overrides = {}) {
  return {
    plan: plan(), coreModule, instantiate: loader.instantiate,
    verifiedArtifact: { world: WORLD, digest: ARTIFACT_DIGEST },
    bridgeOrigin: "http://127.0.0.1:36742",
    fetchBridge: async () => Response.json({
      schema: "lenso.knowledge-settings-result.v1", kind: "ok",
      settings: { excerpt_limit: 48, revision: 2 },
    }),
    ...overrides,
  };
}

test("private KB adapter rejects a different package, instance, world, or artifact before Guest startup", () => {
  const loader = guest();
  const wrongPackage = plan();
  wrongPackage.plugin_instances[0].package_id = "other.plugin";
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, { plan: wrongPackage })), /package identity/);
  const wrongInstance = plan();
  wrongInstance.plugin_instances[0].instance_key = "lenso.reference.knowledge-settings/other";
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, { plan: wrongInstance })), /Instance key/);
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    verifiedArtifact: { world: "lenso:runtime@1.0.0/plugin", digest: ARTIFACT_DIGEST },
  })), /world identity/);
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    verifiedArtifact: { world: WORLD, digest: `sha256:${"b".repeat(64)}` },
  })), /Artifact identity/);
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    verifiedArtifact: { world: WORLD, digest: ARTIFACT_DIGEST, unverified: true },
  })), /unexpected shape/);
  assert.equal(loader.created, 0);
});

test("private KB adapter requires all four exact WIT exports before readiness", () => {
  const loader = guest();
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    instantiate: (...args) => {
      const value = loader.instantiate(...args);
      delete value.completeSettings;
      return value;
    },
  })), /unexpected shape/);
  assert.throws(() => createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    instantiate: (...args) => ({ ...loader.instantiate(...args), extra() {} }),
  })), /unexpected shape/);
});

test("GET settings keeps bearer out of Guest calls and awaits one read from the exact bridge", async () => {
  const loader = guest();
  const calls = [];
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async (url, init) => {
      calls.push({ url, init });
      return Response.json({
        schema: "lenso.knowledge-settings-result.v1", kind: "ok",
        settings: { excerpt_limit: 48, revision: 2 },
      });
    },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    headers: { authorization: "Bearer opaque-user-token" },
  }));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { excerpt_limit: 48, revision: 2 });
  assert.deepEqual(loader.prepared, [{
    schema: "lenso.knowledge-settings-prepare.v1",
    route_id: "knowledge-base.settings.read",
  }]);
  assert.deepEqual(loader.completed, [{
    schema: "lenso.knowledge-settings-complete.v1",
    route_id: "knowledge-base.settings.read",
    result: {
      schema: "lenso.knowledge-settings-result.v1", kind: "ok",
      settings: { excerpt_limit: 48, revision: 2 },
    },
  }]);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "http://127.0.0.1:36742/v1/knowledge-settings/read");
  assert.equal(calls[0].init.method, "POST");
  assert.equal(calls[0].init.redirect, "manual");
  assert.equal(calls[0].init.headers.authorization, "Bearer opaque-user-token");
  assert.match(JSON.parse(calls[0].init.body).request_id,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(calls[0].init.body.includes("opaque-user-token"), false);
  assert.equal(loader.created, 3);
});

test("GET and PUT ignore unbound query parameters like the Native settings routes", async () => {
  const readLoader = guest();
  const readCalls = [];
  const read = createKnowledgeSettingsLocalWorkerAdapter(options(readLoader, {
    fetchBridge: async (url) => {
      readCalls.push(url);
      return Response.json({ schema: "lenso.knowledge-settings-result.v1", kind: "ok",
        settings: { excerpt_limit: 48, revision: 2 } });
    },
  }));
  const readResponse = await read.handle(new Request("http://worker.test/settings?ignored=1", {
    headers: { authorization: "Bearer opaque-user-token" },
  }));
  assert.equal(readResponse.status, 200);
  assert.deepEqual(readCalls, ["http://127.0.0.1:36742/v1/knowledge-settings/read"]);

  const writeLoader = guest({ prepareResult: {
    schema: "lenso.knowledge-settings-command.v1", kind: "cas",
    excerpt_limit: 48, predecessor_revision: 1,
    payload_sha256: `sha256:${"b".repeat(64)}`,
  } });
  const writeCalls = [];
  const write = createKnowledgeSettingsLocalWorkerAdapter(options(writeLoader, {
    fetchBridge: async (url) => {
      writeCalls.push(url);
      return Response.json({ schema: "lenso.knowledge-settings-result.v1", kind: "ok",
        settings: { excerpt_limit: 48, revision: 2 } });
    },
  }));
  const writeResponse = await write.handle(new Request("http://worker.test/settings?ignored=1", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
    body: JSON.stringify({ excerpt_limit: 48, predecessor_revision: 1 }),
  }));
  assert.equal(writeResponse.status, 200);
  assert.deepEqual(writeCalls, ["http://127.0.0.1:36742/v1/knowledge-settings/compare-and-set"]);
});

test("workerd-compatible manual redirect mode rejects a bridge redirect without following it", async () => {
  const loader = guest({
    completeResponse: {
      status: 503,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"knowledge_storage_unavailable"}'),
    },
  });
  let calls = 0;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async (_url, init) => {
      calls++;
      assert.equal(init.redirect, "manual");
      return Response.redirect("https://example.invalid/elsewhere", 302);
    },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    headers: { authorization: "Bearer opaque-user-token" },
  }));
  assert.equal(response.status, 503);
  assert.equal(calls, 1);
  assert.equal(loader.completed[0].result.kind, "storage_unavailable");
});

test("PUT settings forwards a validated Guest CAS command once with an optional idempotency key", async () => {
  const hash = `sha256:${"b".repeat(64)}`;
  const loader = guest({
    prepareResult: {
      schema: "lenso.knowledge-settings-command.v1", kind: "cas",
      excerpt_limit: 64, predecessor_revision: 2,
      idempotency_key: "request-001", payload_sha256: hash,
    },
    completeResponse: {
      status: 200,
      headers: [{ name: "content-type", value: "application/json" }],
      body: btoa('{"excerpt_limit":64,"revision":3}'),
    },
  });
  const calls = [];
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async (url, init) => {
      calls.push({ url, init });
      return Response.json({
        schema: "lenso.knowledge-settings-result.v1", kind: "ok",
        settings: { excerpt_limit: 64, revision: 3 },
      });
    },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    method: "PUT",
    headers: {
      authorization: "Bearer opaque-user-token",
      "content-type": "application/json",
      "idempotency-key": "request-001",
    },
    body: JSON.stringify({ excerpt_limit: 64, predecessor_revision: 2 }),
  }));
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { excerpt_limit: 64, revision: 3 });
  assert.deepEqual(loader.prepared, [{
    schema: "lenso.knowledge-settings-prepare.v1",
    route_id: "knowledge-base.settings.update",
    body: { excerpt_limit: 64, predecessor_revision: 2 },
    idempotency_key: "request-001",
  }]);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "http://127.0.0.1:36742/v1/knowledge-settings/compare-and-set");
  assert.deepEqual(JSON.parse(calls[0].init.body).command, {
    excerpt_limit: 64, predecessor_revision: 2,
    idempotency_key: "request-001", payload_sha256: hash,
  });
  assert.equal(calls[0].init.body.includes("opaque-user-token"), false);
});

test("a bridge that ignores abort cannot hold a settings turn beyond its deadline or trigger a retry", async () => {
  const loader = guest({
    completeResponse: {
      status: 503,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"knowledge_storage_unavailable"}'),
    },
  });
  let calls = 0;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    bridgeTimeoutMs: 20,
    fetchBridge: () => { calls++; return new Promise(() => {}); },
  }));
  const request = new Request("http://worker.test/settings", {
    headers: { authorization: "Bearer opaque-user-token" },
  });
  const response = await Promise.race([
    adapter.handle(request),
    new Promise((_, reject) => setTimeout(() => reject(new Error("turn deadline was not enforced")), 150)),
  ]);
  assert.equal(response.status, 503);
  assert.equal(calls, 1);
  assert.deepEqual(loader.completed[0].result, {
    schema: "lenso.knowledge-settings-result.v1", kind: "storage_unavailable",
  });
});

test("a stalled bridge response body and cancellation still end in bounded storage failure", async () => {
  const loader = guest({
    completeResponse: {
      status: 503,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"knowledge_storage_unavailable"}'),
    },
  });
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    bridgeTimeoutMs: 20,
    fetchBridge: async () => new Response(new ReadableStream({
      start() {},
      cancel() { return new Promise(() => {}); },
    }), { headers: { "content-type": "application/json" } }),
  }));
  const response = await Promise.race([
    adapter.handle(new Request("http://worker.test/settings", {
      headers: { authorization: "Bearer opaque-user-token" },
    })),
    new Promise((_, reject) => setTimeout(() => reject(new Error("body deadline was not enforced")), 150)),
  ]);
  assert.equal(response.status, 503);
});

test("request body read and Host bridge share one total wall deadline", async () => {
  const loader = guest({
    prepareResult: {
      schema: "lenso.knowledge-settings-command.v1", kind: "cas",
      excerpt_limit: 64, predecessor_revision: 2,
      payload_sha256: `sha256:${"b".repeat(64)}`,
    },
    completeResponse: {
      status: 503,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"knowledge_storage_unavailable"}'),
    },
  });
  let calls = 0;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    bridgeTimeoutMs: 100,
    fetchBridge: () => { calls++; return new Promise(() => {}); },
  }));
  const request = new Request("http://worker.test/settings", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
    body: new ReadableStream({
      start(controller) {
        setTimeout(() => {
          controller.enqueue(new TextEncoder().encode(JSON.stringify({
            excerpt_limit: 64, predecessor_revision: 2,
          })));
          controller.close();
        }, 80);
      },
    }),
    duplex: "half",
  });
  const response = await Promise.race([
    adapter.handle(request),
    new Promise((_, reject) => setTimeout(() => reject(new Error("total deadline was not enforced")), 150)),
  ]);
  assert.equal(response.status, 503);
  assert.equal(calls, 1);
});

test("stale CAS and changed-key conflict preserve Guest 409 responses without retry", async () => {
  for (const kind of ["stale_revision", "idempotency_conflict"]) {
    const loader = guest({
      prepareResult: {
        schema: "lenso.knowledge-settings-command.v1", kind: "cas",
        excerpt_limit: 64, predecessor_revision: 2,
        payload_sha256: `sha256:${"b".repeat(64)}`,
      },
      completeResponse: {
        status: 409,
        headers: [{ name: "content-type", value: "application/problem+json" }],
        body: btoa(JSON.stringify({ code: kind })),
      },
    });
    let calls = 0;
    const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
      fetchBridge: async () => {
        calls++;
        return Response.json({ schema: "lenso.knowledge-settings-result.v1", kind });
      },
    }));
    const response = await adapter.handle(new Request("http://worker.test/settings", {
      method: "PUT",
      headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
      body: JSON.stringify({ excerpt_limit: 64, predecessor_revision: 2 }),
    }));
    assert.equal(response.status, 409, kind);
    assert.equal(calls, 1, kind);
    assert.equal(loader.completed[0].result.kind, kind);
  }
});

test("signed negative predecessor reaches the shared CAS store and remains a 409 stale result", async () => {
  const loader = guest({
    prepareResult: {
      schema: "lenso.knowledge-settings-command.v1", kind: "cas",
      excerpt_limit: 64, predecessor_revision: -1,
      payload_sha256: `sha256:${"b".repeat(64)}`,
    },
    completeResponse: {
      status: 409,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"stale_settings_revision"}'),
    },
  });
  const calls = [];
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async (_url, init) => {
      calls.push(JSON.parse(init.body));
      return Response.json({ schema: "lenso.knowledge-settings-result.v1", kind: "stale_revision" });
    },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "Application/JSON; charset=UTF-8" },
    body: JSON.stringify({ excerpt_limit: 64, predecessor_revision: -1 }),
  }));
  assert.equal(response.status, 409);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command.predecessor_revision, -1);
});

test("coalesced or oversized Idempotency-Key is rejected before Guest and storage", async () => {
  for (const key of ["request-001, request-002", "x".repeat(129)]) {
    const loader = guest();
    let calls = 0;
    const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
      fetchBridge: async () => { calls++; throw new Error("must not fetch"); },
    }));
    const response = await adapter.handle(new Request("http://worker.test/settings", {
      method: "PUT",
      headers: {
        authorization: "Bearer opaque-user-token",
        "content-type": "application/json",
        "idempotency-key": key,
      },
      body: JSON.stringify({ excerpt_limit: 64, predecessor_revision: 2 }),
    }));
    assert.equal(response.status, 400);
    assert.equal((await response.json()).code, "invalid_idempotency_key");
    assert.deepEqual(loader.prepared, []);
    assert.equal(calls, 0);
  }
});

test("Guest HTTP output cannot turn a settings result into active HTML", async () => {
  const loader = guest({
    completeResponse: {
      status: 200,
      headers: [{ name: "content-type", value: "text/html" }],
      body: btoa('{"excerpt_limit":48,"revision":2}'),
    },
  });
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    headers: { authorization: "Bearer opaque-user-token" },
  }));
  assert.equal(response.status, 503);
  assert.deepEqual(await response.json(), { error: "component_unavailable" });
});

test("missing or malformed Bearer credentials never reach Guest preparation or storage", async () => {
  for (const authorization of [null, "Basic abc", "Bearer first, Bearer second"]) {
    const loader = guest({
      completeResponse: {
        status: 401,
        headers: [{ name: "content-type", value: "application/problem+json" }],
        body: btoa('{"code":"authentication_required"}'),
      },
    });
    let calls = 0;
    const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
      fetchBridge: async () => { calls++; throw new Error("must not fetch"); },
    }));
    const response = await adapter.handle(new Request("http://worker.test/settings", {
      headers: authorization === null ? {} : { authorization },
    }));
    assert.equal(response.status, 401);
    assert.deepEqual(loader.prepared, []);
    assert.equal(calls, 0);
  }
});

test("Guest business validation 400 is emitted without a storage call", async () => {
  const loader = guest({
    prepareError: JSON.stringify({
      status: 400,
      headers: [{ name: "content-type", value: "application/problem+json; charset=utf-8" }],
      body: btoa('{"code":"invalid_excerpt_limit"}'),
    }),
  });
  let calls = 0;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async () => { calls++; throw new Error("must not fetch"); },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
    body: JSON.stringify({ excerpt_limit: 4, predecessor_revision: 2 }),
  }));
  assert.equal(response.status, 400);
  assert.equal((await response.json()).code, "invalid_excerpt_limit");
  assert.equal(calls, 0);
});

test("unsafe JSON integers are rejected before rounding them into a Guest CAS command", async () => {
  const loader = guest();
  let calls = 0;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    fetchBridge: async () => { calls++; throw new Error("must not fetch"); },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
    body: '{"excerpt_limit":64,"predecessor_revision":9007199254740993}',
  }));
  assert.equal(response.status, 400);
  assert.equal((await response.json()).code, "unsafe_settings_integer");
  assert.deepEqual(loader.prepared, []);
  assert.equal(calls, 0);
});

test("fractional JSON tokens reach Guest validation without a Host precision rewrite", async () => {
  const loader = guest({
    prepareError: JSON.stringify({
      status: 400,
      headers: [{ name: "content-type", value: "application/problem+json" }],
      body: btoa('{"code":"invalid_predecessor_revision"}'),
    }),
  });
  let input;
  const adapter = createKnowledgeSettingsLocalWorkerAdapter(options(loader, {
    instantiate: (...args) => {
      const value = loader.instantiate(...args);
      return { ...value, prepareSettings(requestJson) {
        input = requestJson;
        return value.prepareSettings(requestJson);
      } };
    },
  }));
  const response = await adapter.handle(new Request("http://worker.test/settings", {
    method: "PUT",
    headers: { authorization: "Bearer opaque-user-token", "content-type": "application/json" },
    body: '{"excerpt_limit":64,"predecessor_revision":2.0000000000000001}',
  }));
  assert.equal(response.status, 400);
  assert.match(input, /"predecessor_revision":2\.0000000000000001/);
});
