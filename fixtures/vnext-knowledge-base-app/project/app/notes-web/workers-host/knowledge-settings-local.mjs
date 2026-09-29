import { admitWorkersComponent, boundedJson } from "./component-admission.mjs";

const PACKAGE_ID = "lenso.reference.knowledge-settings";
const INSTANCE_KEY = `${PACKAGE_ID}/default`;
const WORLD = "lenso:knowledge-settings-local@1.0.0/plugin";
const CAPABILITY = "lenso.http.endpoint@1";
const DESCRIPTOR_DIGEST = "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe";
const MAX_JSON_BYTES = 65_536;

function exactObject(value, keys, name) {
  if (value === null || typeof value !== "object" || Array.isArray(value) ||
      Reflect.ownKeys(value).length !== keys.length ||
      keys.some((key) => !Object.hasOwn(value, key)))
    throw new TypeError(`${name} has an unexpected shape`);
  return value;
}

function bridgeOriginUrl(origin) {
  if (typeof origin !== "string") throw new TypeError("explicit loopback bridge origin is required");
  const url = new URL(origin);
  if (url.origin !== origin || url.protocol !== "http:" ||
      !["127.0.0.1", "[::1]"].includes(url.hostname) || !url.port ||
      url.username || url.password)
    throw new TypeError("bridge origin must be an explicit loopback HTTP origin");
  return url.origin;
}

function failure(status, code, headers = {}) {
  return Response.json({ error: code }, {
    status,
    headers: { "cache-control": "no-store", "x-content-type-options": "nosniff", ...headers },
  });
}

