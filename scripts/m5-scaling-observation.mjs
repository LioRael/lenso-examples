function byWorkload(items) {
  const grouped = new Map();
  for (const item of items) {
    const workloadId = item.metadata?.labels?.["lenso.dev/workload"];
    if (!workloadId) return null;
    const matches = grouped.get(workloadId) ?? [];
    matches.push(item);
    grouped.set(workloadId, matches);
  }
  return grouped;
}

export function actualScalingIsSatisfied(workloads, deployments, hpas) {
  const scalableWorkloads = workloads.filter((workload) => workload.role !== "migration");
  const deploymentsByWorkload = byWorkload(deployments);
  const hpasByWorkload = byWorkload(hpas);
  if (!deploymentsByWorkload || !hpasByWorkload) return false;
  if (deployments.length !== scalableWorkloads.length) return false;

  const dynamicWorkloads = scalableWorkloads.filter(
    (workload) => workload.scaling.maxReplicas > workload.scaling.minReplicas,
  );
  if (hpas.length !== dynamicWorkloads.length) return false;

  return scalableWorkloads.every((workload) => {
    const deploymentMatches = deploymentsByWorkload.get(workload.workloadId) ?? [];
    if (deploymentMatches.length !== 1) return false;
    const deployment = deploymentMatches[0];
    const readyReplicas = deployment.status?.readyReplicas ?? 0;
    const availableReplicas = deployment.status?.availableReplicas ?? 0;
    const observed = deployment.status?.observedGeneration ?? 0;
    const generation = deployment.metadata?.generation ?? 0;
    if (observed < generation) return false;

    const hpaMatches = hpasByWorkload.get(workload.workloadId) ?? [];
    if (workload.scaling.maxReplicas === workload.scaling.minReplicas) {
      return hpaMatches.length === 0
        && deployment.spec?.replicas === workload.replicas
        && workload.replicas === workload.scaling.minReplicas
        && readyReplicas >= workload.replicas
        && availableReplicas >= workload.replicas;
    }

    if (hpaMatches.length !== 1) return false;
    const hpa = hpaMatches[0];
    const currentReplicas = hpa.status?.currentReplicas ?? 0;
    const desiredReplicas = hpa.status?.desiredReplicas ?? 0;
    const cpuMetrics = (hpa.spec?.metrics ?? []).filter(
      (metric) => metric.type === "Resource" && metric.resource?.name === "cpu",
    );
    return hpa.spec?.scaleTargetRef?.apiVersion === "apps/v1"
      && hpa.spec?.scaleTargetRef?.kind === "Deployment"
      && hpa.spec?.scaleTargetRef?.name === deployment.metadata?.name
      && hpa.spec?.minReplicas === workload.scaling.minReplicas
      && hpa.spec?.maxReplicas === workload.scaling.maxReplicas
      && cpuMetrics.length === 1
      && cpuMetrics[0].resource?.target?.type === "Utilization"
      && cpuMetrics[0].resource?.target?.averageUtilization
        === workload.scaling.targetCpuUtilization
      && currentReplicas >= workload.scaling.minReplicas
      && currentReplicas <= workload.scaling.maxReplicas
      && desiredReplicas >= workload.scaling.minReplicas
      && desiredReplicas <= workload.scaling.maxReplicas
      && deployment.spec?.replicas === desiredReplicas
      && readyReplicas >= desiredReplicas
      && availableReplicas >= desiredReplicas;
  });
}
