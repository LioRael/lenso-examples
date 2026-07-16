import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";
import test from "node:test";

const fixtureRoot = new URL("../examples/support-system/", import.meta.url);
const repoRoot = new URL("../", import.meta.url);
const execFileAsync = promisify(execFile);

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
