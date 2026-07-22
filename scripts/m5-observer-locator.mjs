export function trustedObserverLocator(kind, environment) {
  if (!new Set(["staging", "production"]).has(environment)) {
    throw new Error("trusted observer environment is not allowlisted");
  }
  if (!new Set(["operator", "gateway"]).has(kind)) {
    throw new Error("trusted observer kind is not allowlisted");
  }
  return {
    namespace: `lenso-m5-${environment}`,
    resource: kind === "gateway"
      ? "configmap/lenso-m5-gateway"
      : `lensoautonomousservice/service-support-${environment}`,
  };
}
