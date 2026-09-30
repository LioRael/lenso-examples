import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const options = Object.fromEntries(process.argv.slice(2).reduce((pairs, item, index, args) => {
  if (index % 2 === 0) pairs.push([item.slice(2), args[index + 1]]);
  return pairs;
}, []));
const origin = new URL(options.origin);
assert.equal(origin.origin, options.origin);
if (options["remote-facts"]) {
  const facts = JSON.parse(fs.readFileSync(options["remote-facts"], "utf8"));
  assert.equal(options.origin, facts.public_origin);
  assert.equal(origin.protocol, "https:");
  assert.equal(origin.hostname, `${facts.worker_name}.${facts.workers_subdomain}.workers.dev`);
} else {
  assert.equal(origin.protocol, "http:");
  assert.equal(origin.hostname, "127.0.0.1");
}
assert.match(options.nonce, /^[a-zA-Z0-9]{1,32}$/);
fs.mkdirSync(options.directory, { mode: 0o700, recursive: true });
function record(name, value) {
  const descriptor = fs.openSync(path.join(options.directory, name + ".json"), "wx", 0o600);
  try {
    fs.writeFileSync(descriptor, JSON.stringify(value, null, 2) + "\n");
    fs.fsyncSync(descriptor);
  } finally {
    fs.closeSync(descriptor);
  }
}
function credential(file) {
  const metadata = fs.lstatSync(file);
  assert(metadata.isFile() && !metadata.isSymbolicLink());
  assert.equal(metadata.mode & 0o777, 0o600);
  assert.equal(metadata.uid, process.getuid());
  assert(metadata.size <= 8192);
  return fs.readFileSync(file, "utf8").trim();
}
const alice = credential(options["alice-file"]);
const bob = credential(options["bob-file"]);
const machine = credential(options["machine-file"]);
const sdkRoot = path.join(options["sdk-dir"], "node_modules/@modelcontextprotocol/client");
const sdk = JSON.parse(fs.readFileSync(path.join(sdkRoot, "package.json"), "utf8"));
assert.equal(sdk.name, "@modelcontextprotocol/client");
assert.equal(sdk.version, "2.2.0");
const { Client, StreamableHTTPClientTransport } = await import(
  pathToFileURL(path.join(sdkRoot, "dist/index.mjs")).href
);
function decode(body) {
  if (body.split("\n").some(line => line.startsWith("data:"))) {
    const lines = body.split("\n").filter(line => line.startsWith("data:"));
    assert.equal(lines.length, 1);
    return JSON.parse(lines[0].slice(5).trim());
  }
  return JSON.parse(body);
}
function owner(reply) {
  assert(!reply.isError);
  return JSON.parse(reply.content.find(block => block.type === "text").text);
}
let protocolVersion;
let dropNextCommit = false;
let commitCalls = 0;
let commitParameters;
const client = new Client({ name: "ordinary-worker-official-client", version: "1" }, {
  capabilities: {}, versionNegotiation: { mode: "legacy" },
});
const transport = new StreamableHTTPClientTransport(new URL(options.origin + "/mcp"), {
  authProvider: { token: async () => alice },
  fetch: async (url, init) => {
    assert.equal(String(url), options.origin + "/mcp");
    const packet = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
    const commit = dropNextCommit && packet?.method === "tools/call";
    if (commit) {
      assert.deepEqual(packet.params, commitParameters);
      assert.equal(++commitCalls, 1);
    }
    const response = await fetch(url, init);
    assert.equal(response.headers.get("cache-control"), "no-store");
    if (packet?.method === "initialize") {
      const initialized = decode(await response.clone().text());
      record("initialize-observed", { status: response.status, request: packet, response: initialized });
      assert(initialized.result);
      protocolVersion = initialized.result.protocolVersion;
    }
    if (commit) {
      const upstream = decode(await response.clone().text());
      record("commit-upstream-completed", { status: response.status, response: upstream });
      await response.body?.cancel();
      throw new Error("qualification_response_deliberately_discarded");
    }
    return response;
  },
});
async function http(route, token, payload) {
  const response = await fetch(options.origin + route, {
    headers: { "content-type": "application/json", ...(token ? { authorization: "Bearer " + token } : {}) },
    ...(payload === undefined ? {} : { method: "POST", body: JSON.stringify(payload) }),
  });
  if (route !== "/.well-known/oauth-protected-resource") {
    assert.equal(response.headers.get("cache-control"), "no-store");
  }
  return { status: response.status, reply: await response.json(), challenge: response.headers.get("www-authenticate") };
}
async function mutation(phase, route, token, payload) {
  record(phase + "-started", { route, request: payload });
  const response = await http(route, token, payload);
  record(phase + "-completed", response);
  return response;
}
try {
  assert.equal((await http("/.well-known/oauth-protected-resource")).status, 404);
  const unauthorized = await http("/mcp", undefined, { jsonrpc: "2.0", id: 1, method: "tools/list", params: {} });
  assert.equal(unauthorized.status, 401);
  assert.equal(unauthorized.challenge, "Bearer");
  await client.connect(transport);
  const { tools } = await client.listTools();
  assert.equal(tools.length, 3);
  assert(tools.every(tool => tool.name.length <= 64 && !tool.name.includes("approve") && !tool.name.includes("token")));
  const read = tools.find(tool => tool.annotations?.readOnlyHint && tool.name !== "management__status");
  const write = tools.find(tool => tool.annotations?.destructiveHint);
  const readArguments = { input: {}, expected_revision: null, idempotency_key: null };
  const initial = JSON.parse(owner(await client.callTool({ name: read.name, arguments: readArguments })).result_json);
  if (options.phase === "observe") {
    const prior = JSON.parse(fs.readFileSync(options.reference, "utf8"));
    const recovered = owner(await client.callTool({ name: "management__status", arguments: { operation_id: prior.operation_id } }));
    assert.deepEqual(recovered, prior.committed);
    assert.deepEqual(initial, prior.final_business_state);
    assert.deepEqual((await http("/management/operations/" + prior.operation_id, alice)).reply, recovered);
    record("official-mcp-restart-observed", {
      schema: "lenso.qualification.receipt.v1", operation_id: prior.operation_id,
      client: { package: sdk.name, version: sdk.version, protocol_version: protocolVersion },
      ...(options["remote-facts"] ? {
        same_artifact_redeploy: "passed", physical_process_restart: "not_asserted",
      } : { persistent_restart: "passed" }),
      same_operation_receipt: true,
      final_business_state: initial, target_replayed: false, write_calls: 0,
    });
    console.log(JSON.stringify({ phase: "observe", result: "passed" }));
    process.exitCode = 0;
  } else {
  assert(options.phase === undefined || options.phase === "journey");
  for (const rejected of [
    { name: "hidden_business_write", arguments: {} },
    { name: read.name, arguments: { ...readArguments, target_instance: "example.ops-state/secondary" } },
  ]) {
    let denied = false;
    try {
      denied = Boolean((await client.callTool(rejected)).isError);
    } catch (error) {
      assert.equal(typeof error.code, "number");
      denied = true;
    }
    assert(denied);
  }
  const parameters = { name: write.name, arguments: {
    input: { value: 49 }, expected_revision: String(initial.revision),
    idempotency_key: "workers-official-mcp-" + options.nonce,
  } };
  record("pending-started", { request: parameters });
  const pending = owner(await client.callTool(parameters));
  record("pending-completed", pending);
  assert.equal(pending.state, "pending_approval");
  const operation = pending.operation_id;
  const intent = await http("/management/approval/" + operation, bob);
  assert.equal(intent.status, 200);
  assert.deepEqual(JSON.parse(intent.reply.parameters_json), {
    input: { value: 49 }, expected_revision: String(initial.revision),
  });
  const decision = { operation_id: operation, intent_digest: intent.reply.intent_digest, decision: "approved" };
  for (const [name, token] of [["self-denied", alice], ["machine-denied", machine]]) {
    assert.equal((await mutation(name, "/management/approval", token, decision)).status, 403);
  }
  const approved = await mutation("human-approved", "/management/approval", bob, decision);
  assert.equal(approved.status, 200);
  assert.equal(approved.reply.status, "approved");
  assert.equal(approved.reply.audit_pending, false);
  record("commit-started", { request: parameters, response_delivery: "deliberately_discarded" });
  commitParameters = parameters;
  dropNextCommit = true;
  await assert.rejects(client.callTool(parameters));
  dropNextCommit = false;
  assert.equal(commitCalls, 1);
  const upstream = JSON.parse(fs.readFileSync(path.join(options.directory, "commit-upstream-completed.json"), "utf8"));
  assert.equal(upstream.status, 200);
  const committed = owner(upstream.response.result);
  assert.equal(committed.state, "succeeded");
  assert.equal(committed.operation_id, operation);
  const recovered = owner(await client.callTool({ name: "management__status", arguments: { operation_id: operation } }));
  assert.deepEqual(recovered, committed);
  assert.deepEqual((await http("/management/operations/" + operation, alice)).reply, committed);
  const final = JSON.parse(owner(await client.callTool({ name: read.name, arguments: readArguments })).result_json);
  assert.equal(final.value, 49);
  assert.equal(final.revision, initial.revision + 1);
  const result = JSON.parse(committed.result_json);
  assert.equal(result.receipt_id, committed.receipt);
  assert.deepEqual({ label: result.label, value: result.value, revision: result.revision }, final);
  record("official-mcp-lost-reply", {
    schema: "lenso.qualification.receipt.v1", layer: "ordinary-source-worker-graph",
    infrastructure: options["remote-facts"] ? "cloudflare-workers" : "local-workerd",
    client: { package: sdk.name, version: sdk.version, protocol_version: protocolVersion },
    authentication: "explicit-preissued-bearer", oauth_discovery: "absent",
    operation_id: operation, intent_digest: intent.reply.intent_digest,
    committed, final_business_state: final, upstream_commit_calls: commitCalls,
    lost_client_response: true, recovery: "status-and-read-only", target_replayed: false,
    cases: ["actual_official_client_handshake_catalog_read", "no_human_or_token_tools",
      "hidden_tool_denied", "target_override_denied",
      "actual_mcp_pending", "self_and_machine_approval_denied", "distinct_human_approved",
      "commit_response_discarded", "same_operation_receipt_recovered_without_invoke"],
  });
  console.log(JSON.stringify({ client: "@modelcontextprotocol/client@2.2.0", result: "passed" }));
  }
} finally {
  await client.close();
}
