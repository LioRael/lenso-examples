import fs from "node:fs";
import path from "node:path";

const root = path.join(process.cwd(), "fixtures/launchpad/support-desk-addon");
const launchpad = JSON.parse(
  fs.readFileSync(path.join(root, "launchpad.json"), "utf8"),
);
const doctor = JSON.parse(
  fs.readFileSync(path.join(root, "dev-doctor.json"), "utf8"),
);
const agentTask = fs.readFileSync(path.join(root, "agent-task.md"), "utf8");
const proofRoot = path.join(
  process.cwd(),
  "fixtures/launchpad/support-desk-proof",
);
const appProof = JSON.parse(
  fs.readFileSync(path.join(proofRoot, "app-proof.json"), "utf8"),
);
const proofAgentTask = fs.readFileSync(
  path.join(proofRoot, "agent-task.md"),
  "utf8",
);
const changePlanRoot = path.join(
  process.cwd(),
  "fixtures/launchpad/support-desk-change-plan",
);
const appChangePlan = JSON.parse(
  fs.readFileSync(path.join(changePlanRoot, "app-change-plan.json"), "utf8"),
);
const changePlanAgentTask = fs.readFileSync(
  path.join(changePlanRoot, "agent-task.md"),
  "utf8",
);
const composerRoot = path.join(
  process.cwd(),
  "fixtures/launchpad/support-desk-composer",
);
const composerPlan = JSON.parse(
  fs.readFileSync(path.join(composerRoot, "app-change-plan.json"), "utf8"),
);
const composerAgentTask = fs.readFileSync(
  path.join(composerRoot, "agent-task.md"),
  "utf8",
);
const capabilityRoot = path.join(
  process.cwd(),
  "fixtures/capabilities/support-sla-pack",
);
const capabilityPack = JSON.parse(
  fs.readFileSync(path.join(capabilityRoot, "lenso.capability.json"), "utf8"),
);
const capabilityLibrary = JSON.parse(
  fs.readFileSync(
    path.join(capabilityRoot, ".lenso/lenso.capability-library.json"),
    "utf8",
  ),
);
const capabilityPlan = JSON.parse(
  fs.readFileSync(path.join(capabilityRoot, "app-change-plan.json"), "utf8"),
);
const capabilityAgentTask = fs.readFileSync(
  path.join(capabilityRoot, "agent-task.md"),
  "utf8",
);

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

assert(launchpad.protocol === "lenso.launchpad.v1", "launchpad protocol");
assert(launchpad.blueprint === "support-desk", "launchpad blueprint");
assert(
  launchpad.supportedAddons.includes("support-sla"),
  "support-sla is supported",
);
assert(
  launchpad.addons.some(
    (addon) =>
      addon.name === "support-sla" &&
      addon.status === "configured" &&
      addon.services.includes("support-sla") &&
      addon.modules.includes("support-sla"),
  ),
  "support-sla addon is configured",
);
assert(
  launchpad.services.some((service) => service.name === "support-sla"),
  "support-sla service is present",
);
assert(doctor.protocol === "lenso.dev-doctor.v1", "doctor protocol");
assert(doctor.status === "ready", "doctor status");
assert(
  doctor.checks.some(
    (check) =>
      check.id === "service-manifest-support-sla" &&
      check.status === "passed",
  ),
  "support-sla manifest check is present",
);
assert(agentTask.includes("## Addons"), "agent task includes addons");
assert(agentTask.includes("## Dev Doctor"), "agent task includes dev doctor");
assert(appProof.protocol === "lenso.app-proof.v1", "app proof protocol");
assert(appProof.status === "ready", "app proof status");
assert(
  appProof.addons.includes("support-sla"),
  "app proof includes support-sla",
);
assert(proofAgentTask.includes("## App Proof"), "agent task includes app proof");
assert(
  proofAgentTask.includes("Existing service source files are user code."),
  "agent task includes app proof source boundary",
);
assert(
  appChangePlan.protocol === "lenso.app-change-plan.v1",
  "app change plan protocol",
);
assert(appChangePlan.status === "changes", "app change plan status");
assert(
  appChangePlan.changes.some(
    (change) =>
      change.kind === "addon-apply" &&
      change.name === "support-sla" &&
      change.safe === true,
  ),
  "app change plan includes safe support-sla addon apply",
);
assert(
  appChangePlan.nextCommand === "lenso app apply .lenso/app-change-plan.json",
  "app change plan next command",
);
assert(
  changePlanAgentTask.includes("## App Change Plan"),
  "agent task includes app change plan",
);
assert(
  changePlanAgentTask.includes("Generated control-plane files may be planned and applied."),
  "agent task includes app change plan generated boundary",
);
assert(
  composerPlan.protocol === "lenso.app-change-plan.v1",
  "composer fixture uses app change plan protocol",
);
assert(
  composerPlan.composition?.protocol === "lenso.app-composition.v1",
  "composer fixture includes app composition",
);
assert(
  composerPlan.composition.requestedAddons.includes("support-sla"),
  "composer fixture requests support-sla",
);
assert(
  composerPlan.composition.requestedAddons.includes("customer-profile"),
  "composer fixture requests customer-profile",
);
assert(
  composerPlan.composition.appliedAddons.includes("customer-profile"),
  "composer fixture applies customer-profile",
);
assert(
  composerAgentTask.includes("## App Change Plan"),
  "composer agent task includes app change plan",
);
assert(
  composerAgentTask.includes("Services are out-of-process providers"),
  "composer agent task includes service boundary",
);
assert(
  capabilityPack.protocol === "lenso.capability-pack.v1",
  "capability pack protocol",
);
assert(capabilityPack.name === "support-sla", "capability pack name");
assert(
  capabilityLibrary.protocol === "lenso.capability-library.v1",
  "capability library protocol",
);
assert(
  capabilityLibrary.packs.some(
    (pack) => pack.name === "support-sla" && pack.path === ".",
  ),
  "capability library includes support-sla",
);
assert(
  capabilityPack.supports.blueprints.includes("support-desk"),
  "capability pack supports support-desk",
);
assert(
  capabilityPlan.composition?.requestedPacks.includes("support-sla"),
  "capability plan requests support-sla pack",
);
assert(
  capabilityPlan.composition?.appliedPacks.includes("support-sla"),
  "capability plan applies support-sla pack",
);
assert(
  capabilityPlan.composition?.capabilityPacks.some(
    (pack) => pack.name === "support-sla" && pack.status === "applied",
  ),
  "capability plan includes applied support-sla pack state",
);
assert(
  capabilityPlan.composition?.packFit.some(
    (fit) => fit.name === "support-sla" && fit.status === "applied",
  ),
  "capability plan includes applied support-sla fit state",
);
assert(
  capabilityAgentTask.includes("## Capability Scope"),
  "capability agent task includes capability scope",
);
assert(
  capabilityAgentTask.includes("Pack fit: support-sla"),
  "capability agent task includes pack fit",
);
assert(
  capabilityAgentTask.includes("Runtime queues, retries, Outbox"),
  "capability agent task includes host-owned runtime boundary",
);

console.log("launchpad fixtures ok");
