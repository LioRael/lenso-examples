import * as generated from "./pkg/lenso_ops_workers_owner_operator.js";
import wasm from "./pkg/lenso_ops_workers_owner_operator_bg.wasm";
import { createEventScope, createWorkersHttpHost } from "./owner-assets/workers-runtime/index.mjs";
import { create as authState } from "./owner-assets/auth-state.mjs";
import { create as accessState } from "./owner-assets/access-state.mjs";
import { create as auditStore, setup as setupAudit } from "./owner-assets/audit-store.mjs";
import { create as approvalStore, setup as setupApproval } from "./owner-assets/approval-store.mjs";
import { create as managementStore, setup as setupManagement, grant, revoke } from "./owner-assets/management-store.mjs";

const actions = new Map();
const ID = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$/;
const response = (status, value) => JSON.stringify({ status, headers: [["content-type", "application/json"], ["cache-control", "no-store"]], body: [...new TextEncoder().encode(JSON.stringify(value))], shutdown: "clean" });
const READS = new Set(["verify", "metadata", "list_roles", "public_key", "inspect_record", "inspect_operation", "verify_operation"]);
function inspection(parameters) {
  if (typeof parameters.deployment !== "string" || parameters.deployment.length < 1 || parameters.deployment.length > 128
    || typeof parameters.operation_id !== "string" || !/^[a-f0-9]{64}$/.test(parameters.operation_id)
    || Object.keys(parameters).some(key => !["deployment", "operation_id"].includes(key))) {
    throw new Error("invalid_read_inspection");
  }
}
function selectedScope(request, env) {
  if (request.method !== "POST" || new URL(request.url).pathname !== "/_qualification/owners" || new URL(request.url).search || request.headers.has("origin") || request.headers.has("cookie")) {
    throw Object.assign(new Error("private_operator_only"), { status: 403 });
  }
  const capability = env.OPS_TEST_OPERATOR_CAPABILITY;
  if (typeof capability !== "string" || capability.length < 32 || request.headers.get("authorization") !== `Bearer ${capability}`) {
    throw Object.assign(new Error("private_operator_only"), { status: 403 });
  }
  return createEventScope((scope) => ({
    auth: authState(env.AUTH_DB, scope, { profile: "workers-d1", binding: "AUTH_DB" }),
    access: accessState(env.ACCESS_CONTROL_DB, scope, { profile: "workers-d1", binding: "ACCESS_CONTROL_DB" }),
    audit: auditStore(env.AUDIT_DB, scope, { profile: "workers-d1" }),
    approval: approvalStore(env.APPROVAL_DB, scope, { profile: "workers-d1" }),
    management: managementStore(env.MANAGEMENT_DB, scope, { profile: "workers-d1" }),
    operator: Object.freeze({
      setup: Object.freeze({
        audit: () => scope.run(() => setupAudit(env.AUDIT_DB)),
        approval: () => scope.run(() => setupApproval(env.APPROVAL_DB)),
        management: () => scope.run(() => setupManagement(env.MANAGEMENT_DB)),
      }),
      qualify: (deployment, subject, remove) => scope.run(() => (remove ? revoke : grant)(env.MANAGEMENT_DB, deployment, subject)),
      authConfiguration: env.OPS_AUTH_CONFIGURATION_JSON,
      authSecrets: env.OPS_AUTH_SECRETS_JSON,
    }),
  }), { cleanupTimeoutMs: 1000, maxOperations: 256 });
}

