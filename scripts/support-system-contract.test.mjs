import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";
import test from "node:test";
import { trustedObserverLocator } from "./m5-observer-locator.mjs";
import { canonicalDataPlaneOperationResults } from "./m5-outage-observation.mjs";
import { actualScalingIsSatisfied } from "./m5-scaling-observation.mjs";

const fixtureRoot = new URL("../examples/support-system/", import.meta.url);
const repoRoot = new URL("../", import.meta.url);
const execFileAsync = promisify(execFile);

test("outage observations use the canonical Rust Data Plane operation order", () => {
  const operationResults = canonicalDataPlaneOperationResults({
    compensation: true,
    direct_request: true,
    durable_workflow: true,
    event: true,
    inbox: true,
    outbox: true,
    retry: true,
    runtime_story: true,
    timer: true,
  });

  assert.deepEqual(Object.keys(operationResults), [
    "direct_request",
    "event",
    "durable_workflow",
    "inbox",
    "outbox",
    "timer",
    "retry",
    "compensation",
    "runtime_story",
  ]);
  assert.throws(
    () => canonicalDataPlaneOperationResults({ ...operationResults, unexpected: true }),
    /unexpected Data Plane operation set/,
  );
});

test("canary scaling evidence is bound to exact Deployments and HPAs", () => {
  const workloads = [{
    workloadId: "support-api",
    role: "api",
    replicas: 1,
    scaling: { minReplicas: 1, maxReplicas: 3, targetCpuUtilization: 70 },
  }];
  const deployment = {
    metadata: {
      name: "service-support-production-support-api",
      generation: 4,
      labels: { "lenso.dev/workload": "support-api" },
    },
    spec: { replicas: 1 },
    status: { observedGeneration: 4, readyReplicas: 1, availableReplicas: 1 },
  };
  const hpa = {
    metadata: { labels: { "lenso.dev/workload": "support-api" } },
    spec: {
      minReplicas: 1,
      maxReplicas: 3,
      scaleTargetRef: {
        apiVersion: "apps/v1",
        kind: "Deployment",
        name: "service-support-production-support-api",
      },
      metrics: [{
        type: "Resource",
        resource: {
          name: "cpu",
          target: { type: "Utilization", averageUtilization: 70 },
        },
      }],
    },
    status: { currentReplicas: 1, desiredReplicas: 1 },
  };

  assert.equal(actualScalingIsSatisfied(workloads, [deployment], [hpa]), true);
  assert.equal(actualScalingIsSatisfied(workloads, [deployment], []), false);
  assert.equal(
    actualScalingIsSatisfied(
      workloads,
      [deployment],
      [{ ...hpa, status: { currentReplicas: 1, desiredReplicas: 2 } }],
    ),
    false,
  );
  assert.equal(
    actualScalingIsSatisfied(workloads, [deployment], [{
      ...hpa,
      spec: {
        ...hpa.spec,
        metrics: [{
          ...hpa.spec.metrics[0],
          resource: {
            ...hpa.spec.metrics[0].resource,
            target: { type: "Utilization", averageUtilization: 99 },
          },
        }],
      },
    }]),
    false,
  );
  assert.equal(
    actualScalingIsSatisfied(workloads, [deployment], [{
      ...hpa,
      spec: {
        ...hpa.spec,
        scaleTargetRef: { ...hpa.spec.scaleTargetRef, name: "wrong-deployment" },
      },
    }]),
    false,
  );
  assert.equal(
    actualScalingIsSatisfied(
      [{ ...workloads[0], scaling: { minReplicas: 1, maxReplicas: 1 } }],
      [{ ...deployment, spec: { replicas: 1 } }],
      [],
    ),
    true,
  );
});

async function readJson(path) {
  return JSON.parse(await readFile(new URL(path, fixtureRoot), "utf8"));
}

