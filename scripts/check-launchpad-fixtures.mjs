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

console.log("launchpad fixtures ok");