async function dispatch(body, scope) {
  const { owner, operation, parameters = {} } = body;
  if (owner === "auth") {
    if (operation === "setup") { await generated.setup_auth(scope.auth.batch); return { completed: true }; }
    if (operation === "public_key") {
      const config = JSON.parse(scope.operator.authConfiguration), secrets = JSON.parse(scope.operator.authSecrets);
      const publicKey = generated.auth_public_key(secrets[config.assertion_signing_key_secret]);
      return { assertion_public_key: publicKey,
        configured_key_matches: config.assertion_public_key === publicKey };
    }
    if (operation === "verify") { await generated.verify_auth(scope.auth.batch); return { verified: true }; }
    if (!["issue", "metadata", "revoke_token", "revoke_session", "attenuate"].includes(operation)) throw new Error("unsupported_operator_action");
    const config = JSON.parse(scope.operator.authConfiguration);
    const secrets = JSON.parse(scope.operator.authSecrets);
    const pepper = secrets[config.token_pepper_secret];
    if (typeof pepper !== "string" || pepper.length < 32) throw new Error("operator_secret_unavailable");
    return JSON.parse(await generated.operate_auth(scope.auth.batch, JSON.stringify({ ...parameters, action: operation }), pepper));
  }
  if (owner === "access") {
    if (operation === "setup") { await generated.setup_access(scope.access.batch); return { completed: true }; }
    if (operation === "verify") { await generated.verify_access(scope.access.batch); return { verified: true }; }
    if (!["bootstrap", "revoke_role", "list_roles"].includes(operation) || typeof body.credential !== "string") throw new Error("unsupported_operator_action");
    let authCalls = 0, accessCalls = 0;
    try {
      return JSON.parse(await generated.operate_access(
        input => { authCalls++; return scope.auth.batch(input); },
        input => { accessCalls++; return scope.access.batch(input); },
        scope.operator.authConfiguration, scope.operator.authSecrets,
        body.credential, JSON.stringify({ ...parameters, action: operation }),
      ));
    } catch (error) {
      const stage = typeof error === "string" && /^operator_access_[a-z_]{1,64}$/.test(error)
        ? error : "operator_access_unavailable";
      throw Object.assign(new Error("private_access_unavailable"), {
        operatorProgress: { stage, auth_calls: authCalls, access_calls: accessCalls },
      });
    }
  }
  if (!["audit", "approval", "management"].includes(owner)) throw new Error("unsupported_operator_action");
  if (operation === "setup") { await scope.operator.setup[owner](); return { completed: true }; }
  if (operation === "verify") {
    if (owner === "management") {
      const result = await scope.management.call({ action: "inspect" });
      if (result?.kind !== "ready") throw new Error("operator_readiness_unavailable");
    } else { await scope[owner].readiness(); }
    return { verified: true };
  }
  if (owner === "management" && ["grant", "revoke"].includes(operation)) {
    await scope.operator.qualify(parameters.deployment, parameters.subject, operation === "revoke");
    return { completed: true };
  }
  if (owner === "management" && operation === "inspect_record") {
    inspection(parameters);
    const row = await scope.management.call({ action: "load", ...parameters });
    if (row?.error === "not_found") return { record_exists: false };
    if (row?.kind !== "record" || typeof row.value !== "object" || !row.value) throw new Error("operator_record_unavailable");
    const record = row.value;
    return {
      record_exists: true,
      field_names: Object.keys(record).sort(),
      state: typeof record.response?.state === "string" ? record.response.state : null,
      entry_id: typeof record.entry_id === "string" ? record.entry_id : null,
      intent_digest_present: typeof record.digest === "string" && /^[a-f0-9]{64}$/.test(record.digest),
      response_fields: record.response && typeof record.response === "object" ? Object.keys(record.response).sort() : [],
      intent_fields: record.intent && typeof record.intent === "object" ? Object.keys(record.intent).sort() : [],
    };
  }
  if (owner === "audit" && operation === "inspect_operation") {
    inspection(parameters);
    const events = await scope.audit.list({limit:100,event_name:"management-operation",
      source_instance:"example.ops-management/default",scope_type:"deployment",scope_id:parameters.deployment,
      resource_type:"management-operation",resource_id:parameters.operation_id,correlation_id:parameters.operation_id});
    if (!Array.isArray(events) || events.length > 100) throw new Error("operator_events_unavailable");
    return { event_count:events.length,events:events.map(event=>({
      field_names:Object.keys(event).sort(),action:event.action,outcome:event.outcome,
      state:event.metadata?.state ?? null,decision:event.metadata?.decision ?? null,
      receipt_present:typeof event.metadata?.receipt==="string"&&event.metadata.receipt.length>0,
      digest_present:typeof event.metadata?.intent_digest==="string"&&/^[a-f0-9]{64}$/.test(event.metadata.intent_digest),
      source_matches:event.source_instance==="example.ops-management/default",
      actor_role:event.actor_id==="alice"?"requester":event.actor_id==="bob"?"decider":"other",
    })) };
  }
  if (owner === "audit" && operation === "verify_operation") {
    return JSON.parse(await generated.inspect_audit(scope.audit, JSON.stringify(parameters)));
  }
  if (owner === "approval" && operation === "inspect_operation") {
    inspection(parameters);
    const record = await scope.approval.read({id:parameters.operation_id,requester:"example.ops-management/default"});
    if (!record) return {record_exists:false};
    if (typeof record!=="object") throw new Error("operator_approval_unavailable");
    return {record_exists:true,field_names:Object.keys(record).sort(),status:record.status,revision:record.revision,
      digest_present:typeof record.intent_digest==="string"&&/^[a-f0-9]{64}$/.test(record.intent_digest),
      requester_matches:record.requester_instance==="example.ops-management/default"};
  }
  throw new Error("unsupported_operator_action");
}

async function handleHttp(input, scope) {
  let body;
  try {
    body = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(JSON.parse(input).body)));
    if (!body || typeof body !== "object" || Array.isArray(body) || !ID.test(body.action_id) || typeof body.owner !== "string" || typeof body.operation !== "string" || Object.keys(body).some(key => !["action_id", "owner", "operation", "parameters", "credential"].includes(key))) {
      return response(400, { state: "rejected" });
    }
  } catch { return response(400, { state: "rejected" }); }
  const mutating = !READS.has(body.operation);
  // This isolate fence is supplemental. The client owns the durable pre-dispatch marker.
  if (mutating) {
    if (actions.has(body.action_id) || actions.size >= 256) return response(409, { state: "not_replayed" });
    actions.set(body.action_id, "started");
  }
  try {
    const result = await dispatch(body, scope);
    if (mutating) actions.set(body.action_id, "completed");
    return response(200, { state: "completed", result });
  } catch (error) {
    if (mutating) actions.set(body.action_id, "unknown");
    return response(503, { state: mutating ? "unknown" : "unavailable",
      ...(error.operatorProgress ? { progress: error.operatorProgress } : {}) });
  }
}

export default createWorkersHttpHost({
  bindings: { ...generated, handle_http: handleHttp }, wasmModule: wasm,
  createScope: selectedScope,
  limits: { eventLimitMs: 30000, bodyReadTimeoutMs: 5000, maxRequestBodyBytes: 65536, maxResponseBodyBytes: 65536, maxConcurrent: 1 },
});
