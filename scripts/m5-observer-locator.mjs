export function trustedObserverLocator(kind, environment, resourceName = null) {
  if (!new Set(["staging", "production"]).has(environment)) {
    throw new Error("trusted observer environment is not allowlisted");
  }
  if (!new Set(["operator", "gateway"]).has(kind)) {
    throw new Error("trusted observer kind is not allowlisted");
  }
  const namespace = `lenso-m5-${environment}`;
  if (kind === "gateway") {
    if (resourceName !== null && resourceName !== "lenso-m5-gateway") {
      throw new Error("trusted observer resource is not allowlisted");
    }
    return { namespace, resource: "configmap/lenso-m5-gateway" };
  }
  const defaultName = `service-support-${environment}`;
  const allowedNames = new Set([
    defaultName,
    ...(environment === "production" ? ["service-support-production-canary"] : []),
  ]);
  const selectedName = resourceName ?? defaultName;
  if (!allowedNames.has(selectedName)) {
    throw new Error("trusted observer resource is not allowlisted");
  }
  return {
    namespace,
    resource: `lensoautonomousservice/${selectedName}`,
  };
}
