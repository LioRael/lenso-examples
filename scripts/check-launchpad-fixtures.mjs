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

console.log("launchpad fixtures ok");
