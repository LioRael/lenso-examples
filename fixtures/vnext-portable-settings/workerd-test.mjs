import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Miniflare } from "miniflare";

const root = path.dirname(fileURLToPath(import.meta.url));
if (!process.env.LENSO_WORKERS_RUNTIME_ROOT) throw new Error("set LENSO_WORKERS_RUNTIME_ROOT");
const runtimeClock = path.join(process.env.LENSO_WORKERS_RUNTIME_ROOT, "clock.mjs");
const modules = [
  { type: "ESModule", path: "worker.mjs", contents: await readFile(path.join(root, "worker.mjs"), "utf8") },
  { type: "ESModule", path: "body.mjs", contents: await readFile(path.join(root, "body.mjs"), "utf8") },
  { type: "ESModule", path: ".wasm/portable_settings_fixture.js",
    contents: await readFile(path.join(root, ".wasm/portable_settings_fixture.js"), "utf8") },
  { type: "CompiledWasm", path: ".wasm/portable_settings_fixture_bg.wasm",
    contents: await readFile(path.join(root, ".wasm/portable_settings_fixture_bg.wasm")) },
  { type: "ESModule", path: ".wasm/@lenso/workers-runtime/clock", contents: await readFile(runtimeClock, "utf8") },
];
const options = {
  modules: modules.map(module => ({ ...module, path: path.join(root, module.path) })),
  modulesRoot: root, compatibilityDate: "2026-06-01",
  bindings: {
    SETTINGS_BRIDGE_ORIGIN: process.env.SETTINGS_BRIDGE_ORIGIN,
    SETTINGS_BRIDGE_TOKEN: process.env.SETTINGS_BRIDGE_TOKEN,
  },
};

async function request(mf, method, body, principal = "shared") {
  const response = await mf.dispatchFetch("http://fixture/settings", {
    method, headers: { "x-local-test-principal": principal, "content-type": "application/json" },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  return { status: response.status, body: await response.json() };
}

let mf = new Miniflare(options);
try {
  if (process.argv.includes("--unavailable")) {
    assert.equal((await request(mf, "GET")).status, 503);
    console.log("workerd: unavailable durable store fails closed");
  } else {
    let partial;
    const body = new ReadableStream({
      start(controller) { partial = controller; controller.enqueue(new TextEncoder().encode("{")); },
    });
    try {
      const stalled = await mf.dispatchFetch("http://fixture/settings", {
        method: "PUT", headers: { "content-type": "application/json" },
        body, duplex: "half", signal: AbortSignal.timeout(6000),
      });
      assert.equal(stalled.status, 408, "unfinished body must time out before Kernel startup");
      await stalled.text();
    } finally {
      try { partial.close(); } catch { /* Reader cancellation may already have closed the upload. */ }
    }
    assert.equal((await mf.dispatchFetch("http://fixture/not-settings")).status, 404);
    assert.equal((await mf.dispatchFetch("http://fixture/settings", {
      method: "PUT", headers: { "content-type": "application/json" }, body: "{",
    })).status, 400);
    assert.equal((await mf.dispatchFetch("http://fixture/settings", {
      method: "PUT", headers: { "content-type": "text/plain" }, body: "{}",
    })).status, 415);
    assert.deepEqual(await request(mf, "GET"), { status: 200, body: { revision: 1, value: "native" } });
    const change = { expected_revision: 1, idempotency_key: "workers", value: "workers" };
    const written = { status: 200, body: { revision: 2, value: "workers" } };
    assert.deepEqual(await request(mf, "PUT", change), written);
    assert.deepEqual(await request(mf, "PUT", change), written);
    assert.equal((await request(mf, "PUT", { ...change, value: "different" })).status, 409);
    assert.equal((await request(mf, "PUT", { ...change, idempotency_key: "stale" })).status, 409);
    assert.equal((await request(mf, "PUT", { ...change, value: "" })).status, 400);
    assert.deepEqual(await request(mf, "GET", null, "other"), { status: 200, body: { revision: 0, value: "" } });
    await mf.dispose();
    mf = new Miniflare(options);
    assert.deepEqual(await request(mf, "GET"), written);
    assert.deepEqual(await request(mf, "PUT", change), written);
    console.log("workerd: typed endpoint -> store read/CAS/replay/conflict/restart passed");
  }
} finally {
  await mf.dispose();
}
