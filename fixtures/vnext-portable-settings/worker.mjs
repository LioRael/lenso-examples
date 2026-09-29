import { initSync, settings_event } from "./.wasm/portable_settings_fixture.js";
import module from "./.wasm/portable_settings_fixture_bg.wasm";
import { BodyReadError, boundedText } from "./body.mjs";

initSync({ module });

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (Number(request.headers.get("content-length") || 0) > 4096) {
      return new Response("too large", { status: 413 });
    }
    // The fixture Host owns this trusted loopback resource, never the endpoint.
    const origin = new URL(env.SETTINGS_BRIDGE_ORIGIN);
    if (origin.protocol !== "http:" || origin.hostname !== "127.0.0.1"
        || origin.pathname !== "/" || origin.username || origin.password
        || origin.search || origin.hash) throw new Error("bridge must be loopback");
    const persistence = async (operation, body) => {
      if (!["read", "change"].includes(operation)) throw new Error("unknown persistence operation");
      const signal = AbortSignal.timeout(3000);
      const result = await fetch(new URL(operation, origin), {
        method: "POST",
        headers: { authorization: `Bearer ${env.SETTINGS_BRIDGE_TOKEN}` },
        body,
        redirect: "manual",
        signal,
      });
      if (result.status !== 200) throw new Error("persistence unavailable");
      return boundedText(result.body, 4096, { signal });
    };
    let body;
    try {
      body = await boundedText(request.body, 4096, { signal: request.signal });
    } catch (error) {
      if (!(error instanceof BodyReadError)) throw error;
      return new Response(error.message, { status: error.status });
    }
    const result = JSON.parse(await settings_event(JSON.stringify({
      method: request.method,
      uri: url.pathname + url.search,
      content_type: request.headers.get("content-type") || "",
      principal: request.headers.get("x-local-test-principal") || "",
      body,
    }), persistence));
    return Response.json(result.body, { status: result.status });
  },
};