test("support System preserves two autonomous Service identities", async () => {
  const system = await readJson("lenso.system.json");
  const services = new Map(
    system.autonomousServices.map((service) => [service.serviceId, service]),
  );

  assert.equal(system.protocol, "lenso.system.v2");
  assert.deepEqual([...services.keys()], [
    "support-sla-service",
    "support-ticket-service",
  ]);
  assert.deepEqual(services.get("support-ticket-service").modules, ["support-ticket"]);
  assert.deepEqual(services.get("support-sla-service").modules, ["support-sla"]);
  for (const [serviceId, service] of services) {
    assert.deepEqual(
      service.workloads.map(({ workloadId, role }) => [workloadId, role]),
      [
        [`${serviceId}-api`, "api"],
        [`${serviceId}-worker`, "worker"],
        [`${serviceId}-migrate`, "migration"],
      ],
    );
  }

  assert.deepEqual(
    system.contracts.map(({ contractId, producerId, tenancyMode }) => [
      contractId,
      producerId,
      tenancyMode,
    ]),
    [
      ["support-http", "support-ticket-service", "none"],
      ["support-grpc", "support-sla-service", "none"],
      ["support-ticket-opened", "support-ticket-service", "required"],
    ],
  );
  assert.deepEqual(system.consumers, [
    {
      consumerId: "support-ticket-support-sla",
      ownerKind: "autonomous_service",
      ownerId: "support-ticket-service",
      contractId: "support-grpc",
      tenancyMode: "none",
    },
    {
      consumerId: "support-sla-ticket-opened",
      ownerKind: "autonomous_service",
      ownerId: "support-sla-service",
      contractId: "support-ticket-opened",
      tenancyMode: "required",
    },
  ]);
  assert.deepEqual(system.host, {
    hostId: "support-compat-host",
    modules: ["auth"],
  });
  assert.equal(
    system.consumers.some(({ ownerKind }) => ownerKind === "host"),
    false,
  );
  assert.deepEqual(system.providers, [
    {
      providerId: "support-suite-provider",
      modules: ["support-notification"],
    },
  ]);
  assert.equal(
    system.consumers.some(({ ownerKind }) => ownerKind === "provider"),
    false,
  );
});

test("sandbox launches every Workload and exposes only API endpoints", async () => {
  const sandbox = await readJson("lenso.system-sandbox.json");
  assert.equal(sandbox.protocol, "lenso.system-sandbox.v1");

  const workloads = sandbox.services.flatMap(({ serviceId, workloads }) =>
    workloads.map((workload) => ({ serviceId, ...workload })),
  );
  assert.equal(workloads.length, 6);
  assert.deepEqual(
    workloads.filter(({ endpoint }) => endpoint).map(({ serviceId }) => serviceId),
    ["support-sla-service", "support-ticket-service"],
  );
  for (const workload of workloads) {
    assert.match(workload.command[0], /support-system-workload$/);
    assert.deepEqual(workload.command.slice(1), [
      workload.serviceId,
      workload.workloadId.split("-").at(-1),
    ]);
    assert.equal(
      workload.env.LENSO_SERVICE_STORE_ID,
      workload.serviceId === "support-ticket-service"
        ? "support-ticket-store"
        : "support-sla-store",
    );
  }

  const ticketApi = workloads.find(
    ({ workloadId }) => workloadId === "support-ticket-service-api",
  );
  assert.deepEqual(ticketApi.scenarioCommand, [
    "./target/debug/support-system-workload",
    "scenario",
  ]);
  assert.deepEqual(
    sandbox.scenarios.map(({ scenarioId, fault, callPolicy }) => ({
      scenarioId,
      kind: fault.kind,
      serviceId: fault.serviceId,
      workloadId: fault.workloadId,
      idempotent: callPolicy.idempotent,
    })),
    [
      {
        scenarioId: "deadline-timeout",
        kind: "timeout",
        serviceId: "support-ticket-service",
        workloadId: "support-ticket-service-api",
        idempotent: true,
      },
      {
        scenarioId: "support-ticket-api-crash",
        kind: "workload_crash",
        serviceId: "support-ticket-service",
        workloadId: "support-ticket-service-api",
        idempotent: false,
      },
      {
        scenarioId: "support-ticket-api-partial-unavailability",
        kind: "partial_unavailability",
        serviceId: "support-ticket-service",
        workloadId: "support-ticket-service-api",
        idempotent: false,
      },
    ],
  );
});

test("Service definitions retain isolated Store and contract ownership", async () => {
  const ticket = await readJson("services/support-ticket/lenso.service.json");
  const sla = await readJson("services/support-sla/lenso.service.json");

  assert.equal(ticket.serviceId, "support-ticket-service");
  assert.equal(sla.serviceId, "support-sla-service");
  assert.deepEqual(ticket.modules, ["support-ticket"]);
  assert.deepEqual(sla.modules, ["support-sla"]);
  assert.deepEqual(ticket.stores, [
    { storeId: "support-ticket-store", serviceId: "support-ticket-service" },
  ]);
  assert.deepEqual(sla.stores, [
    { storeId: "support-sla-store", serviceId: "support-sla-service" },
  ]);
  assert.deepEqual(ticket.serviceContracts.map(({ contractId }) => contractId), [
    "support-http",
  ]);
  assert.deepEqual(
    ticket.eventContracts.map(
      ({ contractId, moduleId, version, tenancyMode, artifact, context }) => ({
        contractId,
        moduleId,
        version,
        tenancyMode,
        artifact,
        requiredContext: context.required,
      }),
    ),
    [
      {
        contractId: "ticket-opened",
        moduleId: "support-ticket",
        version: "v1",
        tenancyMode: "required",
        artifact: {
          format: "json_schema",
          path: "contracts/support.ticket-opened.v1.schema.json",
        },
        requiredContext: [
          "story",
          "trace",
          "service_principal",
          "delegated_actor",
          "tenant",
          "deadline",
          "idempotency_key",
          "causation",
          "region",
        ],
      },
    ],
  );
  assert.deepEqual(sla.serviceContracts.map(({ contractId }) => contractId), [
    "support-grpc",
  ]);
});

