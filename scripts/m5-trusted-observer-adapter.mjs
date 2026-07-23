#!/usr/bin/env node

import { createHash, createPrivateKey, sign } from "node:crypto";
import { execFileSync } from "node:child_process";
import process from "node:process";
import { trustedObserverLocator } from "./m5-observer-locator.mjs";

const request = JSON.parse(process.argv[2] ?? "null");
if (!request || !["operator", "gateway"].includes(request.kind)) {
  throw new Error("trusted observer requires an operator or gateway read request");
}
const authorityId = process.env.LENSO_OBSERVER_AUTHORITY_ID;
const privateKeyPem = process.env.LENSO_OBSERVER_PRIVATE_KEY_PEM;
if (!authorityId || !privateKeyPem) throw new Error("trusted observer signing authority is unavailable");
const privateKey = createPrivateKey(privateKeyPem);
const kubectl = process.env.KUBECTL_BIN ?? "kubectl";

function readResource(kind, environment, resourceName = null) {
  const locator = trustedObserverLocator(kind, environment, resourceName);
  return JSON.parse(execFileSync(kubectl, [
    "get", locator.resource, "--namespace", locator.namespace, "--output=json",
  ], { encoding: "utf8", env: process.env }));
}

function digestJson(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function signature(subject) {
  return sign(null, Buffer.from(subject), privateKey).toString("base64");
}

function nginxQuoted(value) {
  if (typeof value !== "string" || /[\u0000\r\n]/u.test(value)) {
    throw new Error("Gateway plan contains an unsafe Nginx value");
  }
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function publicPathPattern(publicPath) {
  if (typeof publicPath !== "string" || publicPath === "/" || !publicPath.startsWith("/")
      || publicPath.endsWith("/")) throw new Error("Gateway public path is invalid");
  return `^/${publicPath.slice(1).split("/").map((segment) => {
    if (/^\{[A-Za-z][A-Za-z0-9_]{0,63}\}$/u.test(segment)) return "[^/]+";
    if (!segment || !/^[A-Za-z0-9._-]+$/u.test(segment)) throw new Error("Gateway public path is unsafe");
    return segment.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  }).join("/")}$`;
}

function renderGatewayConfiguration(plan) {
  if (plan.routes.length !== 1) throw new Error("trusted Gateway adapter requires one explicit route");
  const route = plan.routes[0];
  const allowedOrigin = route.cors.allowedOrigins[0];
  const quotedAllowedOrigin = nginxQuoted(allowedOrigin);
  const locationPattern = nginxQuoted(publicPathPattern(route.publicPath));
  const upstream = `service-support-${plan.environment}-support-api:8080`;
  return [
    "limit_req_zone $binary_remote_addr zone=m5_edge:10m rate=100r/m;",
    "map $http_authorization $m5_auth_ok { default 1; \"\" 0; }",
    `map $http_origin $m5_cors_ok { default 0; \"\" 1; ${quotedAllowedOrigin} 1; }`,
    "server {",
    "  listen 80;",
    `  location ~ ${locationPattern} {`,
    "    if ($m5_auth_ok = 0) { return 401; }",
    "    if ($m5_cors_ok = 0) { return 403; }",
    "    limit_req zone=m5_edge burst=1 nodelay;",
    `    add_header Access-Control-Allow-Origin ${quotedAllowedOrigin} always;`,
    "    add_header Vary Origin always;",
    `    proxy_pass http://${upstream};`,
    "  }",
    "  location / { return 404; }",
    "}",
  ].join("\n");
}

function validateGatewayPlan(plan) {
  const configurationIdentity = digestJson([
    plan.edgeContractDigest,
    plan.edgeReleaseId,
    plan.edgeReleaseDigest,
    plan.operationCatalogDigest,
    plan.edgeProviderId,
    plan.edgeProviderProof,
    plan.environment,
    plan.gatewayAdapter,
    plan.publicOrigin,
    plan.routes,
  ]);
  if (configurationIdentity !== plan.configurationIdentity) {
    throw new Error("trusted Gateway adapter rejected a modified configuration identity");
  }
  const binding = {
    environment: plan.environment,
    gatewayAdapter: plan.gatewayAdapter,
    publicOrigin: plan.publicOrigin,
    expectedGatewayRevision: plan.expectedGatewayRevision,
  };
  const planDigest = digestJson([
    plan.protocol,
    plan.edgeContractId,
    plan.edgeContractDigest,
    plan.edgeReleaseId,
    plan.edgeReleaseDigest,
    plan.operationCatalogDigest,
    plan.edgeProviderId,
    plan.edgeProviderProof,
    binding,
    plan.configurationIdentity,
    plan.routes,
    plan.diff,
    plan.drifted,
    plan.issues,
    plan.nextActions,
    plan.effects,
  ]);
  if (plan.planDigest !== planDigest || plan.planId !== `gateway-plan:${planDigest}`) {
    throw new Error("trusted Gateway adapter rejected a modified plan identity");
  }
}

function requiredText(value, field) {
  const result = value?.[field];
  if (typeof result !== "string" || !result) throw new Error(`live resource field ${field} is required`);
  return result;
}

function observeOperator() {
  const resource = readResource("operator", request.environment, request.resourceName);
  const { spec, status, metadata } = resource;
  const planId = spec.evidenceReferences.find((value) => value.startsWith("deployment-plan:sha256:"));
  if (!planId) throw new Error("live Operator resource does not identify its Deployment plan");
  const workloads = status.workloads ?? [];
  const desiredWorkloadDigests = Object.fromEntries(workloads.map((item) => [item.workloadId, item.desiredDigest]));
  const observedWorkloadDigests = Object.fromEntries(
    workloads.filter((item) => item.observedDigest).map((item) => [item.workloadId, item.observedDigest]),
  );
  const workloadHealth = Object.fromEntries(workloads.map((item) => [item.workloadId, item.ready === true]));
  const fresh = metadata.generation === status.observedGeneration;
  const drifted = status.drifted !== false
    || spec.releaseId !== status.observedReleaseId
    || spec.releaseDigest !== status.observedReleaseDigest
    || spec.configRevisionId !== status.configRevisionId;
  const passed = status.state === "ready" && fresh && !drifted;
  const claims = {
    protocol: "lenso.operator-observation-claims.v1",
    serviceId: requiredText(spec, "serviceId"),
    environment: requiredText(spec, "environment"),
    deploymentPlanId: planId,
    deploymentPlanDigest: planId.slice("deployment-plan:".length),
    expectedEnvironmentRevision: spec.expectedEnvironmentRevision,
    environmentRevision: spec.expectedEnvironmentRevision + 1,
    authorityContext: request.authorityContext ?? planId,
    resourceUid: requiredText(metadata, "uid"),
    resourceVersion: requiredText(metadata, "resourceVersion"),
    desiredReleaseId: requiredText(spec, "releaseId"),
    desiredReleaseDigest: requiredText(spec, "releaseDigest"),
    observedReleaseId: requiredText(status, "observedReleaseId"),
    observedReleaseDigest: requiredText(status, "observedReleaseDigest"),
    desiredWorkloadDigests,
    observedWorkloadDigests,
    workloadHealth,
    configRevisionId: requiredText(status, "configRevisionId"),
    state: requiredText(status, "state"),
    rolloutPhase: requiredText(status, "rolloutPhase"),
    rollbackState: requiredText(status, "rollbackState"),
    drifted,
    fresh,
    decision: passed ? "passed" : "blocked",
  };
  const observationDigest = digestJson(claims);
  return {
    protocol: "lenso.operator-observation.v1",
    observationId: `operator-observation:${observationDigest}`,
    observationDigest,
    authorityId,
    authorityProof: signature(observationDigest),
    claims,
    serviceId: claims.serviceId,
    environment: claims.environment,
    resourceUid: claims.resourceUid,
    resourceVersion: claims.resourceVersion,
    expectedEnvironmentRevision: claims.expectedEnvironmentRevision,
    environmentRevision: claims.environmentRevision,
    authorityContext: claims.authorityContext,
    desiredReleaseId: claims.desiredReleaseId,
    desiredReleaseDigest: claims.desiredReleaseDigest,
    observedReleaseId: claims.observedReleaseId,
    observedReleaseDigest: claims.observedReleaseDigest,
    configRevisionId: claims.configRevisionId,
    state: claims.state,
    rolloutPhase: claims.rolloutPhase,
    rollbackState: claims.rollbackState,
    workloads,
    issues: status.issues ?? [],
    nextActions: status.nextActions ?? [],
    evidenceReferences: status.evidenceReferences ?? [],
    fresh,
    drifted,
    decision: claims.decision,
    effects: {
      mutatesEnvironment: false,
      mutatesConfiguration: false,
      mutatesGateway: false,
      mutatesDeployment: false,
      appendsLedger: false,
    },
  };
}

function observeGateway() {
  validateGatewayPlan(request.plan);
  const config = readResource("gateway", request.plan.environment);
  const identity = config.metadata.annotations?.["lenso.dev/gateway-configuration-identity"] ?? "";
  const revisionText = config.metadata.annotations?.["lenso.dev/gateway-revision"] ?? "";
  const parsed = /^(0|[1-9][0-9]*)$/u.test(revisionText) ? Number(revisionText) : Number.NaN;
  const revision = Number.isSafeInteger(parsed) ? parsed : 0;
  const fresh = identity === request.plan.configurationIdentity
    && config.data?.["default.conf"] === renderGatewayConfiguration(request.plan)
    && Number.isSafeInteger(parsed)
    && revision === request.plan.expectedGatewayRevision;
  const content = [
    "lenso.gateway-observation.v1",
    request.plan.planId,
    request.plan.planDigest,
    request.plan.environment,
    request.plan.edgeReleaseId,
    request.plan.edgeReleaseDigest,
    requiredText(config.metadata, "uid"),
    requiredText(config.metadata, "resourceVersion"),
    request.authorityContext,
    identity,
    revision,
    request.observedAfter,
    fresh,
    authorityId,
  ];
  const observationDigest = digestJson(content);
  const observationId = `gateway-observation:${observationDigest}`;
  return {
    protocol: "lenso.gateway-observation.v1",
    observationId,
    planId: request.plan.planId,
    planDigest: request.plan.planDigest,
    environment: request.plan.environment,
    releaseId: request.plan.edgeReleaseId,
    releaseDigest: request.plan.edgeReleaseDigest,
    resourceUid: config.metadata.uid,
    resourceVersion: config.metadata.resourceVersion,
    authorityContext: request.authorityContext,
    configurationIdentity: identity,
    revision,
    observedAfter: request.observedAfter,
    fresh,
    providerId: authorityId,
    providerProof: signature(observationId),
  };
}

process.stdout.write(`${JSON.stringify(request.kind === "operator" ? observeOperator() : observeGateway())}\n`);
