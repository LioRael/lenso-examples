import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const fixtureRoot = new URL("../examples/support-system/", import.meta.url);

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
    system.contracts.map(({ contractId, producerId }) => [contractId, producerId]),
    [
      ["support-http", "support-ticket-service"],
      ["support-grpc", "support-sla-service"],
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
  assert.deepEqual(sla.serviceContracts.map(({ contractId }) => contractId), [
    "support-grpc",
  ]);
});
