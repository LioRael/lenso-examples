import fs from "node:fs";
import path from "node:path";

const fixture = path.join(
  process.cwd(),
  "examples/support-ticket/kubernetes/operator/staging/lensoserviceprovider.yaml",
);
const contents = fs.readFileSync(fixture, "utf8");
const required = [
  "apiVersion: lenso.dev/v1alpha1",
  "kind: LensoServiceProvider",
  "serviceName: support-suite-provider",
  "environment: staging",
  "modules:",
  "support-ticket",
  "autoscaling:",
  "networkPolicy:",
];

for (const value of required) {
  if (!contents.includes(value)) {
    throw new Error(`${fixture} is missing ${value}`);
  }
}

console.log("operator fixture ok");
