import fs from "node:fs";
import path from "node:path";

const fixtures = [
  {
    environment: "staging",
    host: "support-staging.example.com",
    namespace: "lenso-staging",
  },
  {
    environment: "prod",
    host: "support.example.com",
    namespace: "lenso-prod",
  },
];

for (const fixtureConfig of fixtures) {
  const fixture = path.join(
    process.cwd(),
    `examples/support-ticket/kubernetes/operator/${fixtureConfig.environment}/lensoserviceprovider.yaml`,
  );
  const contents = fs.readFileSync(fixture, "utf8");
  const required = [
    "apiVersion: lenso.dev/v1alpha1",
    "kind: LensoServiceProvider",
    "serviceName: support-suite-provider",
    `environment: ${fixtureConfig.environment}`,
    `namespace: ${fixtureConfig.namespace}`,
    `host: ${fixtureConfig.host}`,
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
}

console.log("operator fixtures ok");