function invalidIdempotencyKey() {
  return Response.json({
    type: "about:blank",
    title: "Bad Request",
    status: 400,
    detail: "Idempotency-Key must be 1 through 128 printable ASCII bytes without a comma",
    code: "invalid_idempotency_key",
  }, {
    status: 400,
    headers: {
      "content-type": "application/problem+json; charset=utf-8",
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
}

function unsafeSettingsInteger() {
  return Response.json({
    type: "about:blank",
    title: "Bad Request",
    status: 400,
    detail: "settings integer is outside the exact JavaScript integer range",
    code: "unsafe_settings_integer",
  }, {
    status: 400,
    headers: {
      "content-type": "application/problem+json; charset=utf-8",
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
}

async function readBounded(body, limit, signal) {
  if (!body) return new Uint8Array();
  if (signal.aborted) throw new DOMException("request aborted", "AbortError");
  const reader = body.getReader();
  const chunks = [];
  let size = 0;
  let complete = false;
  let onAbort;
  const interrupted = new Promise((_, reject) => {
    onAbort = () => reject(new DOMException("request aborted", "AbortError"));
    signal.addEventListener("abort", onAbort, { once: true });
  });
  try {
    while (true) {
      const { value, done } = await Promise.race([reader.read(), interrupted]);
      if (done) { complete = true; break; }
      if (value.byteLength > limit - size) throw new RangeError("body exceeds local bound");
      size += value.byteLength;
      chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return bytes;
  } finally {
    signal.removeEventListener("abort", onAbort);
    if (!complete) {
      let cleanupTimer;
      try {
        await Promise.race([
          reader.cancel().catch(() => {}),
          new Promise((resolve) => { cleanupTimer = setTimeout(resolve, 25); }),
        ]);
      } finally { clearTimeout(cleanupTimer); }
    }
    reader.releaseLock();
  }
}

function bridgeResult(value) {
  if (value?.kind === "ok") {
    exactObject(value, ["schema", "kind", "settings"], "bridge result");
    const settings = exactObject(value.settings, ["excerpt_limit", "revision"], "bridge settings");
    if (!Number.isSafeInteger(settings.excerpt_limit) ||
        settings.excerpt_limit < 16 || settings.excerpt_limit > 512 ||
        !Number.isSafeInteger(settings.revision) || settings.revision < 1)
      throw new TypeError("bridge settings are outside the local protocol");
  } else {
    exactObject(value, ["schema", "kind"], "bridge result");
    if (!["stale_revision", "idempotency_conflict", "unauthorized", "storage_unavailable"]
      .includes(value.kind)) throw new TypeError("unknown bridge outcome");
  }
  if (value.schema !== "lenso.knowledge-settings-result.v1")
    throw new TypeError("unknown bridge result schema");
  return value;
}

function responseFromGuest(value, expectedStatus) {
  const result = exactObject(JSON.parse(boundedJson(value, "Guest HTTP response")),
    ["status", "headers", "body"], "Guest HTTP response");
  if (result.status !== expectedStatus || !Array.isArray(result.headers) ||
      result.headers.length !== 1 || typeof result.body !== "string" ||
      result.body.length > 4 * Math.ceil(MAX_JSON_BYTES / 3))
    throw new TypeError("Guest HTTP response is outside the local protocol");
  const headers = new Headers();
  for (const entry of result.headers) {
    exactObject(entry, ["name", "value"], "Guest HTTP header");
    if (typeof entry.name !== "string" || typeof entry.value !== "string" ||
        entry.name.toLowerCase() !== "content-type" || /[\r\n]/.test(entry.value))
      throw new TypeError("Guest HTTP header is outside the local protocol");
    const mediaType = entry.value.split(";", 1)[0].trim().toLowerCase();
    if (mediaType !== (expectedStatus === 200 ? "application/json" : "application/problem+json") ||
        (entry.value.includes(";") && !/^;\s*charset=utf-8$/i.test(entry.value.slice(entry.value.indexOf(";")))))
      throw new TypeError("Guest HTTP content type is outside the local protocol");
    headers.set(entry.name, entry.value);
  }
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(result.body) || result.body.length % 4 !== 0)
    throw new TypeError("Guest HTTP body is not canonical Base64");
  const binary = atob(result.body);
  if (btoa(binary) !== result.body || binary.length > MAX_JSON_BYTES)
    throw new TypeError("Guest HTTP body is outside the local bound");
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  headers.set("cache-control", "no-store");
  headers.set("x-content-type-options", "nosniff");
  return new Response(bytes, { status: result.status, headers });
}

function bearer(request) {
  const value = request.headers.get("authorization");
  const match = /^Bearer ([\x21-\x7e]{1,4096})$/i.exec(value ?? "");
  return match && !match[1].includes(",") ? `Bearer ${match[1]}` : null;
}

function isJsonMediaType(value) {
  if (typeof value !== "string") return false;
  const parts = value.split(";");
  return parts[0].trim().toLowerCase() === "application/json" &&
    (parts.length === 1 ||
      (parts.length === 2 && /^charset\s*=\s*utf-8$/i.test(parts[1].trim())));
}

function settingsCommand(value, routeId, body, idempotencyKey) {
  if (routeId === "knowledge-base.settings.read") {
    exactObject(value, ["schema", "kind"], "Guest settings command");
    if (value.schema !== "lenso.knowledge-settings-command.v1" || value.kind !== "read")
      throw new TypeError("Guest settings read command differs from route");
    return value;
  }
  exactObject(value, ["schema", "kind", "excerpt_limit", "predecessor_revision",
    "payload_sha256", ...(idempotencyKey === null ? [] : ["idempotency_key"])],
  "Guest settings command");
  if (value.schema !== "lenso.knowledge-settings-command.v1" || value.kind !== "cas" ||
      !Number.isSafeInteger(value.excerpt_limit) || value.excerpt_limit < 16 ||
      value.excerpt_limit > 512 || !Number.isSafeInteger(value.predecessor_revision) ||
      !/^sha256:[0-9a-f]{64}$/.test(value.payload_sha256) ||
      value.excerpt_limit !== body?.excerpt_limit ||
      value.predecessor_revision !== body?.predecessor_revision ||
      (idempotencyKey !== null && value.idempotency_key !== idempotencyKey))
    throw new TypeError("Guest settings CAS command differs from the request");
  return value;
}

async function readUpdateBody(request, timeoutMs) {
  if (timeoutMs <= 0) return { error: failure(503, "request_deadline_exceeded") };
  if (!isJsonMediaType(request.headers.get("content-type")))
    return { error: failure(415, "unsupported_media_type") };
  const length = request.headers.get("content-length");
  if (length !== null && /^\d+$/.test(length) && Number(length) > MAX_JSON_BYTES)
    return { error: failure(413, "request_too_large") };
  const controller = new AbortController();
  const onAbort = () => controller.abort();
  request.signal.addEventListener("abort", onAbort, { once: true });
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const bytes = await readBounded(request.body, MAX_JSON_BYTES, controller.signal);
    const rawBody = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    const body = JSON.parse(rawBody);
    if (body !== null && typeof body === "object" && !Array.isArray(body) &&
        [body.excerpt_limit, body.predecessor_revision].some((value) =>
          typeof value === "number" &&
          (!Number.isFinite(value) || Math.abs(value) > Number.MAX_SAFE_INTEGER)))
      return { error: unsafeSettingsInteger() };
    return { body, rawBody };
  } catch (error) {
    if (error instanceof RangeError) return { error: failure(413, "request_too_large") };
    if (controller.signal.aborted) return { error: failure(503, "request_cancelled") };
    return { error: failure(400, "invalid_request_body") };
  } finally {
    clearTimeout(timer);
    request.signal.removeEventListener("abort", onAbort);
  }
}

async function readStore({ origin, fetchBridge, authorization, signal, timeoutMs, command }) {
  if (timeoutMs <= 0)
    return { schema: "lenso.knowledge-settings-result.v1", kind: "storage_unavailable" };
  const controller = new AbortController();
  const onAbort = () => controller.abort();
  signal.addEventListener("abort", onAbort, { once: true });
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  let onInterrupted;
  const interrupted = new Promise((_, reject) => {
    onInterrupted = () => reject(new DOMException("bridge deadline exceeded", "AbortError"));
    controller.signal.addEventListener("abort", onInterrupted, { once: true });
  });
  const payload = {
    schema: "lenso.knowledge-settings-store.v1",
    request_id: crypto.randomUUID(),
    ...(command.kind === "cas" ? { command: {
      excerpt_limit: command.excerpt_limit,
      predecessor_revision: command.predecessor_revision,
      ...(Object.hasOwn(command, "idempotency_key")
        ? { idempotency_key: command.idempotency_key } : {}),
      payload_sha256: command.payload_sha256,
    } } : {}),
  };
  const path = command.kind === "read" ? "/v1/knowledge-settings/read" :
    "/v1/knowledge-settings/compare-and-set";
  try {
    if (signal.aborted) throw new DOMException("request aborted", "AbortError");
    const response = await Promise.race([fetchBridge(`${origin}${path}`, {
      method: "POST",
      headers: { authorization, "content-type": "application/json" },
      body: JSON.stringify(payload),
      redirect: "manual",
      signal: controller.signal,
    }), interrupted]);
    if (response.status !== 200 || !isJsonMediaType(response.headers.get("content-type")))
      throw new TypeError("bridge HTTP response is outside the local protocol");
    const length = response.headers.get("content-length");
    if (length !== null && /^\d+$/.test(length) && Number(length) > MAX_JSON_BYTES)
      throw new RangeError("bridge result exceeds local bound");
    const bytes = await readBounded(response.body, MAX_JSON_BYTES, controller.signal);
    return bridgeResult(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)));
  } catch {
    return { schema: "lenso.knowledge-settings-result.v1", kind: "storage_unavailable" };
  } finally {
    clearTimeout(timer);
    controller.signal.removeEventListener("abort", onInterrupted);
    signal.removeEventListener("abort", onAbort);
  }
}

/** Local workerd qualification adapter for one verified knowledge-settings Bundle. */
export function createKnowledgeSettingsLocalWorkerAdapter({
  plan, verifiedArtifact, coreModule, instantiate, coreModulePath,
  bridgeOrigin, fetchBridge, bridgeTimeoutMs = 1000,
} = {}) {
  const instance = plan?.plugin_instances?.[0];
  if (instance?.package_id !== PACKAGE_ID)
    throw new TypeError("knowledge-settings package identity differs from the selected Plan");
  if (instance.instance_key !== INSTANCE_KEY)
    throw new TypeError("knowledge-settings Instance key differs from the selected Plan");
  exactObject(verifiedArtifact, ["world", "digest"], "verified Artifact");
  if (verifiedArtifact?.world !== WORLD)
    throw new TypeError("knowledge-settings world identity differs from the verified Artifact");
  if (!/^sha256:[0-9a-f]{64}$/.test(verifiedArtifact.digest) ||
      instance.package_revision !== verifiedArtifact.digest)
    throw new TypeError("knowledge-settings Artifact identity differs from the selected Plan");
  if (typeof fetchBridge !== "function" || !Number.isSafeInteger(bridgeTimeoutMs) ||
      bridgeTimeoutMs < 1 || bridgeTimeoutMs > 5000)
    throw new TypeError("explicit bounded bridge transport is required");
  const origin = bridgeOriginUrl(bridgeOrigin);
  const admitted = admitWorkersComponent({
    plan, instanceKey: INSTANCE_KEY, coreModule, instantiate, coreModulePath,
    expectedDescriptorDigests: { [CAPABILITY]: DESCRIPTOR_DIGEST },
    guestExports: ["describe", "invoke", "prepareSettings", "completeSettings"],
    expectedPackageId: PACKAGE_ID,
    expectedArtifactDigest: verifiedArtifact.digest,
  });
  return Object.freeze({
    async handle(request) {
      const deadlineAt = performance.now() + bridgeTimeoutMs;
      const url = new URL(request.url);
      if (url.pathname !== "/settings") return failure(404, "not_found");
      if (!["GET", "PUT"].includes(request.method))
        return failure(405, "method_not_allowed", { allow: "GET, PUT" });
      if (request.signal.aborted) return failure(503, "request_cancelled");
      const routeId = request.method === "GET" ?
        "knowledge-base.settings.read" : "knowledge-base.settings.update";
      const authorization = bearer(request);
      if (!authorization) {
        try {
          return responseFromGuest(admitted.freshGuest().completeSettings(JSON.stringify({
            schema: "lenso.knowledge-settings-complete.v1", route_id: routeId,
            result: { schema: "lenso.knowledge-settings-result.v1", kind: "unauthorized" },
          })), 401);
        } catch { return failure(503, "component_unavailable"); }
      }
      let body;
      let rawBody;
      const idempotencyKey = request.headers.get("idempotency-key");
      if (request.method === "PUT") {
        if (idempotencyKey !== null &&
            (!/^[\x21-\x7e]{1,128}$/.test(idempotencyKey) || idempotencyKey.includes(",")))
          return invalidIdempotencyKey();
        const incoming = await readUpdateBody(request, deadlineAt - performance.now());
        if (incoming.error) return incoming.error;
        body = incoming.body;
        rawBody = incoming.rawBody;
      }
      let command;
      try {
        const preparedInput = request.method === "PUT" ?
          `{"schema":"lenso.knowledge-settings-prepare.v1","route_id":"${routeId}",` +
          `"body":${rawBody}` +
          (idempotencyKey === null ? "" : `,"idempotency_key":${JSON.stringify(idempotencyKey)}`) +
          "}" : JSON.stringify({ schema: "lenso.knowledge-settings-prepare.v1", route_id: routeId });
        const prepared = admitted.freshGuest().prepareSettings(preparedInput);
        command = settingsCommand(JSON.parse(boundedJson(prepared, "Guest settings command")),
          routeId, body, idempotencyKey);
      } catch (error) {
        if (typeof error?.payload === "string") {
          try { return responseFromGuest(error.payload, 400); }
          catch { return failure(502, "invalid_component_error"); }
        }
        return failure(503, "component_unavailable");
      }
      const result = await readStore({
        origin, fetchBridge, authorization, signal: request.signal,
        timeoutMs: deadlineAt - performance.now(), command,
      });
      try {
        const expectedStatus = result.kind === "ok" ? 200 :
          result.kind === "unauthorized" ? 401 :
            ["stale_revision", "idempotency_conflict"].includes(result.kind) ? 409 : 503;
        return responseFromGuest(admitted.freshGuest().completeSettings(JSON.stringify({
          schema: "lenso.knowledge-settings-complete.v1", route_id: routeId, result,
        })), expectedStatus);
      } catch { return failure(503, "component_unavailable"); }
    },
  });
}