test("one public command owns the M1 acceptance proof", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m1"], "node scripts/m1-acceptance.mjs");
});

test("one public command owns the M2 acceptance proof", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m2"], "node scripts/m2-acceptance.mjs");
  assert.equal(
    pkg.scripts["acceptance:m2:production"],
    "node scripts/m2-production-evidence.mjs",
  );
});

test("Runtime Console serves the SPA entry point at the root path", async () => {
  const dockerfile = await readFile(
    new URL("infrastructure/m5-runtime-console.Dockerfile", repoRoot),
    "utf8",
  );

  assert.match(
    dockerfile,
    /location = \/ \{ try_files \/index\.html =404; \}/u,
  );
  assert.match(dockerfile, /root \/usr\/share\/nginx\/html;/u);
  assert.match(dockerfile, /index index\.html;/u);
});

test("stable Gateway targets the declared API workload port", async () => {
  const [acceptance, observer] = await Promise.all([
    readFile(new URL("scripts/m5-acceptance.mjs", repoRoot), "utf8"),
    readFile(
      new URL("scripts/m5-trusted-observer-adapter.mjs", repoRoot),
      "utf8",
    ),
  ]);

  for (const source of [acceptance, observer]) {
    assert.match(
      source,
      /service-support-\$\{plan\.environment\}-support-api:8080/u,
    );
  }
});

test("one public command owns the M3 acceptance proof", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m3"], "node scripts/m3-acceptance.mjs");
});

test("one public command owns the M4 safe extraction proof", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m4"], "node scripts/m4-acceptance.mjs");
  const { stdout } = await execFileAsync(process.execPath, ["scripts/m4-acceptance.mjs", "--describe"], { cwd: repoRoot });
  const proof = JSON.parse(stdout);
  assert.equal(proof.artifactVersion, "lenso.m4-acceptance-description.v1");
  assert.equal(proof.publicSeam, "support-system");
  assert.deepEqual(proof.authorityHistory, ["linked", "provisional", "linked", "provisional", "autonomous"]);
  assert.equal(proof.priorGuarantees, "m3_acceptance");
  assert.equal(proof.providerCompatibility, "independent_host_managed_smoke");
  assert.equal(proof.productionAuthorityRequired, false);
});

test("one public command owns the M5 production delivery proof", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m5"], "node scripts/m5-acceptance.mjs");
  const [manifest, acceptance] = await Promise.all([
    readFile(new URL("examples/support-system/Cargo.toml", repoRoot), "utf8"),
    readFile(new URL("scripts/m5-acceptance.mjs", repoRoot), "utf8"),
  ]);
  assert.match(manifest, /name = "support-system-m5-attest-outage"/u);
  assert.match(acceptance, /--bin", "support-system-m5-attest-outage"/u);
  const { stdout } = await execFileAsync(
    process.execPath,
    ["scripts/m5-acceptance.mjs", "--describe"],
    { cwd: repoRoot },
  );
  const proof = JSON.parse(stdout);
  assert.equal(proof.artifactVersion, "lenso.m5-acceptance-description.v1");
  assert.equal(proof.publicSeam, "support-system");
  assert.equal(proof.priorGuarantees, "m4_acceptance");
  assert.equal(proof.kubernetesRequired, true);
  assert.equal(proof.actualKubernetesApiRequired, true);
  assert.equal(proof.runningOperatorRequired, true);
  assert.equal(proof.runtimeConsoleDeploymentAuthority, false);
  assert.ok(proof.workflow.includes("migration_first_staging"));
  assert.ok(proof.workflow.includes("stale_target_zero_mutation"));
  assert.ok(proof.workflow.includes("safe_operator_rollback"));
  assert.ok(proof.workflow.includes("system_plane_outage_continuity"));
});

test("one public command owns the M6 candidate and published acceptance shell", async () => {
  const pkg = JSON.parse(await readFile(new URL("package.json", repoRoot), "utf8"));
  assert.equal(pkg.scripts["acceptance:m6"], "node scripts/m6-acceptance.mjs");
  assert.equal(
    pkg.scripts["acceptance:m6:environment"],
    "node scripts/m6-environment-verification.mjs",
  );
  const { stdout } = await execFileAsync(
    process.execPath,
    ["scripts/m6-acceptance.mjs", "--describe"],
    { cwd: repoRoot },
  );
  const proof = JSON.parse(stdout);
  assert.equal(proof.artifactVersion, "lenso.m6-acceptance-description.v1");
  assert.equal(proof.publicSeam, "pnpm acceptance:m6");
  assert.deepEqual(proof.modes, ["candidate", "published"]);
  assert.equal(proof.freshStarterOutsideFrameworkWorkspaces, true);
  assert.equal(proof.mutableOrLocalArtifactsRejected, true);
  assert.equal(proof.candidateCanClaimGa, false);
  assert.equal(proof.productionMutation, false);
  assert.match(proof.environmentVerification.nats, /JetStream/u);
  assert.match(proof.environmentVerification.spiffe, /SPIRE/u);
});

test("M5 trusted observer cannot select an alternate namespace", () => {
  assert.deepEqual(trustedObserverLocator("operator", "staging"), {
    namespace: "lenso-m5-staging",
    resource: "lensoautonomousservice/service-support-staging",
  });
  assert.deepEqual(trustedObserverLocator("gateway", "production"), {
    namespace: "lenso-m5-production",
    resource: "configmap/lenso-m5-gateway",
  });
  assert.deepEqual(
    trustedObserverLocator("operator", "production", "service-support-production-canary"),
    {
      namespace: "lenso-m5-production",
      resource: "lensoautonomousservice/service-support-production-canary",
    },
  );
  assert.throws(() => trustedObserverLocator("operator", "staging-shadow"), /not allowlisted/u);
  assert.throws(
    () => trustedObserverLocator("operator", "production", "service-support-production-shadow"),
    /resource is not allowlisted/u,
  );
});

test("M2 acceptance describes every deterministic scenario and separate production proof", async () => {
  const { stdout } = await execFileAsync(
    process.execPath,
    ["scripts/m2-acceptance.mjs", "--describe"],
    { cwd: repoRoot },
  );
  const proof = JSON.parse(stdout);

  assert.equal(proof.artifactVersion, "lenso.m2-acceptance-description.v1");
  assert.equal(proof.publicSeam, "support-system");
  assert.deepEqual(proof.deterministicScenarios, [
    "duplicate",
    "delayed",
    "reordered",
    "poison",
    "producer_restart",
    "consumer_restart",
    "transport_interruption",
    "dead_letter_replay",
    "identity_expiry",
    "wrong_audience",
    "missing_tenant",
    "circuit_open",
    "bulkhead",
    "overload",
    "deadline",
    "fallback",
  ]);
  assert.deepEqual(proof.productionEvidence, {
    transport: "nats_jetstream_real_environment",
    workloadIdentity: "spiffe_spire_real_environment",
    approvalBoundary: true,
  });
  assert.equal(proof.providerCompatibility, "independent_host_managed_smoke");
});

test("M3 acceptance describes the durable workflow and federated evidence seam", async () => {
  const { stdout } = await execFileAsync(
    process.execPath,
    ["scripts/m3-acceptance.mjs", "--describe"],
    { cwd: repoRoot },
  );
  const proof = JSON.parse(stdout);

  assert.equal(proof.artifactVersion, "lenso.m3-acceptance-description.v1");
  assert.equal(proof.publicSeam, "support-system");
  assert.deepEqual(proof.workflow.services, [
    "support-ticket-service",
    "support-sla-service",
  ]);
  assert.deepEqual(proof.workflow.capabilities, [
    "child_workflow",
    "participant_restart",
    "controlled_timeout",
    "exactly_once_compensation",
    "version_pinning",
    "worker_mismatch_rejection",
  ]);
  assert.deepEqual(proof.evidence, [
    "delayed_federation",
    "late_evidence",
    "explicit_segment_gap",
    "workflow_reliability",
  ]);
  assert.deepEqual(proof.planesWithheld, [
    "runtime_console",
    "story_aggregator",
    "system_plane",
  ]);
  assert.equal(proof.priorGuarantees, "m2_acceptance");
  assert.equal(proof.providerCompatibility, "independent_host_managed_smoke");
  assert.equal(proof.externalWorkflowEngineRequired, false);
  assert.equal(proof.kubernetesRequired, false);
  assert.equal(proof.productionAuthorityRequired, false);
});
