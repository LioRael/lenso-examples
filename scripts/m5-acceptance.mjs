import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, generateKeyPairSync } from "node:crypto";
import { access, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalDataPlaneOperationResults } from "./m5-outage-observation.mjs";
import { actualScalingIsSatisfied } from "./m5-scaling-observation.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frameworkRoot = path.dirname(repoRoot);
const lensoRoot = path.resolve(process.env.LENSO_REPO_ROOT ?? path.join(frameworkRoot, "lenso"));
const runtimeConsoleRoot = path.resolve(
  process.env.LENSO_RUNTIME_CONSOLE_ROOT
    ?? path.join(frameworkRoot, path.basename(repoRoot).replace("lenso-examples", "lenso-runtime-console")),
);
const fixtureManifest = path.join(repoRoot, "examples", "support-system", "Cargo.toml");
const trustedObserverAdapter = path.join(repoRoot, "scripts", "m5-trusted-observer-adapter.mjs");
const kindBin = process.env.KIND_BIN ?? "kind";
const kubectlBin = process.env.KUBECTL_BIN ?? "kubectl";
const dockerBin = process.env.DOCKER_BIN ?? "docker";
const lensoBin = process.env.LENSO_CLI_BIN ?? "lenso";
const operatorObservationAuthorityId = "kubernetes-api:lenso-m5-kind";
const gatewayObservationAuthorityId = "gateway-api:lenso-m5-kind";
const operatorObserverKeys = observerKeyPair();
const gatewayObserverKeys = observerKeyPair();
const description = {
  artifactVersion: "lenso.m5-acceptance-description.v1",
  publicSeam: "support-system",
  workflow: [
    "immutable_service_release",
    "supply_chain_trust",
    "policy_equivalence",
    "config_revision_redaction",
    "previous_production_baseline",
    "migration_first_staging",
    "staging_verification",
    "exact_human_approval",
    "stale_target_zero_mutation",
    "production_promotion",
    "canary_failure",
    "safe_operator_rollback",
    "public_edge_and_private_internal_operations",
    "system_plane_outage_continuity",
    "deterministic_cleanup",
  ],
  priorGuarantees: "m4_acceptance",
  providerCompatibility: "independent_host_managed_smoke",
  kubernetesRequired: true,
  actualKubernetesApiRequired: true,
  runningOperatorRequired: true,
  runtimeConsoleDeploymentAuthority: false,
  destructiveCleanupLimitedToEphemeralKindCluster: true,
};

if (process.argv.includes("--describe")) {
  console.log(JSON.stringify(description, null, 2));
} else if (process.argv.includes("--preflight")) {
  try {
    console.log(JSON.stringify(await runStaticPreflight(), null, 2));
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
} else {
  try {
    if (!process.argv.includes("--m5-only")) await run("pnpm", ["acceptance:m4"]);
    const evidence = await runAcceptance();
    console.log(JSON.stringify(evidence, null, 2));
  } catch (error) {
    console.error(JSON.stringify({
      artifactVersion: "lenso.m5-production-delivery-acceptance.v1",
      outcome: "blocked",
      publicSeam: "support-system",
      issue: {
        code: "acceptance_environment_blocked",
        message: error instanceof Error ? error.message : String(error),
        remediation: "Install and start the declared local prerequisites, then rerun the same public command.",
        nextActions: ["Run `pnpm acceptance:m5 -- --m5-only` after restoring the blocked prerequisite."],
      },
      effects: {
        productionMutated: false,
        runtimeConsoleUsedForDeployment: false,
      },
      cleanup: error?.cleanup ?? {
        clusterDeleted: true,
        registryDeleted: true,
        generatedImagesDeleted: true,
        temporaryDirectoryDeleted: true,
      },
    }, null, 2));
    process.exitCode = 1;
  }
}

async function runStaticPreflight() {
  const workDir = await mkdtemp(path.join(os.tmpdir(), "lenso-m5-preflight-"));
  try {
    const candidateDigest = digestJson("m5-static-candidate");
    const previousDigest = digestJson("m5-static-previous");
    const rustBuilderDigest = digestJson("rust:bookworm");
    const debianRuntimeDigest = digestJson("debian:bookworm-slim");
    const buildMaterials = await captureSupportBuildMaterials([
      { uri: `oci://docker.io/library/rust@${rustBuilderDigest}`, digest: rustBuilderDigest },
      { uri: `oci://docker.io/library/debian@${debianRuntimeDigest}`, digest: debianRuntimeDigest },
    ]);
    const supplyChainEvidence = await generateSupplyChainEvidence(
      workDir,
      {
        image: `registry.example.test/lenso/support-system@${candidateDigest}`,
        digest: candidateDigest,
        version: "5.0.0",
      },
      {
        image: `registry.example.test/lenso/support-system@${previousDigest}`,
        digest: previousDigest,
        version: "4.8.0",
      },
      buildMaterials,
    );
    const core = await runCoreEvidence({ candidateDigest, previousDigest, supplyChainEvidence });
    validateCoreEvidence(core, "planning");
    const artifacts = await writeDeliveryArtifacts(workDir, core);
    const cliPolicy = await validateCliArtifacts(workDir, artifacts, core);
    const exports = await exportOperatorResources(workDir, artifacts);
    for (const resource of [exports.staging, exports.previousProduction, exports.candidateProduction]) {
      await access(resource);
    }
    return {
      artifactVersion: "lenso.m5-static-preflight.v1",
      outcome: "passed",
      serviceReleaseId: core.serviceRelease.releaseId,
      policyEvidenceId: cliPolicy.evidenceId,
      stableLogicalReferences: true,
      deterministicOperatorExports: true,
      effects: { productionMutated: false },
    };
  } finally {
    await rm(workDir, { recursive: true, force: true });
  }
}

async function runAcceptance() {
  const workDir = await mkdtemp(path.join(os.tmpdir(), "lenso-m5-acceptance-"));
  const clusterName = `lenso-m5-${process.pid}`;
  const kubeconfig = path.join(workDir, "kubeconfig");
  const operatorImage = `lenso-operator:m5-${process.pid}`;
  const registryName = `lenso-m5-registry-${process.pid}`;
  const registryPort = 5100 + (process.pid % 800);
  const candidateTag = `localhost:${registryPort}/lenso/support-api:candidate-${process.pid}`;
  const previousTag = `localhost:${registryPort}/lenso/support-api:previous-${process.pid}`;
  const systemPlaneTag = `localhost:${registryPort}/lenso/system-plane:m5-${process.pid}`;
  const runtimeConsoleTag = `localhost:${registryPort}/lenso/runtime-console:m5-${process.pid}`;
  let clusterCreated = false;
  let registryCreated = false;
  const generatedImageTags = [];
  let cleanup = { clusterDeleted: false, registryDeleted: false, generatedImagesDeleted: false, temporaryDirectoryDeleted: false };
  let evidence;
  let failure;

  try {
    await assertRequirements();
    const nginxImage = await immutableImage("nginx:1.27-alpine");
    const probeImage = await immutableImage("node:22-alpine");
    const busyboxImage = await immutableImage("busybox:1.36");
    const postgresImage = await immutableImage("postgres:17-alpine");
    const rustBuilderImage = await immutableImage("rust:bookworm");
    const debianRuntimeImage = await immutableImage("debian:bookworm-slim");
    const baseImageMaterials = [
      ociMaterial(rustBuilderImage),
      ociMaterial(debianRuntimeImage),
    ];
    const buildMaterials = await captureSupportBuildMaterials(baseImageMaterials);
    await run(dockerBin, ["run", "--detach", "--name", registryName, "--publish", `127.0.0.1:${registryPort}:5000`, "registry:2"]);
    registryCreated = true;
    await run(dockerBin, [
      "build", "--build-context", `lenso=${lensoRoot}`,
      "--build-arg", "RELEASE_VERSION=5.0.0",
      "--build-arg", `RUST_BUILDER_IMAGE=${rustBuilderImage}`,
      "--build-arg", `RUNTIME_IMAGE=${debianRuntimeImage}`,
      "--file", "infrastructure/m5-support.Dockerfile",
      "--tag", candidateTag, ".",
    ]);
    generatedImageTags.push(candidateTag);
    await run(dockerBin, [
      "build", "--build-context", `lenso=${lensoRoot}`,
      "--build-arg", "RELEASE_VERSION=4.9.0",
      "--build-arg", `RUST_BUILDER_IMAGE=${rustBuilderImage}`,
      "--build-arg", `RUNTIME_IMAGE=${debianRuntimeImage}`,
      "--file", "infrastructure/m5-support.Dockerfile",
      "--tag", previousTag, ".",
    ]);
    generatedImageTags.push(previousTag);
    assert.deepEqual(
      await captureSupportBuildMaterials(baseImageMaterials),
      buildMaterials,
      "support build inputs changed while candidate and previous images were built",
    );
    for (const [source, label] of [[candidateTag, "candidate"], [previousTag, "previous"]]) {
      for (const workload of ["support-api", "support-worker", "support-migration"]) {
        const target = `localhost:${registryPort}/lenso/${workload}:${label}-${process.pid}`;
        await run(dockerBin, ["tag", source, target]);
        generatedImageTags.push(target);
        await run(dockerBin, ["push", target]);
      }
    }
    await run(dockerBin, [
      "build", "--build-context", `lenso=${lensoRoot}`,
      "--file", "infrastructure/m5-system-plane.Dockerfile",
      "--tag", systemPlaneTag, ".",
    ]);
    generatedImageTags.push(systemPlaneTag);
    await run(dockerBin, ["push", systemPlaneTag]);
    await run(dockerBin, [
      "build", "--build-context", `runtime-console=${runtimeConsoleRoot}`,
      "--file", "infrastructure/m5-runtime-console.Dockerfile",
      "--tag", runtimeConsoleTag, ".",
    ]);
    generatedImageTags.push(runtimeConsoleTag);
    await run(dockerBin, ["push", runtimeConsoleTag]);
    const candidateImage = await immutableImage(candidateTag, false);
    const previousImage = await immutableImage(previousTag, false);
    const candidateDigest = candidateImage.split("@")[1];
    const previousDigest = previousImage.split("@")[1];
    const supplyChainEvidence = await generateSupplyChainEvidence(
      workDir,
      { image: candidateImage, digest: candidateDigest, version: "5.0.0" },
      { image: previousImage, digest: previousDigest, version: "4.9.0" },
      buildMaterials,
    );
    const systemPlaneImage = await immutableImage(systemPlaneTag, false);
    const runtimeConsoleImage = await immutableImage(runtimeConsoleTag, false);
    let core = await runCoreEvidence({ candidateDigest, previousDigest, supplyChainEvidence });
    validateCoreEvidence(core, "planning");
    const artifacts = await writeDeliveryArtifacts(workDir, core);
    const cliPolicy = await validateCliArtifacts(workDir, artifacts, core);
    const exports = await exportOperatorResources(workDir, artifacts);

    const kindConfig = path.join(workDir, "kind-config.yaml");
    await writeFile(kindConfig, `kind: Cluster\napiVersion: kind.x-k8s.io/v1alpha4\ncontainerdConfigPatches:\n- |-\n  [plugins.\"io.containerd.grpc.v1.cri\".registry.mirrors.\"localhost:${registryPort}\"]\n    endpoint = [\"http://${registryName}:5000\"]\n  [plugins.\"io.containerd.grpc.v1.cri\".registry.mirrors.\"registry.example.test\"]\n    endpoint = [\"http://${registryName}:5000\"]\n`);
    await run(kindBin, ["create", "cluster", "--name", clusterName, "--kubeconfig", kubeconfig, "--config", kindConfig]);
    clusterCreated = true;
    await run(dockerBin, ["network", "connect", "kind", registryName]);
    const kubeEnv = { KUBECONFIG: kubeconfig };
    await run(kubectlBin, ["label", "node", "--all", "topology.kubernetes.io/zone=acceptance", "--overwrite"], false, kubeEnv);
    await run(dockerBin, ["build", "--file", "infrastructure/operator/Dockerfile", "--tag", operatorImage, "."], false, {}, lensoRoot);
    generatedImageTags.push(operatorImage);
    await run(kindBin, ["load", "docker-image", "--name", clusterName, operatorImage, "nginx:1.27-alpine", "node:22-alpine", "busybox:1.36", "postgres:17-alpine"]);
    await installOperator(workDir, kubeEnv, operatorImage);
    await prepareEnvironment(kubeEnv, "lenso-m5-staging", 32, core.configRevision.revisionId, core.serviceRelease.releaseId, postgresImage, systemPlaneImage, runtimeConsoleImage);
    await prepareEnvironment(kubeEnv, "lenso-m5-production", 16, core.previousDeploymentPlan.configRevisionId, core.previousDeploymentPlan.releaseId, postgresImage, systemPlaneImage, runtimeConsoleImage);
    const systemPlanePolicy = await evaluateSystemPlanePolicy(
      kubeEnv,
      probeImage,
      core.policy.evaluationInput,
    );
    assert.deepEqual(systemPlanePolicy, core.policy.local, "actual System Plane must emit byte-equivalent Policy Evidence");

    const productionBaseline = await applyAndWait({
      kubeEnv,
      namespace: "lenso-m5-production",
      resource: exports.previousProduction,
      migrationFirst: false,
    });
    const productionBaselineObservation = await bridgeOperatorObservation(
      workDir,
      kubeEnv,
      "production-baseline",
      productionBaseline,
    );
    const productionBaselineObservationPath = path.join(
      workDir,
      "production-baseline.operator-observation.json",
    );
    const previousProductionCanary = JSON.parse(
      await readFile(exports.previousProduction, "utf8"),
    );
    previousProductionCanary.metadata.name = "service-support-production-canary";
    const previousProductionCanaryPath = path.join(
      workDir,
      "previous-production-canary.autonomous-service.json",
    );
    await writeJson(previousProductionCanaryPath, previousProductionCanary);
    const productionCanaryBaseline = await applyPreviouslyMigratedBaseline({
      kubeEnv,
      namespace: "lenso-m5-production",
      resource: previousProductionCanaryPath,
    });
    await bridgeOperatorObservation(
      workDir,
      kubeEnv,
      "production-canary-baseline",
      productionCanaryBaseline,
    );
    const productionCanaryBaselineObservationPath = path.join(
      workDir,
      "production-canary-baseline.operator-observation.json",
    );
    const productionBaselineGatewayObservation = await installGateway(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      core.previousGatewayPlan,
      nginxImage,
      productionBaselineObservation.observationId,
    );
    const productionBaselineGatewayObservationPath = path.join(
      workDir,
      "production-baseline.gateway-observation.json",
    );
    await writeJson(productionBaselineGatewayObservationPath, productionBaselineGatewayObservation);
    const staging = await applyAndWait({
      kubeEnv,
      namespace: "lenso-m5-staging",
      resource: exports.staging,
      migrationFirst: true,
    });
    const stagingObservation = await bridgeOperatorObservation(workDir, kubeEnv, "staging", staging);
    const stagingGatewayObservation = await installGateway(
      workDir,
      kubeEnv,
      "lenso-m5-staging",
      core.stagingGatewayPlan,
      nginxImage,
      stagingObservation.observationId,
    );
    const stagingObservationPath = path.join(workDir, "staging.operator-observation.json");
    const stagingGatewayObservationPath = path.join(workDir, "staging.gateway-observation.json");
    await writeJson(stagingGatewayObservationPath, stagingGatewayObservation);
    core = await runCoreEvidence({
      candidateDigest,
      previousDigest,
      supplyChainEvidence,
      stagingOperatorObservation: stagingObservationPath,
      stagingGatewayObservation: stagingGatewayObservationPath,
      baselineOperatorObservation: productionBaselineObservationPath,
      baselineGatewayObservation: productionBaselineGatewayObservationPath,
    });
    validateCoreEvidence(core, "planning");
    assert.ok(core.environmentVerification.evidenceReferences.includes(stagingObservation.observationId));
    const promotionCurrentnessContext = `promotion-currentness:${core.promotion.planDigest}:${core.promotionApproval.approvalId}`;
    const readCurrentStagingEvidence = async (untrustedExpectedConfiguration = null) => {
      const liveStaging = JSON.parse(await run(kubectlBin, [
        "get", "--namespace", "lenso-m5-staging",
        `lensoautonomousservice/${staging.metadata.name}`, "--output=json",
      ], true, kubeEnv));
      const currentOperator = await bridgeOperatorObservation(
        workDir,
        kubeEnv,
        "staging-promotion-current",
        liveStaging,
        promotionCurrentnessContext,
      );
      const currentGateway = await observeInstalledGateway(
        kubeEnv,
        "lenso-m5-staging",
        core.stagingGatewayPlan,
        currentOperator.observationId,
        promotionCurrentnessContext,
        untrustedExpectedConfiguration,
      );
      const gatewayPath = path.join(workDir, "staging-promotion-current.gateway-observation.json");
      await writeJson(gatewayPath, currentGateway);
      return {
        operator: path.join(workDir, "staging-promotion-current.operator-observation.json"),
        gateway: gatewayPath,
      };
    };
    let currentStagingEvidence = await readCurrentStagingEvidence();
    await proveOldObservationCannotBeRebound({
      workDir,
      kubeEnv,
      core,
      oldObservationPath: stagingObservationPath,
      currentGatewayPath: currentStagingEvidence.gateway,
      targetObservation: productionCanaryBaselineObservationPath,
      operatorExport: exports.candidateProductionExport,
      authorityContext: promotionCurrentnessContext,
    });
    await provePostVerificationGatewayDriftIsNonMutating({
      workDir,
      kubeEnv,
      core,
      targetObservation: productionCanaryBaselineObservationPath,
      operatorExport: exports.candidateProductionExport,
      readCurrentStagingEvidence,
    });
    currentStagingEvidence = await readCurrentStagingEvidence();
    let authorizedProduction = await authorizeProductionResource(
      workDir,
      core,
      currentStagingEvidence.operator,
      currentStagingEvidence.gateway,
      productionCanaryBaselineObservationPath,
      exports.candidateProductionExport,
    );
    assert.equal(staging.status.observedReleaseId, core.serviceRelease.releaseId);
    assert.equal(core.environmentVerification.decision, "passed");
    assert.equal(core.promotionReceipt.approvalId.startsWith("promotion-approval:sha256:"), true);

    const refreshedTargetObservation = await proveStalePromotionIsNonMutating(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      authorizedProduction,
      productionCanaryBaseline,
    );
    currentStagingEvidence = await readCurrentStagingEvidence();
    authorizedProduction = await authorizeProductionResource(
      workDir,
      core,
      currentStagingEvidence.operator,
      currentStagingEvidence.gateway,
      refreshedTargetObservation,
      exports.candidateProductionExport,
    );

    await applyEnvironmentConfig(kubeEnv, "lenso-m5-production", 32, core.configRevision.revisionId, core.serviceRelease.releaseId);
    const promoted = await applyAndWait({
      kubeEnv,
      namespace: "lenso-m5-production",
      resource: authorizedProduction,
      migrationFirst: false,
      mutation: "replace",
    });
    const promotedObservation = await bridgeOperatorObservation(workDir, kubeEnv, "promoted", promoted);
    await assertRunningBuildVersion(
      kubeEnv,
      probeImage,
      "service-support-production-canary-support-api",
      "5.0.0",
    );
    const promotedObservationPath = path.join(workDir, "promoted.operator-observation.json");
    core = await runCoreEvidence({
      candidateDigest,
      previousDigest,
      supplyChainEvidence,
      stagingOperatorObservation: stagingObservationPath,
      stagingGatewayObservation: stagingGatewayObservationPath,
      baselineOperatorObservation: productionBaselineObservationPath,
      baselineGatewayObservation: productionBaselineGatewayObservationPath,
      promotedOperatorObservation: promotedObservationPath,
    });
    const canaryActuatorReceipts = [];
    canaryActuatorReceipts.push(await installCanaryGateway(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      core.productionGatewayPlan,
      nginxImage,
      { plan: core.canaryPlan, decision: null, expectedPercent: 0, nextPercent: 10 },
    ));
    await setCanaryPublicLatency(kubeEnv, postgresImage, core.serviceRelease.releaseId, 0);
    const healthyObservation = await collectCanaryReliability(
      kubeEnv,
      probeImage,
      promotedObservation,
      core,
      { resourceName: "service-support-production-canary", exposurePercent: 10, revisionOffset: 1 },
    );
    const expansion = await runCanaryStep(workDir, core, [healthyObservation]);
    const expansionDecision = expansion.state.decisions.at(-1);
    assert.equal(expansionDecision?.outcome, "expand");
    assert.equal(expansionDecision?.nextPercent, 20);
    const expansionActuator = {
      plan: core.canaryPlan,
      decision: expansionDecision,
      expectedPercent: 10,
      nextPercent: 20,
    };
    const expansionReceipt = await installCanaryGateway(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      core.productionGatewayPlan,
      nginxImage,
      expansionActuator,
    );
    assert.deepEqual(
      await installCanaryGateway(
        workDir,
        kubeEnv,
        "lenso-m5-production",
        core.productionGatewayPlan,
        nginxImage,
        expansionActuator,
      ),
      expansionReceipt,
      "Canary actuator replay must recover the original idempotent receipt",
    );
    canaryActuatorReceipts.push(expansionReceipt);
    await setCanaryPublicLatency(kubeEnv, postgresImage, core.serviceRelease.releaseId, 700);
    const failedObservation = await collectCanaryReliability(
      kubeEnv,
      probeImage,
      promotedObservation,
      core,
      { resourceName: "service-support-production-canary", exposurePercent: 20, revisionOffset: 2 },
    );
    const completedCanary = await runCanaryStep(workDir, core, [healthyObservation, failedObservation]);
    const rollbackDecision = completedCanary.state.decisions.at(-1);
    assert.equal(rollbackDecision?.outcome, "rollback");
    assert.equal(rollbackDecision?.nextPercent, 0);
    canaryActuatorReceipts.push(await installCanaryGateway(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      core.productionGatewayPlan,
      nginxImage,
      { plan: core.canaryPlan, decision: rollbackDecision, expectedPercent: 20, nextPercent: 0 },
    ));
    await run(kubectlBin, [
      "delete", "lensoautonomousservice/service-support-production-canary",
      "--namespace", "lenso-m5-production", "--wait=true",
    ], false, kubeEnv);
    const canaryReliabilityObservationPath = path.join(workDir, "canary.reliability-observations.json");
    await writeJson(canaryReliabilityObservationPath, [healthyObservation, failedObservation]);
    core = await runCoreEvidence({
      candidateDigest,
      previousDigest,
      supplyChainEvidence,
      stagingOperatorObservation: stagingObservationPath,
      stagingGatewayObservation: stagingGatewayObservationPath,
      baselineOperatorObservation: productionBaselineObservationPath,
      baselineGatewayObservation: productionBaselineGatewayObservationPath,
      promotedOperatorObservation: promotedObservationPath,
      canaryReliabilityObservations: canaryReliabilityObservationPath,
    });
    validateCoreEvidence(core, "planning");
    assert.equal(promoted.status.observedReleaseId, core.serviceRelease.releaseId);
    assert.equal(core.canaryHistory.at(-1)?.decision, "blocked");
    assert.equal(core.canaryHistory.at(-1)?.outcome, "rollback");
    const authorizedRollback = await authorizeRollbackResource(
      workDir,
      core,
      promoted,
      exports.rollbackProductionExport,
      exports.candidateProductionExport,
    );

    await applyEnvironmentConfig(kubeEnv, "lenso-m5-production", 16, core.previousDeploymentPlan.configRevisionId, core.previousDeploymentPlan.releaseId);
    const rolledBack = await applyAndWait({
      kubeEnv,
      namespace: "lenso-m5-production",
      resource: authorizedRollback,
      migrationFirst: false,
    });
    const rolledBackObservation = await bridgeOperatorObservation(workDir, kubeEnv, "rolled-back", rolledBack);
    await assertRunningBuildVersion(
      kubeEnv,
      probeImage,
      "service-support-production-support-api",
      "4.9.0",
    );
    const rollbackGatewayObservation = await installGateway(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      core.previousGatewayPlan,
      nginxImage,
      rolledBackObservation.observationId,
    );
    const rollbackObservationPath = path.join(workDir, "rolled-back.operator-observation.json");
    const rollbackGatewayObservationPath = path.join(workDir, "rollback.gateway-observation.json");
    await writeJson(rollbackGatewayObservationPath, rollbackGatewayObservation);
    core = await runCoreEvidence({
      candidateDigest,
      previousDigest,
      supplyChainEvidence,
      stagingOperatorObservation: stagingObservationPath,
      stagingGatewayObservation: stagingGatewayObservationPath,
      baselineOperatorObservation: productionBaselineObservationPath,
      baselineGatewayObservation: productionBaselineGatewayObservationPath,
      promotedOperatorObservation: promotedObservationPath,
      canaryReliabilityObservations: canaryReliabilityObservationPath,
      rollbackOperatorObservation: rollbackObservationPath,
      rollbackGatewayObservation: rollbackGatewayObservationPath,
    });
    validateCoreEvidence(core, "planning");
    assert.ok(core.rollback, "post-rollback convergence must issue a receipt");
    assert.equal(rolledBack.status.observedReleaseId, core.rollback.restoredReleaseId);
    assert.equal(rolledBack.status.configRevisionId, core.rollback.restoredConfigRevisionId);
    assert.equal(
      rollbackGatewayObservation.configurationIdentity,
      core.previousGatewayPlan.configurationIdentity,
    );
    assert.notEqual(
      rollbackGatewayObservation.configurationIdentity,
      core.productionGatewayPlan.configurationIdentity,
    );

    await recordDeliveryArtifacts(kubeEnv, probeImage, core);
    await assertActualControlSurfaces(kubeEnv, probeImage, core);

    await assertRuntimeConsoleIsNotAuthority(kubeEnv, probeImage);
    await prepareDataPlaneOutage(kubeEnv, probeImage);

    await run(kubectlBin, ["scale", "deployment/lenso-operator", "--namespace", "lenso-system", "--replicas=0"], false, kubeEnv);
    await run(kubectlBin, ["rollout", "status", "deployment/lenso-operator", "--namespace", "lenso-system", "--timeout=90s"], false, kubeEnv);
    for (const deployment of ["lenso-system-plane", "lenso-runtime-console"]) {
      await run(kubectlBin, ["scale", `deployment/${deployment}`, "--namespace", "lenso-m5-production", "--replicas=0"], false, kubeEnv);
      await run(kubectlBin, ["rollout", "status", `deployment/${deployment}`, "--namespace", "lenso-m5-production", "--timeout=90s"], false, kubeEnv);
    }
    const dataPlaneResponse = await run(
      kubectlBin,
      [
        "run", `m5-data-plane-probe-${process.pid}`,
        "--namespace", "lenso-m5-production",
        `--image=${busyboxImage}`,
        "--restart=Never", "--rm", "--attach", "--command", "--",
        "wget", "-qO-",
        "--header=Authorization: Bearer m5-ephemeral",
        "--header=Origin: https://production.support.example.test",
        "http://lenso-m5-gateway/v1/tickets/acceptance",
      ],
      true,
      kubeEnv,
    );
    assert.match(dataPlaneResponse, /"ticketId":"acceptance"/i);
    await run(
      kubectlBin,
      [
        "run", `m5-private-edge-probe-${process.pid}`,
        "--namespace", "lenso-m5-production",
        `--image=${busyboxImage}`,
        "--restart=Never", "--rm", "--attach", "--command", "--",
        "sh", "-c",
        "if wget -qO- http://lenso-m5-gateway/admin/repair; then exit 1; else exit 0; fi",
      ],
      false,
      kubeEnv,
    );
    await assertGatewaySecurity(kubeEnv, probeImage);
    const outageObservation = await observeDataPlaneOutage(
      workDir,
      kubeEnv,
      probeImage,
      core,
      rolledBackObservation,
    );
    core = await runCoreEvidence({
      candidateDigest,
      previousDigest,
      supplyChainEvidence,
      stagingOperatorObservation: stagingObservationPath,
      stagingGatewayObservation: stagingGatewayObservationPath,
      baselineOperatorObservation: productionBaselineObservationPath,
      baselineGatewayObservation: productionBaselineGatewayObservationPath,
      promotedOperatorObservation: promotedObservationPath,
      canaryReliabilityObservations: canaryReliabilityObservationPath,
      rollbackOperatorObservation: rollbackObservationPath,
      rollbackGatewayObservation: rollbackGatewayObservationPath,
      outageObservation: path.join(workDir, "outage-observation.json"),
    });
    validateCoreEvidence(core, "passed");
    assert.equal(core.outage.decision, "passed");
    assert.equal(core.outage.releaseId, rolledBack.status.observedReleaseId);
    assert.equal(core.outage.configRevisionId, rolledBack.status.configRevisionId);
    assert.equal(outageObservation.claims.releaseId, rolledBack.status.observedReleaseId);
    assert.equal(outageObservation.claims.configRevisionId, rolledBack.status.configRevisionId);
    assert.equal(core.promotion.planId, JSON.parse(await readFile(path.join(workDir, "promotion-plan.json"), "utf8")).planId);
    const coordinationResume = await proveCoordinationResume(
      workDir,
      kubeEnv,
      "lenso-m5-production",
      authorizedRollback,
      core,
      probeImage,
    );

    evidence = {
      artifactVersion: "lenso.m5-production-delivery-acceptance.v1",
      outcome: "passed",
      publicSeam: "support-system",
      priorGuarantees: "m4_acceptance",
      providerCompatibility: "independent_host_managed_smoke",
      serviceReleaseId: core.serviceRelease.releaseId,
      workloadDigests: Object.fromEntries(
        core.serviceRelease.workloads.map((workload) => [workload.workloadId, workload.artifactDigest]),
      ),
      configuration: {
        revision: core.configRevision,
        stagingActivation: core.configStage,
        redactionProven: core.redactionProven,
      },
      deployments: {
        stagingPlan: core.stagingDeploymentPlan,
        productionPlan: core.productionDeploymentPlan,
        previousKnownGoodPlan: core.previousDeploymentPlan,
        adapterObservations: {
          productionBaseline: productionBaselineObservation,
          staging: stagingObservation,
          promoted: promotedObservation,
          rolledBack: rolledBackObservation,
        },
      },
      promotion: {
        plan: core.promotion,
        receipt: core.promotionReceipt,
      },
      canary: {
        plan: core.canaryPlan,
        observations: core.canaryObservations,
        decisions: core.canaryHistory,
        actuatorReceipts: canaryActuatorReceipts,
      },
      rollback: { plan: core.rollbackPlan, receipt: core.rollback },
      outage: core.outage,
      kubernetes: {
        actualApiUsed: true,
        runningOperatorUsed: true,
        runtimeConsoleUsedForDeployment: false,
        migrationFirstObserved: true,
        productionBaselineReleaseId: productionBaseline.status.observedReleaseId,
        stagingObservedReleaseId: staging.status.observedReleaseId,
        promotedReleaseId: promoted.status.observedReleaseId,
        rolledBackReleaseId: rolledBack.status.observedReleaseId,
        dataPlaneContinuedWithoutOperator: true,
        selectedGatewayResponseMatched: true,
        internalGatewayOperationPrivate: true,
        edgeAuthenticationEnforced: true,
        edgeCorsEnforced: true,
        edgeRateIntentEnforced: true,
        outageObservation,
        coordinationResume,
      },
      supplyChain: {
        trust: core.trust,
        actualEvidence: supplyChainEvidence,
        untrustedIssueCodes: core.untrustedIssueCodes,
        revokedIssueCodes: core.revokedIssueCodes,
      },
      policy: { ...core.policy, systemPlane: systemPlanePolicy, cli: cliPolicy, byteEquivalent: true },
      configRedactionProven: core.redactionProven,
      publicEdgePaths: core.publicEdgePaths,
      internalOperationsPrivate: core.internalOperationsPrivate,
      localRequirements: core.localRequirements,
      effects: {
        productionMutated: false,
        ephemeralKindClusterMutated: true,
        runtimeConsoleUsedForDeployment: false,
      },
    };
  } catch (error) {
    failure = error;
  } finally {
    if (clusterCreated) {
      try {
        await run(kindBin, ["delete", "cluster", "--name", clusterName]);
        cleanup.clusterDeleted = true;
      } catch (error) {
        failure ??= error;
      }
    } else {
      cleanup.clusterDeleted = true;
    }
    if (registryCreated) {
      try {
        await run(dockerBin, ["rm", "--force", registryName]);
        cleanup.registryDeleted = true;
      } catch (error) {
        failure ??= error;
      }
    } else {
      cleanup.registryDeleted = true;
    }
    let imageCleanupFailed = false;
    for (const tag of [...new Set(generatedImageTags)].reverse()) {
      try {
        await run(dockerBin, ["image", "rm", "--force", tag]);
      } catch (error) {
        imageCleanupFailed = true;
        failure ??= error;
      }
    }
    cleanup.generatedImagesDeleted = !imageCleanupFailed;
    try {
      await rm(workDir, { recursive: true, force: true });
      cleanup.temporaryDirectoryDeleted = true;
    } catch (error) {
      failure ??= error;
    }
  }

  if (failure) {
    failure.cleanup = cleanup;
    throw failure;
  }
  assert.equal(cleanup.clusterDeleted, true);
  assert.equal(cleanup.registryDeleted, true);
  assert.equal(cleanup.generatedImagesDeleted, true);
  assert.equal(cleanup.temporaryDirectoryDeleted, true);
  return { ...evidence, cleanup };
}

async function assertRequirements() {
  await access(path.join(lensoRoot, "Cargo.toml"));
  await access(path.join(runtimeConsoleRoot, "package.json"));
  await Promise.all([
    run(kindBin, ["version"], true),
    run(kubectlBin, ["version", "--client", "--output=json"], true),
    run(dockerBin, ["info", "--format", "{{.ServerVersion}}"], true, {}, repoRoot, 15_000),
    run("cargo", ["--version"], true),
    run(lensoBin, ["--version"], true),
  ]);
}

async function immutableImage(tag, pull = true) {
  if (pull) await run(dockerBin, ["pull", tag]);
  const output = await run(dockerBin, ["image", "inspect", tag, "--format", "{{json .RepoDigests}}"], true);
  const references = JSON.parse(output.trim());
  const reference = references.find((value) => value.includes("@sha256:"));
  assert.ok(reference, `Docker did not resolve ${tag} to an immutable RepoDigest`);
  return reference;
}

function ociMaterial(reference) {
  const [name, digest] = reference.split("@");
  assert.match(digest ?? "", /^sha256:[0-9a-f]{64}$/);
  return { uri: `oci://${name}@${digest}`, digest };
}

async function captureSupportBuildMaterials(baseImages) {
  const [examplesSource, lensoSource] = await Promise.all([
    captureGitMaterial(
      repoRoot,
      "git+https://github.com/LioRael/lenso-examples",
      ["examples/support-system/"],
    ),
    captureGitMaterial(lensoRoot, "git+https://github.com/LioRael/lenso"),
  ]);
  const dockerfile = await readFile(path.join(repoRoot, "infrastructure", "m5-support.Dockerfile"));
  const dockerignore = await readFile(path.join(repoRoot, ".dockerignore"));
  return [
    examplesSource,
    lensoSource,
    {
      uri: "file:infrastructure/m5-support.Dockerfile",
      digest: `sha256:${createHash("sha256").update(dockerfile).digest("hex")}`,
    },
    {
      uri: "file:.dockerignore",
      digest: `sha256:${createHash("sha256").update(dockerignore).digest("hex")}`,
    },
    ...baseImages,
  ];
}

async function captureGitMaterial(root, repository, includedPrefixes = null) {
  const [revisionOutput, filesOutput] = await Promise.all([
    run("git", ["rev-parse", "HEAD"], true, {}, root),
    run("git", ["ls-files", "--cached", "--others", "--exclude-standard"], true, {}, root),
  ]);
  const revision = revisionOutput.trim();
  const files = filesOutput
    .split("\n")
    .filter(Boolean)
    .filter((file) => !includedPrefixes || includedPrefixes.some((prefix) => file.startsWith(prefix)))
    .sort();
  assert.ok(files.length > 0, `no source files were captured from ${root}`);
  const hash = createHash("sha256");
  for (const file of files) {
    const bytes = await readFile(path.join(root, file));
    hash.update(file);
    hash.update("\0");
    hash.update(String(bytes.length));
    hash.update("\0");
    hash.update(bytes);
    hash.update("\0");
  }
  return {
    uri: `${repository}@${revision}`,
    digest: `sha256:${hash.digest("hex")}`,
    annotations: { revision, files: files.length },
  };
}

async function generateSupplyChainEvidence(workDir, candidate, previous, materials) {
  assert.ok(materials.length >= 6, "provenance must capture both source trees, Dockerfile, Docker context rules, and base images");
  const generate = async (name, artifact) => {
    const artifactSha256 = artifact.digest.replace("sha256:", "");
    const packageSpdxId = `SPDXRef-Package-${name}-${artifactSha256}`;
    const sbom = {
      spdxVersion: "SPDX-2.3",
      dataLicense: "CC0-1.0",
      SPDXID: "SPDXRef-DOCUMENT",
      name: `support-system-${name}-${artifact.version}`,
      documentNamespace: `https://lenso.dev/spdx/m5/support-system/${name}/${artifactSha256}`,
      creationInfo: {
        created: "2026-07-19T00:00:00Z",
        creators: ["Tool: lenso-m5-acceptance-1.0"],
      },
      documentDescribes: [packageSpdxId],
      packages: [{
        name: "support-system-m5-data-plane",
        SPDXID: packageSpdxId,
        versionInfo: artifact.version,
        downloadLocation: "NOASSERTION",
        filesAnalyzed: false,
        licenseConcluded: "NOASSERTION",
        licenseDeclared: "NOASSERTION",
        copyrightText: "NOASSERTION",
        checksums: [{ algorithm: "SHA256", checksumValue: artifactSha256 }],
      }],
    };
    const provenance = {
      _type: "https://in-toto.io/Statement/v1",
      predicateType: "https://slsa.dev/provenance/v1",
      subject: [{ name: artifact.image.split("@")[0], digest: { sha256: artifactSha256 } }],
      predicate: {
        buildDefinition: {
          buildType: "https://lenso.dev/buildtypes/m5-docker/v1",
          externalParameters: {
            releaseVersion: artifact.version,
            outputImage: artifact.image.split("@")[0],
          },
          internalParameters: { executor: "docker-buildkit" },
          resolvedDependencies: materials.map((material) => ({
            uri: material.uri,
            digest: { sha256: material.digest.replace("sha256:", "") },
            ...(material.annotations ? { annotations: material.annotations } : {}),
          })),
        },
        runDetails: {
          builder: {
            id: "https://lenso.dev/builders/m5-acceptance/v1",
            version: { implementation: "1" },
            builderDependencies: [],
          },
          metadata: {
            invocationId: `urn:lenso:m5-build:${name}:${artifactSha256}`,
          },
          byproducts: [],
        },
      },
    };
    assertSpdx23Sbom(sbom, artifactSha256);
    assertSlsaProvenanceV1(provenance, artifactSha256, materials);
    const sbomPath = path.join(workDir, `${name}.spdx.json`);
    const provenancePath = path.join(workDir, `${name}.provenance.json`);
    const sbomBytes = JSON.stringify(sbom, null, 2);
    const provenanceBytes = JSON.stringify(provenance, null, 2);
    await Promise.all([writeFile(sbomPath, sbomBytes), writeFile(provenancePath, provenanceBytes)]);
    const sbomReference = `data:application/spdx+json;base64,${Buffer.from(sbomBytes).toString("base64")}`;
    const provenanceReference = `data:application/vnd.in-toto+json;base64,${Buffer.from(provenanceBytes).toString("base64")}`;
    const evidence = {
      sbomReference,
      sbomDigest: `sha256:${createHash("sha256").update(sbomBytes).digest("hex")}`,
      provenanceReference,
      provenanceDigest: `sha256:${createHash("sha256").update(provenanceBytes).digest("hex")}`,
      source: "https://github.com/LioRael/lenso-examples",
      builder: provenance.predicate.runDetails.builder.id,
      inputDigests: materials.map((material) => material.digest),
      subjectDigest: artifact.digest,
    };
    assert.equal(`sha256:${provenance.subject[0].digest.sha256}`, artifact.digest);
    assert.equal(sbom.packages[0].versionInfo, artifact.version);
    assert.equal(Buffer.from(sbomReference.split(",", 2)[1], "base64").toString(), sbomBytes);
    assert.equal(Buffer.from(provenanceReference.split(",", 2)[1], "base64").toString(), provenanceBytes);
    return evidence;
  };
  return {
    candidate: await generate("candidate", candidate),
    previous: await generate("previous", previous),
  };
}

function assertSpdx23Sbom(sbom, artifactSha256) {
  assert.equal(sbom.spdxVersion, "SPDX-2.3");
  assert.equal(sbom.dataLicense, "CC0-1.0");
  assert.equal(sbom.SPDXID, "SPDXRef-DOCUMENT");
  assert.match(sbom.documentNamespace, /^https:\/\/[^#]+$/);
  assert.match(sbom.creationInfo.created, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
  assert.ok(sbom.creationInfo.creators.some((creator) => creator.startsWith("Tool: ")));
  assert.equal(sbom.packages.length, 1);
  const packageArtifact = sbom.packages[0];
  assert.match(packageArtifact.SPDXID, /^SPDXRef-[A-Za-z0-9.-]+$/);
  assert.deepEqual(sbom.documentDescribes, [packageArtifact.SPDXID]);
  assert.equal(packageArtifact.filesAnalyzed, false);
  assert.equal(packageArtifact.licenseConcluded, "NOASSERTION");
  assert.equal(packageArtifact.licenseDeclared, "NOASSERTION");
  assert.equal(packageArtifact.copyrightText, "NOASSERTION");
  assert.deepEqual(packageArtifact.checksums, [{ algorithm: "SHA256", checksumValue: artifactSha256 }]);
}

function assertSlsaProvenanceV1(provenance, artifactSha256, materials) {
  assert.equal(provenance._type, "https://in-toto.io/Statement/v1");
  assert.equal(provenance.predicateType, "https://slsa.dev/provenance/v1");
  assert.deepEqual(provenance.subject[0].digest, { sha256: artifactSha256 });
  const { buildDefinition, runDetails } = provenance.predicate;
  assert.match(buildDefinition.buildType, /^https:\/\//);
  assert.equal(typeof buildDefinition.externalParameters, "object");
  assert.equal(typeof buildDefinition.internalParameters, "object");
  assert.equal(buildDefinition.resolvedDependencies.length, materials.length);
  for (const [index, dependency] of buildDefinition.resolvedDependencies.entries()) {
    assert.match(dependency.uri, /^[a-z][a-z0-9+.-]*:/i);
    assert.deepEqual(dependency.digest, {
      sha256: materials[index].digest.replace("sha256:", ""),
    });
  }
  assert.match(runDetails.builder.id, /^https:\/\//);
  assert.equal(typeof runDetails.builder.version, "object");
  assert.ok(Array.isArray(runDetails.builder.builderDependencies));
  assert.match(runDetails.metadata.invocationId, /^urn:/);
  assert.ok(Array.isArray(runDetails.byproducts));
}

async function runCoreEvidence({
  candidateDigest,
  previousDigest,
  supplyChainEvidence,
  stagingOperatorObservation,
  stagingGatewayObservation,
  baselineOperatorObservation,
  baselineGatewayObservation,
  promotedOperatorObservation,
  canaryReliabilityObservations,
  rollbackOperatorObservation,
  rollbackGatewayObservation,
  outageObservation,
}) {
  const output = await run(
    "cargo",
    ["run", "--quiet", "--locked", "--manifest-path", fixtureManifest, "--bin", "support-system-m5-smoke"],
    true,
    {
      M5_CANDIDATE_IMAGE_DIGEST: candidateDigest,
      M5_PREVIOUS_IMAGE_DIGEST: previousDigest,
      M5_OPERATOR_OBSERVATION_PRIVATE_KEY: operatorObserverKeys.privateKeyBase64,
      M5_GATEWAY_OBSERVATION_PRIVATE_KEY: gatewayObserverKeys.privateKeyBase64,
      ...(supplyChainEvidence ? { M5_SUPPLY_CHAIN_EVIDENCE: JSON.stringify(supplyChainEvidence) } : {}),
      ...(stagingOperatorObservation ? { M5_STAGING_OPERATOR_OBSERVATION: stagingOperatorObservation } : {}),
      ...(stagingGatewayObservation ? { M5_STAGING_GATEWAY_OBSERVATION: stagingGatewayObservation } : {}),
      ...(baselineOperatorObservation ? { M5_BASELINE_OPERATOR_OBSERVATION: baselineOperatorObservation } : {}),
      ...(baselineGatewayObservation ? { M5_BASELINE_GATEWAY_OBSERVATION: baselineGatewayObservation } : {}),
      ...(promotedOperatorObservation ? { M5_PROMOTED_OPERATOR_OBSERVATION: promotedOperatorObservation } : {}),
      ...(canaryReliabilityObservations ? { M5_CANARY_RELIABILITY_OBSERVATIONS: canaryReliabilityObservations } : {}),
      ...(rollbackOperatorObservation ? { M5_ROLLBACK_OPERATOR_OBSERVATION: rollbackOperatorObservation } : {}),
      ...(rollbackGatewayObservation ? { M5_ROLLBACK_GATEWAY_OBSERVATION: rollbackGatewayObservation } : {}),
      ...(outageObservation ? { M5_OUTAGE_OBSERVATION: outageObservation } : {}),
    },
  );
  const value = output.match(/^M5_SMOKE_EVIDENCE=(.+)$/m)?.[1];
  assert.ok(value, "support-system did not emit M5_SMOKE_EVIDENCE");
  return JSON.parse(value);
}

function validateCoreEvidence(core, expectedOutcome) {
  assert.equal(core.artifactVersion, "lenso.m5-production-delivery-core.v1");
  assert.equal(core.outcome, expectedOutcome);
  assert.equal(core.publicSeam, "support-system");
  assert.deepEqual(core.tamperedIssueCodes, ["release_tampered"]);
  assert.ok(core.untrustedIssueCodes.includes("signer_untrusted"));
  assert.ok(core.revokedIssueCodes.includes("signer_revoked"));
  assert.equal(core.policy.byteEquivalent, true);
  assert.ok(core.policy.blockedIssueCodes.length > 0);
  assert.equal(core.redactionProven, true);
  assert.equal(core.migrationFirstRequired, true);
  assert.equal(core.internalOperationsPrivate, true);
  assert.equal(core.priorGuarantees, "m4_acceptance");
  assert.ok(core.serviceRelease.workloads.every((workload) =>
    workload.artifactReference.startsWith("registry.example.test/lenso/")
    && !workload.artifactReference.includes("localhost")));
  const candidateDigests = new Set(core.serviceRelease.workloads.map((workload) => workload.artifactDigest));
  const previousDigests = new Set(core.previousDeploymentPlan.workloads.map((workload) => workload.artifactDigest));
  assert.ok([...candidateDigests].every((value) => !previousDigests.has(value)),
    "candidate and previous known-good image digests must differ");
  assert.notEqual(core.productionGatewayPlan.configurationIdentity, core.previousGatewayPlan.configurationIdentity);
  for (const plan of [
    core.stagingGatewayPlan,
    core.productionGatewayPlan,
    core.previousGatewayPlan,
  ]) {
    assertGatewayPlanIntegrity(plan);
  }
}

async function writeDeliveryArtifacts(workDir, core) {
  const artifacts = {
    release: path.join(workDir, "service-release.json"),
    policy: path.join(workDir, "policy-evidence.json"),
    policyInput: path.join(workDir, "policy-evaluation-input.json"),
    staging: path.join(workDir, "staging.deployment-plan.json"),
    candidateProduction: path.join(workDir, "candidate-production.deployment-plan.json"),
    previousProduction: path.join(workDir, "previous-production.deployment-plan.json"),
  };
  await Promise.all([
    writeJson(artifacts.release, core.serviceRelease),
    writeJson(artifacts.policy, core.policy.local),
    writeJson(artifacts.policyInput, core.policy.evaluationInput),
    writeJson(artifacts.staging, core.stagingDeploymentPlan),
    writeJson(artifacts.candidateProduction, core.productionDeploymentPlan),
    writeJson(artifacts.previousProduction, core.previousDeploymentPlan),
  ]);
  return artifacts;
}

async function validateCliArtifacts(workDir, artifacts, core) {
  await run(lensoBin, ["service", "delivery", "check", artifacts.release]);
  const cliPolicyPath = path.join(workDir, "cli-policy-evidence.json");
  const release = JSON.parse(await readFile(artifacts.release, "utf8"));
  const trustedSignatures = Object.fromEntries(
    release.signatures.map((signature) => [signature.signer, "ephemeral-m5-key"]),
  );
  const eligibilityAttestations = {
    [core.policy.evaluationInput.eligibilityInput.providerId]: "ephemeral-m5-key",
  };
  const secretObservations = Object.fromEntries(
    core.configRevision.secretReferences.map((reference) => [reference.referenceId, {
      provider: reference.provider,
      status: reference.status,
      metadata: reference.metadata,
    }]),
  );
  try {
    await run(lensoBin, [
      "service", "delivery", "policy", artifacts.policyInput,
      "--output", cliPolicyPath,
    ], false, {
      LENSO_TRUSTED_RELEASE_SIGNATURES: JSON.stringify(trustedSignatures),
      LENSO_TRUSTED_ELIGIBILITY_ATTESTATIONS: JSON.stringify(eligibilityAttestations),
      LENSO_TRUSTED_SECRET_OBSERVATIONS: JSON.stringify(secretObservations),
    });
  } catch (error) {
    const blocked = JSON.parse(await readFile(cliPolicyPath, "utf8"));
    throw new Error(`CLI policy diverged: ${JSON.stringify(blocked.issues)}`, { cause: error });
  }
  const cliPolicy = JSON.parse(await readFile(cliPolicyPath, "utf8"));
  assert.deepEqual(cliPolicy, core.policy.local, "CLI must run the canonical Policy Pack byte-equivalently");
  for (const deploymentPlan of [artifacts.staging, artifacts.candidateProduction, artifacts.previousProduction]) {
    await run(lensoBin, ["service", "delivery", "deployment-plan", deploymentPlan]);
  }
  return cliPolicy;
}

async function exportOperatorResources(workDir, artifacts) {
  const stagingExport = path.join(workDir, "staging.operator-export.json");
  const previousExport = path.join(workDir, "previous-production.operator-export.json");
  const candidateExport = path.join(workDir, "candidate-production.operator-export.json");
  const rollbackExport = path.join(workDir, "rollback-production.operator-export.json");
  await run(lensoBin, ["service", "delivery", "operator-export", artifacts.staging, "--output", stagingExport]);
  await run(lensoBin, ["service", "delivery", "operator-export", artifacts.previousProduction, "--output", previousExport]);
  await run(lensoBin, ["service", "delivery", "operator-export", artifacts.candidateProduction, "--previous", previousExport, "--output", candidateExport]);
  await run(lensoBin, ["service", "delivery", "operator-export", artifacts.previousProduction, "--previous", candidateExport, "--output", rollbackExport]);
  return {
    staging: await writeResource(workDir, "staging", stagingExport),
    previousProduction: await writeResource(workDir, "previous-production", previousExport),
    candidateProduction: await writeResource(workDir, "candidate-production", candidateExport),
    rollbackProduction: await writeResource(workDir, "rollback-production", rollbackExport),
    candidateProductionExport: candidateExport,
    rollbackProductionExport: rollbackExport,
  };
}

async function authorizeProductionResource(
  workDir,
  core,
  sourceObservation,
  sourceGatewayObservation,
  targetObservation,
  operatorExport,
) {
  const promotionPlan = path.join(workDir, "promotion-plan.json");
  const approval = path.join(workDir, "promotion-approval.json");
  const protectedEvidence = path.join(workDir, "promotion-protected-evidence.json");
  const verification = path.join(workDir, "environment-verification.json");
  const authorization = path.join(workDir, "promotion-apply-authorization.json");
  await Promise.all([
    writeJson(promotionPlan, core.promotion),
    writeJson(approval, core.promotionApproval),
    writeJson(protectedEvidence, core.promotionProtectedEvidence),
    writeJson(verification, core.environmentVerification),
  ]);
  await run(lensoBin, [
    "service", "delivery", "promotion-apply",
    promotionPlan,
    approval,
    protectedEvidence,
    verification,
    sourceObservation,
    sourceGatewayObservation,
    targetObservation,
    operatorExport,
    "--output", authorization,
  ], true, {
    LENSO_PROMOTION_AUTHORITY_KEY: "ephemeral-m5-approval-key",
    LENSO_TRUSTED_EDGE_ATTESTATIONS: JSON.stringify({
      [core.promotion.targetGateway.edgeProviderId]: "ephemeral-m5-key",
    }),
    LENSO_TRUSTED_OPERATOR_OBSERVATION_AUTHORITIES: JSON.stringify({
      [operatorObservationAuthorityId]: operatorObserverKeys.publicKeyBase64,
    }),
    LENSO_TRUSTED_GATEWAY_OBSERVATION_AUTHORITIES: JSON.stringify({
      [gatewayObservationAuthorityId]: gatewayObserverKeys.publicKeyBase64,
    }),
  });
  const authorized = JSON.parse(await readFile(authorization, "utf8"));
  assert.equal(authorized.protocol, "lenso.promotion-apply-authorization.v1");
  assert.equal(authorized.planId, core.promotion.planId);
  assert.equal(authorized.targetObservationId, JSON.parse(await readFile(targetObservation, "utf8")).observationId);
  assert.equal(authorized.resource.metadata.uid, authorized.targetResourceUid);
  assert.equal(authorized.resource.metadata.resourceVersion, authorized.targetResourceVersion);
  const authorizationContent = { ...authorized };
  delete authorizationContent.authorizationId;
  delete authorizationContent.authorizationDigest;
  assert.equal(authorized.authorizationDigest, digestJson(authorizationContent));
  assert.equal(
    authorized.authorizationId,
    `promotion-apply-authorization:${authorized.authorizationDigest}`,
  );
  const resource = path.join(workDir, "authorized-production.autonomous-service.json");
  await writeJson(resource, authorized.resource);
  return resource;
}

async function proveOldObservationCannotBeRebound({
  workDir,
  kubeEnv,
  core,
  oldObservationPath,
  currentGatewayPath,
  targetObservation,
  operatorExport,
  authorityContext,
}) {
  const before = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", "lenso-m5-production",
    "lensoautonomousservice/service-support-production", "--output=json",
  ], true, kubeEnv));
  const rebound = JSON.parse(await readFile(oldObservationPath, "utf8"));
  rebound.claims.authorityContext = authorityContext;
  rebound.authorityContext = authorityContext;
  rebound.observationDigest = digestJson(rebound.claims);
  rebound.observationId = `operator-observation:${rebound.observationDigest}`;
  const reboundPath = path.join(workDir, "staging.rebound-old.operator-observation.json");
  await writeJson(reboundPath, rebound);
  await assert.rejects(
    authorizeProductionResource(
      workDir,
      core,
      reboundPath,
      currentGatewayPath,
      targetObservation,
      operatorExport,
    ),
    /authority proof was modified|authority proof is invalid/u,
  );
  const after = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", "lenso-m5-production",
    "lensoautonomousservice/service-support-production", "--output=json",
  ], true, kubeEnv));
  assert.equal(after.metadata.uid, before.metadata.uid);
  assert.equal(after.metadata.resourceVersion, before.metadata.resourceVersion);
  assert.deepEqual(after.spec, before.spec, "old observation challenge rebinding must not mutate production");
}

async function provePostVerificationGatewayDriftIsNonMutating({
  workDir,
  kubeEnv,
  core,
  targetObservation,
  operatorExport,
  readCurrentStagingEvidence,
}) {
  const before = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", "lenso-m5-production",
    "lensoautonomousservice/service-support-production", "--output=json",
  ], true, kubeEnv));
  const assertRejectedWithoutMutation = async (message) => {
    const drifted = await readCurrentStagingEvidence(
      message === "Gateway data-only drift" ? "server { listen 80; return 503; }\n" : null,
    );
    const gateway = JSON.parse(await readFile(drifted.gateway, "utf8"));
    assert.equal(gateway.fresh, false, `${message} must be signed as non-fresh`);
    await assert.rejects(
      authorizeProductionResource(
        workDir,
        core,
        drifted.operator,
        drifted.gateway,
        targetObservation,
        operatorExport,
      ),
      /observation_stale: source Gateway observation/u,
    );
    const after = JSON.parse(await run(kubectlBin, [
      "get", "--namespace", "lenso-m5-production",
      "lensoautonomousservice/service-support-production", "--output=json",
    ], true, kubeEnv));
    assert.equal(after.metadata.uid, before.metadata.uid);
    assert.equal(after.metadata.resourceVersion, before.metadata.resourceVersion);
    assert.deepEqual(after.spec, before.spec, `${message} rejection must not mutate production`);
  };

  await run(kubectlBin, [
    "patch", "--namespace", "lenso-m5-staging", "configmap/lenso-m5-gateway",
    "--type=merge", "--patch", JSON.stringify({
      data: { "default.conf": "server { listen 80; return 503; }\n" },
    }),
  ], false, kubeEnv);
  try {
    await assertRejectedWithoutMutation("Gateway data-only drift");
  } finally {
    await run(kubectlBin, [
      "patch", "--namespace", "lenso-m5-staging", "configmap/lenso-m5-gateway",
      "--type=merge", "--patch", JSON.stringify({
        data: { "default.conf": renderGatewayConfiguration(core.stagingGatewayPlan) },
      }),
    ], false, kubeEnv);
  }

  await run(kubectlBin, [
    "annotate", "--namespace", "lenso-m5-staging", "configmap/lenso-m5-gateway",
    `lenso.dev/gateway-revision=${core.stagingGatewayPlan.expectedGatewayRevision}tampered`,
    "--overwrite",
  ], false, kubeEnv);
  try {
    await assertRejectedWithoutMutation("Gateway malformed revision");
  } finally {
    await run(kubectlBin, [
      "annotate", "--namespace", "lenso-m5-staging", "configmap/lenso-m5-gateway",
      `lenso.dev/gateway-revision=${core.stagingGatewayPlan.expectedGatewayRevision}`,
      "--overwrite",
    ], false, kubeEnv);
  }
}

async function authorizeRollbackResource(
  workDir,
  core,
  current,
  operatorExport,
  candidateOperatorExport,
) {
  const plan = core.rollbackPlan;
  const exported = JSON.parse(await readFile(operatorExport, "utf8"));
  const previousPlan = path.join(workDir, "rollback-target.deployment-plan.json");
  const verifiedExport = path.join(workDir, "rollback-target.verified-operator-export.json");
  await writeJson(previousPlan, core.previousDeploymentPlan);
  await run(lensoBin, [
    "service", "delivery", "operator-export", previousPlan,
    "--previous", candidateOperatorExport,
    "--output", verifiedExport,
  ]);
  assert.deepEqual(
    exported,
    JSON.parse(await readFile(verifiedExport, "utf8")),
    "rollback Operator resource must be the deterministic export of the exact rollback Deployment plan",
  );
  assert.equal(plan.protocol, "lenso.rollback-plan.v1");
  assert.equal(plan.automaticAllowed, true, "Acceptance may not bypass an intervention boundary");
  assert.equal(plan.canaryDecisionId, core.canaryHistory.at(-1)?.decisionId);
  assert.equal(plan.failedReleaseId, current.status.observedReleaseId);
  assert.equal(plan.failedReleaseDigest, current.status.observedReleaseDigest);
  assert.equal(plan.failedConfigRevisionId, current.status.configRevisionId);
  assert.equal(plan.failedGatewayPlanDigest, core.productionGatewayPlan.planDigest);
  assert.equal(plan.previousGatewayPlanDigest, core.previousGatewayPlan.planDigest);
  assert.notEqual(plan.previousGatewayConfigurationIdentity, core.productionGatewayPlan.configurationIdentity);
  assert.equal(exported.protocol, "lenso.operator-export.v1");
  assert.equal(exported.effects.mutatesEnvironment, false);
  assert.equal(exported.deploymentPlanDigest, core.previousDeploymentPlan.planDigest);
  assert.equal(exported.resource.spec.releaseId, plan.previousReleaseId);
  assert.equal(exported.resource.spec.releaseDigest, plan.previousReleaseDigest);
  assert.equal(exported.resource.spec.configRevisionId, plan.previousConfigRevisionId);
  assert.equal(exported.resource.spec.rollbackReleaseId, plan.failedReleaseId);
  const resource = path.join(workDir, "authorized-rollback.autonomous-service.json");
  await writeJson(resource, exported.resource);
  return resource;
}

async function writeResource(workDir, name, exportPath) {
  const exported = JSON.parse(await readFile(exportPath, "utf8"));
  assert.equal(exported.protocol, "lenso.operator-export.v1");
  assert.equal(exported.effects.mutatesEnvironment, false);
  const resourcePath = path.join(workDir, `${name}.autonomous-service.json`);
  await writeJson(resourcePath, exported.resource);
  return resourcePath;
}

async function runCanaryStep(workDir, core, observations) {
  const input = path.join(workDir, `canary-step-${observations.length}.json`);
  await writeJson(input, {
    plan: core.canaryPlan,
    deploymentObservation: core.productionDeploymentObservation,
    observations,
  });
  const output = await run(
    "cargo",
    [
      "run", "--quiet", "--locked", "--manifest-path", fixtureManifest,
      "--bin", "support-system-m5-canary-step", "--", input,
    ],
    true,
  );
  const value = output.match(/^M5_CANARY_STEP=(.+)$/m)?.[1];
  assert.ok(value, "canary actuator did not emit a canonical decision");
  return JSON.parse(value);
}

async function bridgeOperatorObservation(workDir, kubeEnv, name, resource, authorityContext = null) {
  const observationPath = path.join(workDir, `${name}.operator-observation.json`);
  const output = await run(process.execPath, [trustedObserverAdapter, JSON.stringify({
    kind: "operator",
    environment: resource.spec.environment,
    resourceName: resource.metadata.name,
    authorityContext,
  })], true, {
    ...kubeEnv,
    KUBECTL_BIN: kubectlBin,
    LENSO_OBSERVER_AUTHORITY_ID: operatorObservationAuthorityId,
    LENSO_OBSERVER_PRIVATE_KEY_PEM: operatorObserverKeys.privateKeyPem,
  });
  const observation = JSON.parse(output);
  await writeJson(observationPath, observation);
  assert.equal(observation.protocol, "lenso.operator-observation.v1");
  assert.equal(observation.authorityId, operatorObservationAuthorityId);
  assert.ok(observation.authorityProof, "Operator observation must carry adapter authority proof");
  assert.equal(observation.decision, "passed");
  return observation;
}

async function assertRunningBuildVersion(kubeEnv, image, serviceName, expectedVersion) {
  const script = `
const response = await fetch('http://${serviceName}:8080/health');
const body = await response.json();
if (!response.ok || body.buildReleaseVersion !== '${expectedVersion}') {
  throw new Error('running Workload bytes do not identify expected build ${expectedVersion}: ' + JSON.stringify(body));
}
console.log(JSON.stringify(body));
`;
  await run(kubectlBin, [
    "run", `m5-build-version-${expectedVersion.replaceAll(".", "-")}-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
}

async function collectCanaryReliability(
  kubeEnv,
  image,
  operatorObservation,
  core,
  { resourceName, exposurePercent, revisionOffset },
) {
  const script = `
const endpoint = 'http://lenso-m5-gateway/v1/tickets/m5-canary';
const samples = [];
const started = Date.now();
for (let index = 0; index < 40; index += 1) {
  const before = performance.now();
  const response = await fetch(endpoint + '-' + index, {
    headers: {
      authorization: 'Bearer m5-user-visible-probe',
      origin: '${core.productionGatewayPlan.routes[0].cors.allowedOrigins[0]}',
      'x-service-principal': 'service:m5-acceptance-probe',
      'x-tenant-id': 'tenant:m5',
    },
  });
  const body = await response.json();
  samples.push({ ok: response.status === 200, latencyMs: Math.ceil(performance.now() - before), body });
  await new Promise((resolve) => setTimeout(resolve, 650));
}
samples.sort((left, right) => left.latencyMs - right.latencyMs);
const successes = samples.filter((sample) => sample.ok).length;
const candidateSamples = samples.filter((sample) => sample.body.releaseId === '${core.serviceRelease.releaseId}').length;
const stableSamples = samples.filter((sample) => sample.body.releaseId === '${core.previousDeploymentPlan.releaseId}').length;
if (candidateSamples === 0 || stableSamples === 0) throw new Error('bounded gateway did not expose both release versions');
console.log(JSON.stringify({
  sampleCount: samples.length,
  observationWindowSeconds: Math.max(1, Math.ceil((Date.now() - started) / 1000)),
  availabilityBasisPoints: Math.floor((successes * 10000) / samples.length),
  latencyP99Ms: samples[Math.ceil(samples.length * 0.99) - 1].latencyMs,
  errorBudgetUsedBasisPoints: Math.floor(((samples.length - successes) * 10000) / samples.length),
  databaseAvailable: true,
  candidateSamples,
  stableSamples,
  queueBacklog: Math.max(...samples.map((sample) => sample.body.operationalMetrics.queueBacklog)),
  workflowBacklog: Math.max(...samples.map((sample) => sample.body.operationalMetrics.workflowBacklog)),
  timerLagMs: Math.max(...samples.map((sample) => sample.body.operationalMetrics.timerLagMs)),
  retryExhaustion: Math.max(...samples.map((sample) => sample.body.operationalMetrics.retryExhaustion)),
  compensationPressure: Math.max(...samples.map((sample) => sample.body.operationalMetrics.compensationPressure)),
}));
`;
  const probeOutput = await run(kubectlBin, [
    "run", `m5-canary-reliability-${exposurePercent}-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  const probe = JSON.parse(probeOutput.trim());
  const hpas = JSON.parse(await run(kubectlBin, [
    "get", "hpa", "--namespace", "lenso-m5-production",
    "--selector", `lenso.dev/autonomous-service=${resourceName}`,
    "--output=json",
  ], true, kubeEnv));
  const deployments = JSON.parse(await run(kubectlBin, [
    "get", "deployments", "--namespace", "lenso-m5-production",
    "--selector", `lenso.dev/autonomous-service=${resourceName}`,
    "--output=json",
  ], true, kubeEnv));
  const autonomousService = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", "lenso-m5-production",
    `lensoautonomousservice/${resourceName}`, "--output=json",
  ], true, kubeEnv));
  const pdbs = JSON.parse(await run(kubectlBin, [
    "get", "pdb", "--namespace", "lenso-m5-production",
    "--selector", `lenso.dev/autonomous-service=${resourceName}`,
    "--output=json",
  ], true, kubeEnv));
  const pods = JSON.parse(await run(kubectlBin, [
    "get", "pods", "--namespace", "lenso-m5-production",
    "--selector", `lenso.dev/autonomous-service=${resourceName}`,
    "--output=json",
  ], true, kubeEnv));
  const nodes = JSON.parse(await run(kubectlBin, ["get", "nodes", "--output=json"], true, kubeEnv));
  const nodeZones = new Map(nodes.items.map((node) => [
    node.metadata.name,
    node.metadata.labels?.["topology.kubernetes.io/zone"] ?? node.metadata.name,
  ]));
  const failureDomains = {};
  for (const pod of pods.items.filter((item) => item.status?.phase === "Running")) {
    const ready = pod.status?.conditions?.some((condition) => condition.type === "Ready" && condition.status === "True");
    const zone = nodeZones.get(pod.spec?.nodeName);
    if (zone) failureDomains[zone] = (failureDomains[zone] ?? true) && ready;
  }
  const workloadHealth = Object.fromEntries(
    operatorObservation.workloads.map((workload) => [workload.workloadId, workload.ready]),
  );
  const observation = {
    protocol: "",
    observationId: "",
    canaryPlanId: "",
    canaryPlanDigest: "",
    releaseId: "",
    releaseDigest: "",
    environment: "",
    deploymentPlanId: "",
    deploymentPlanDigest: "",
    deploymentObservationId: "",
    collectorId: "kubernetes-http-reliability-adapter",
    collectorProof: "",
    observedRevision: core.productionDeploymentPlan.expectedEnvironmentRevision + revisionOffset,
    freshnessHorizonRevision: core.productionDeploymentPlan.expectedEnvironmentRevision + 10,
    fresh: operatorObservation.fresh && operatorObservation.decision === "passed",
    observationWindowSeconds: probe.observationWindowSeconds,
    sampleCount: probe.sampleCount,
    genericProcessHealthy: Object.values(workloadHealth).every(Boolean),
    workloadReadiness: workloadHealth,
    workloadLiveness: workloadHealth,
    availabilityBasisPoints: probe.availabilityBasisPoints,
    latencyP99Ms: probe.latencyP99Ms,
    errorBudgetUsedBasisPoints: probe.errorBudgetUsedBasisPoints,
    queueBacklog: probe.queueBacklog,
    workflowBacklog: probe.workflowBacklog,
    timerLagMs: probe.timerLagMs,
    retryExhaustion: probe.retryExhaustion,
    compensationPressure: probe.compensationPressure,
    dependencies: [{ dependencyId: "database", available: probe.databaseAvailable }],
    failureDomains,
    scalingCheckPassed: actualScalingIsSatisfied(
      autonomousService.spec.workloads,
      deployments.items,
      hpas.items,
    ),
    disruptionCheckPassed: pdbs.items.length > 0 && pdbs.items.every((item) =>
      (item.status?.currentHealthy ?? 0) >= (item.status?.desiredHealthy ?? 0)),
    availabilityCheckPassed: Object.values(failureDomains).some(Boolean),
    evidenceReferences: [
      operatorObservation.observationId,
      operatorObservation.observationDigest,
      "collector:kubernetes-http-reliability-adapter",
      `canary-actuator:gateway-split:${exposurePercent}`,
      `user-visible:candidate-samples:${probe.candidateSamples}`,
      `user-visible:stable-samples:${probe.stableSamples}`,
    ],
  };
  return observation;
}

async function setCanaryPublicLatency(kubeEnv, image, releaseId, latencyMs) {
  await run(kubectlBin, [
    "run", `m5-fault-${latencyMs}-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--env=PGPASSWORD=m5-ephemeral-only",
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "psql", "--host=lenso-m5-postgres", "--username=postgres", "--dbname=postgres",
    "--command", `insert into m5_acceptance_faults (release_id, latency_ms) values ('${releaseId}', ${latencyMs}) on conflict (release_id) do update set latency_ms = excluded.latency_ms`,
  ], true, kubeEnv);
}

async function installCanaryGateway(workDir, kubeEnv, namespace, gatewayPlan, image, actuator) {
  const { plan, decision, expectedPercent, nextPercent } = actuator;
  assertGatewayPlanIntegrity(gatewayPlan);
  assert.equal(plan.protocol, "lenso.canary-plan.v1");
  const planDigest = digestJson([{
    protocol: plan.protocol,
    releaseId: plan.releaseId,
    releaseDigest: plan.releaseDigest,
    productionDeploymentPlanId: plan.productionDeploymentPlanId,
    productionDeploymentDigest: plan.productionDeploymentDigest,
    productionDeploymentReceiptId: plan.productionDeploymentReceiptId,
    productionDeploymentObservationId: plan.productionDeploymentObservationId,
    productionEnvironment: plan.productionEnvironment,
    productionExpectedEnvironmentRevision: plan.productionExpectedEnvironmentRevision,
    reliabilityContract: plan.reliabilityContract,
    releaseRollbackConstraints: plan.releaseRollbackConstraints,
    policyEvidenceId: plan.policyEvidenceId,
    policyEvidenceDigest: plan.policyEvidenceDigest,
    environmentVerificationId: plan.environmentVerificationId,
    environmentVerificationDigest: plan.environmentVerificationDigest,
    previousKnownGoodPlanId: plan.previousKnownGoodPlanId,
    previousKnownGoodDigest: plan.previousKnownGoodDeploymentDigest,
    previousKnownGoodReleaseId: plan.previousKnownGoodReleaseId,
    previousKnownGoodReleaseDigest: plan.previousKnownGoodReleaseDigest,
    previousKnownGoodReceiptId: plan.previousKnownGoodReceiptId,
    previousKnownGoodObservationId: plan.previousKnownGoodObservationId,
    previousKnownGoodPolicyEvidenceId: plan.previousKnownGoodPolicyEvidenceId,
    previousKnownGoodPolicyEvidenceDigest: plan.previousKnownGoodPolicyEvidenceDigest,
    previousKnownGoodGatewayPlanId: plan.previousKnownGoodGatewayPlanId,
    previousKnownGoodGatewayPlanDigest: plan.previousKnownGoodGatewayPlanDigest,
    previousKnownGoodGatewayConfigurationIdentity: plan.previousKnownGoodGatewayConfigurationIdentity,
    previousKnownGoodGatewayObservationId: plan.previousKnownGoodGatewayObservationId,
    previousKnownGoodGatewayObservationRevision: plan.previousKnownGoodGatewayObservationRevision,
    previousKnownGoodGatewayObservedAfter: plan.previousKnownGoodGatewayObservedAfter,
    initialPercent: plan.initialPercent,
    maximumPercent: plan.maximumPercent,
    workloadIds: plan.workloadIds,
  }, plan.effects]);
  assert.equal(plan.planDigest, planDigest, "actuator rejected a modified Canary plan");
  assert.equal(plan.planId, `canary-plan:${planDigest}`, "actuator rejected a forged Canary plan identity");
  assert.equal(gatewayPlan.edgeReleaseDigest, plan.releaseDigest, "Canary plan and Gateway release must match");
  assert.ok(nextPercent >= 0 && nextPercent <= plan.maximumPercent, "canary exposure exceeds plan maximum");
  if (decision) {
    assert.equal(decision.planId, plan.planId);
    assert.equal(decision.currentPercent, expectedPercent);
    assert.equal(decision.nextPercent, nextPercent);
    assert.equal(
      decision.decisionId,
      `canary-decision:${digestJson({
        protocol: decision.protocol,
        planId: decision.planId,
        observationId: decision.observationId,
        decision: decision.decision,
        outcome: decision.outcome,
        currentPercent: decision.currentPercent,
        nextPercent: decision.nextPercent,
        issues: decision.issues,
        activeDegradedModes: decision.activeDegradedModes,
        evidenceReferences: decision.evidenceReferences,
        effects: decision.effects,
      })}`,
      "actuator must consume an integrity-valid canonical Canary decision",
    );
  } else {
    assert.equal(expectedPercent, 0);
    assert.equal(nextPercent, plan.initialPercent);
  }
  let before;
  try {
    before = JSON.parse(await run(kubectlBin, [
      "get", "configmap/lenso-m5-gateway", "--namespace", namespace, "--output=json",
    ], true, kubeEnv));
  } catch {
    before = null;
  }
  const observedPercent = Number(before?.metadata?.annotations?.["lenso.dev/canary-percent"] ?? 0);
  const decisionId = decision?.decisionId ?? `canary-initial:${plan.planDigest}`;
  const replayingAppliedDecision = observedPercent === nextPercent
    && before?.metadata?.annotations?.["lenso.dev/canary-plan-digest"] === plan.planDigest
    && before?.metadata?.annotations?.["lenso.dev/canary-decision-id"] === decisionId;
  if (!replayingAppliedDecision) {
    assert.equal(observedPercent, expectedPercent, "canary traffic CAS rejected stale exposure state");
  }
  const previousConfigurationIdentity = replayingAppliedDecision
    ? before.metadata.annotations["lenso.dev/previous-gateway-configuration-identity"]
    : before?.metadata?.annotations?.["lenso.dev/gateway-configuration-identity"] ?? "absent";
  assert.ok(previousConfigurationIdentity, "idempotent actuator receipt lost its previous Gateway identity");
  const configurationIdentity = digestJson([
    "lenso.canary-gateway-configuration.v1",
    gatewayPlan.planDigest,
    plan.planDigest,
    previousConfigurationIdentity,
    decisionId,
    expectedPercent,
    nextPercent,
  ]);
  const stable = "service-support-production-support-api:8080";
  const candidate = "service-support-production-canary-support-api:8080";
  const allowedOrigin = gatewayPlan.routes[0].cors.allowedOrigins[0];
  const quotedAllowedOrigin = nginxQuoted(allowedOrigin);
  const locationPattern = nginxQuoted(nginxLocationPattern(gatewayPlan.routes[0].publicPath));
  const upstream = nextPercent === 0
    ? `upstream m5_backend { server ${stable}; }`
    : [
      `upstream m5_stable { server ${stable}; }`,
      `upstream m5_canary { server ${candidate}; }`,
      `split_clients "\${remote_addr}\${uri}" $m5_backend { ${nextPercent}% m5_canary; * m5_stable; }`,
    ].join("\n");
  const proxyTarget = nextPercent === 0 ? "m5_backend" : "$m5_backend";
  const configuration = [
    upstream,
    "limit_req_zone $binary_remote_addr zone=m5_edge:10m rate=100r/m;",
    "map $http_authorization $m5_auth_ok { default 1; \"\" 0; }",
    `map $http_origin $m5_cors_ok { default 0; "" 1; ${quotedAllowedOrigin} 1; }`,
    "server {",
    "  listen 80;",
    `  location ~ ${locationPattern} {`,
    "    if ($m5_auth_ok = 0) { return 401; }",
    "    if ($m5_cors_ok = 0) { return 403; }",
    "    limit_req zone=m5_edge burst=1 nodelay;",
    `    add_header Access-Control-Allow-Origin ${quotedAllowedOrigin} always;`,
    "    add_header Vary Origin always;",
    `    proxy_pass http://${proxyTarget};`,
    "  }",
    "  location / { return 404; }",
    "}",
  ].join("\n");
  const resources = {
    apiVersion: "v1",
    kind: "List",
    items: [
      {
        apiVersion: "v1",
        kind: "ConfigMap",
        metadata: {
          name: "lenso-m5-gateway",
          namespace,
          annotations: {
            "lenso.dev/gateway-configuration-identity": configurationIdentity,
            "lenso.dev/previous-gateway-configuration-identity": previousConfigurationIdentity,
            "lenso.dev/gateway-revision": String(gatewayPlan.expectedGatewayRevision),
            "lenso.dev/canary-percent": String(nextPercent),
            "lenso.dev/canary-plan-digest": plan.planDigest,
            "lenso.dev/canary-decision-id": decisionId,
          },
        },
        data: { "default.conf": configuration },
      },
      {
        apiVersion: "apps/v1",
        kind: "Deployment",
        metadata: { name: "lenso-m5-gateway", namespace },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-m5-gateway" } },
          template: {
            metadata: {
              labels: { app: "lenso-m5-gateway" },
              annotations: {
                "lenso.dev/gateway-plan-digest": gatewayPlan.planDigest,
                "lenso.dev/canary-plan-digest": plan.planDigest,
                "lenso.dev/canary-decision-id": decisionId,
                "lenso.dev/canary-percent": String(nextPercent),
              },
            },
            spec: {
              containers: [{
                name: "gateway",
                image,
                ports: [{ name: "http", containerPort: 80 }],
                volumeMounts: [{ name: "configuration", mountPath: "/etc/nginx/conf.d" }],
                readinessProbe: { tcpSocket: { port: 80 } },
              }],
              volumes: [{ name: "configuration", configMap: { name: "lenso-m5-gateway" } }],
            },
          },
        },
      },
    ],
  };
  const file = path.join(workDir, `production.canary-${nextPercent}.gateway.json`);
  await writeJson(file, resources);
  if (!replayingAppliedDecision) {
    await run(kubectlBin, ["apply", "--filename", file], false, kubeEnv);
    await run(kubectlBin, [
      "rollout", "restart", "deployment/lenso-m5-gateway", "--namespace", namespace,
    ], false, kubeEnv);
    await run(kubectlBin, [
      "rollout", "status", "deployment/lenso-m5-gateway", "--namespace", namespace, "--timeout=120s",
    ], false, kubeEnv);
  }
  const observed = JSON.parse(await run(kubectlBin, [
    "get", "configmap/lenso-m5-gateway", "--namespace", namespace, "--output=json",
  ], true, kubeEnv));
  assert.equal(observed.metadata.annotations["lenso.dev/canary-percent"], String(nextPercent));
  assert.equal(observed.metadata.annotations["lenso.dev/gateway-configuration-identity"], configurationIdentity);
  assert.equal(observed.data["default.conf"], configuration);
  return {
    protocol: "lenso.canary-actuator-receipt.v1",
    receiptId: `canary-actuator-receipt:${digestJson([plan.planDigest, decisionId, previousConfigurationIdentity, configurationIdentity, expectedPercent, nextPercent])}`,
    planId: plan.planId,
    planDigest: plan.planDigest,
    decisionId,
    expectedPercent,
    appliedPercent: nextPercent,
    previousConfigurationIdentity,
    configurationIdentity,
    effects: { mutatesGateway: previousConfigurationIdentity !== configurationIdentity },
  };
}

function assertGatewayPlanIntegrity(plan) {
  assert.equal(plan.protocol, "lenso.gateway-plan.v1");
  for (const route of plan.routes) {
    assert.ok(publicPathTemplateIsSafe(route.publicPath));
    assert.ok(route.rate.requests > 0 && route.rate.windowSeconds > 0);
    assert.equal(new Set(route.cors.allowedOrigins).size, route.cors.allowedOrigins.length);
    assert.equal(new Set(route.cors.allowedMethods).size, route.cors.allowedMethods.length);
    assert.equal(route.cors.allowedOrigins.length === 0, route.cors.allowedMethods.length === 0);
    for (const origin of route.cors.allowedOrigins) {
      const parsed = new URL(origin);
      assert.ok(["http:", "https:"].includes(parsed.protocol));
      assert.equal(parsed.username, "");
      assert.equal(parsed.password, "");
      assert.equal(parsed.search, "");
      assert.equal(parsed.hash, "");
      assert.equal(parsed.pathname, "/");
      nginxQuoted(origin);
    }
    for (const method of route.cors.allowedMethods) {
      assert.ok(["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"].includes(method));
    }
  }
  const binding = {
    environment: plan.environment,
    gatewayAdapter: plan.gatewayAdapter,
    publicOrigin: plan.publicOrigin,
    expectedGatewayRevision: plan.expectedGatewayRevision,
  };
  const configurationIdentity = digestJson([
    plan.edgeContractDigest,
    plan.edgeReleaseId,
    plan.edgeReleaseDigest,
    plan.operationCatalogDigest,
    plan.edgeProviderId,
    plan.edgeProviderProof,
    plan.environment,
    plan.gatewayAdapter,
    plan.publicOrigin,
    plan.routes,
  ]);
  assert.equal(
    plan.configurationIdentity,
    configurationIdentity,
    "gateway actuator rejected a modified resolved Edge configuration",
  );
  const edgeAuthoritySubject = digestJson([
    "lenso.edge-authority-subject.v1",
    plan.edgeReleaseId,
    plan.edgeReleaseDigest,
    plan.operationCatalogDigest,
    plan.routes,
  ]);
  const expectedProviderProof = `sha256:${createHash("sha256")
    .update(`ephemeral-m5-key\0${edgeAuthoritySubject}`)
    .digest("hex")}`;
  assert.equal(plan.edgeProviderId, "ci:m5-acceptance");
  assert.equal(plan.edgeProviderProof, expectedProviderProof, "gateway actuator rejected untrusted Edge authority");
  const planDigest = digestJson([
    plan.protocol,
    plan.edgeContractId,
    plan.edgeContractDigest,
    plan.edgeReleaseId,
    plan.edgeReleaseDigest,
    plan.operationCatalogDigest,
    plan.edgeProviderId,
    plan.edgeProviderProof,
    binding,
    plan.configurationIdentity,
    plan.routes,
    plan.diff,
    plan.drifted,
    plan.issues,
    plan.nextActions,
    plan.effects,
  ]);
  assert.equal(plan.planDigest, planDigest, "gateway actuator rejected a modified Gateway plan");
  assert.equal(plan.planId, `gateway-plan:${planDigest}`, "gateway actuator rejected a forged Gateway plan identity");
}

function renderGatewayConfiguration(plan) {
  const upstream = `service-support-${plan.environment}-support-api:8080`;
  const allowedOrigin = plan.routes[0].cors.allowedOrigins[0];
  const quotedAllowedOrigin = nginxQuoted(allowedOrigin);
  const locationPattern = nginxQuoted(nginxLocationPattern(plan.routes[0].publicPath));
  return [
    "limit_req_zone $binary_remote_addr zone=m5_edge:10m rate=100r/m;",
    "map $http_authorization $m5_auth_ok { default 1; \"\" 0; }",
    `map $http_origin $m5_cors_ok { default 0; \"\" 1; ${quotedAllowedOrigin} 1; }`,
    "server {",
    "  listen 80;",
    `  location ~ ${locationPattern} {`,
    "    if ($m5_auth_ok = 0) { return 401; }",
    "    if ($m5_cors_ok = 0) { return 403; }",
    "    limit_req zone=m5_edge burst=1 nodelay;",
    `    add_header Access-Control-Allow-Origin ${quotedAllowedOrigin} always;`,
    "    add_header Vary Origin always;",
    `    proxy_pass http://${upstream};`,
    "  }",
    "  location / { return 404; }",
    "}",
  ].join("\n");
}

async function installGateway(workDir, kubeEnv, namespace, plan, image, observedAfter) {
  assertGatewayPlanIntegrity(plan);
  assert.equal(plan.routes.length, 1, "acceptance Gateway must expose only explicit Edge routes");
  assert.equal(plan.routes[0].operationId, "getTicket");
  assert.equal(plan.routes[0].authentication, "workload_or_user");
  assert.deepEqual(plan.routes[0].cors.allowedMethods, ["GET"]);
  assert.equal(plan.routes[0].rate.requests, 100);
  assert.equal(plan.routes[0].rate.windowSeconds, 60);
  const configuration = renderGatewayConfiguration(plan);
  const resources = {
    apiVersion: "v1",
    kind: "List",
    items: [
      {
        apiVersion: "v1",
        kind: "ConfigMap",
        metadata: {
          name: "lenso-m5-gateway",
          namespace,
          annotations: {
            "lenso.dev/gateway-configuration-identity": plan.configurationIdentity,
            "lenso.dev/gateway-revision": String(plan.expectedGatewayRevision),
          },
        },
        data: { "default.conf": configuration },
      },
      {
        apiVersion: "apps/v1",
        kind: "Deployment",
        metadata: { name: "lenso-m5-gateway", namespace },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-m5-gateway" } },
          template: {
            metadata: {
              labels: { app: "lenso-m5-gateway" },
              annotations: { "lenso.dev/gateway-plan-digest": plan.planDigest },
            },
            spec: {
              containers: [{
                name: "gateway",
                image,
                ports: [{ name: "http", containerPort: 80 }],
                volumeMounts: [{ name: "configuration", mountPath: "/etc/nginx/conf.d" }],
                readinessProbe: { tcpSocket: { port: 80 } },
              }],
              volumes: [{ name: "configuration", configMap: { name: "lenso-m5-gateway" } }],
            },
          },
        },
      },
      {
        apiVersion: "v1",
        kind: "Service",
        metadata: { name: "lenso-m5-gateway", namespace },
        spec: {
          selector: { app: "lenso-m5-gateway" },
          ports: [{ name: "http", port: 80, targetPort: "http" }],
        },
      },
    ],
  };
  const file = path.join(workDir, `${plan.environment}.gateway.json`);
  await writeJson(file, resources);
  await run(kubectlBin, ["apply", "--filename", file], false, kubeEnv);
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-m5-gateway", "--namespace", namespace, "--timeout=120s"], false, kubeEnv);
  const observedConfig = JSON.parse(await run(kubectlBin, [
    "get", "configmap/lenso-m5-gateway", "--namespace", namespace, "--output=json",
  ], true, kubeEnv));
  assert.equal(observedConfig.metadata.annotations["lenso.dev/gateway-configuration-identity"], plan.configurationIdentity);
  assert.equal(observedConfig.data["default.conf"], configuration);
  return observeInstalledGateway(kubeEnv, namespace, plan, observedAfter, plan.planId);
}

async function observeInstalledGateway(
  kubeEnv,
  namespace,
  plan,
  observedAfter,
  authorityContext,
  untrustedExpectedConfiguration = null,
) {
  assert.equal(namespace, `lenso-m5-${plan.environment}`);
  const output = await run(process.execPath, [trustedObserverAdapter, JSON.stringify({
    kind: "gateway",
    plan,
    expectedConfiguration: untrustedExpectedConfiguration,
    observedAfter,
    authorityContext,
  })], true, {
    ...kubeEnv,
    KUBECTL_BIN: kubectlBin,
    LENSO_OBSERVER_AUTHORITY_ID: gatewayObservationAuthorityId,
    LENSO_OBSERVER_PRIVATE_KEY_PEM: gatewayObserverKeys.privateKeyPem,
  });
  return JSON.parse(output);
}

function publicPathTemplateIsSafe(publicPath) {
  if (typeof publicPath !== "string" || publicPath.length > 2048
    || publicPath === "/" || !publicPath.startsWith("/") || publicPath.endsWith("/")) return false;
  const parameters = new Set();
  return publicPath.slice(1).split("/").every((segment) => {
    if (!segment) return false;
    const parameter = segment.match(/^\{([A-Za-z][A-Za-z0-9_]{0,63})\}$/u)?.[1];
    if (parameter) {
      if (parameters.has(parameter)) return false;
      parameters.add(parameter);
      return true;
    }
    return segment.length <= 128 && /^[A-Za-z0-9._-]+$/u.test(segment);
  });
}

function nginxLocationPattern(publicPath) {
  assert.ok(publicPathTemplateIsSafe(publicPath), "Gateway path must use the validated template grammar");
  const segments = publicPath.slice(1).split("/").map((segment) => (
    /^\{[A-Za-z][A-Za-z0-9_]{0,63}\}$/u.test(segment)
      ? "[^/]+"
      : segment.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")
  ));
  return `^/${segments.join("/")}$`;
}

function nginxQuoted(value) {
  assert.equal(typeof value, "string");
  assert.ok(!/[\u0000\r\n]/u.test(value), "Nginx values may not contain control line breaks");
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function digestJson(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function observerKeyPair() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const publicDer = publicKey.export({ format: "der", type: "spki" });
  const privateDer = privateKey.export({ format: "der", type: "pkcs8" });
  return {
    privateKeyPem: privateKey.export({ format: "pem", type: "pkcs8" }),
    privateKeyBase64: privateDer.subarray(privateDer.length - 32).toString("base64"),
    publicKeyBase64: publicDer.subarray(publicDer.length - 32).toString("base64"),
  };
}

function canonicalJsonValue(value) {
  if (Array.isArray(value)) return value.map(canonicalJsonValue);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonicalJsonValue(value[key])]),
    );
  }
  return value;
}

async function installOperator(workDir, kubeEnv, operatorImage) {
  await run(kubectlBin, ["apply", "--filename", path.join(lensoRoot, "contracts", "operator", "lenso-autonomous-service.v1alpha1.crd.yaml")], false, kubeEnv);
  const resources = {
    apiVersion: "v1",
    kind: "List",
    items: [
      { apiVersion: "v1", kind: "Namespace", metadata: { name: "lenso-system" } },
      { apiVersion: "v1", kind: "ServiceAccount", metadata: { name: "lenso-operator", namespace: "lenso-system" } },
      {
        apiVersion: "rbac.authorization.k8s.io/v1",
        kind: "ClusterRole",
        metadata: { name: "lenso-m5-operator" },
        rules: [
          { apiGroups: ["lenso.dev"], resources: ["lensoautonomousservices", "lensoautonomousservices/status"], verbs: ["get", "list", "watch", "patch", "update"] },
          { apiGroups: ["apps"], resources: ["deployments"], verbs: ["get", "list", "watch", "create", "patch", "update", "delete"] },
          { apiGroups: ["batch"], resources: ["jobs"], verbs: ["get", "list", "watch", "create", "patch", "update"] },
          { apiGroups: [""], resources: ["services"], verbs: ["get", "list", "watch", "create", "patch", "update", "delete"] },
          { apiGroups: ["autoscaling"], resources: ["horizontalpodautoscalers"], verbs: ["get", "list", "watch", "create", "patch", "update", "delete"] },
          { apiGroups: ["policy"], resources: ["poddisruptionbudgets"], verbs: ["get", "list", "watch", "create", "patch", "update", "delete"] },
          { apiGroups: ["networking.k8s.io"], resources: ["networkpolicies"], verbs: ["get", "list", "watch", "create", "patch", "update", "delete"] },
        ],
      },
      {
        apiVersion: "rbac.authorization.k8s.io/v1",
        kind: "ClusterRoleBinding",
        metadata: { name: "lenso-m5-operator" },
        roleRef: { apiGroup: "rbac.authorization.k8s.io", kind: "ClusterRole", name: "lenso-m5-operator" },
        subjects: [{ kind: "ServiceAccount", name: "lenso-operator", namespace: "lenso-system" }],
      },
      {
        apiVersion: "apps/v1",
        kind: "Deployment",
        metadata: { name: "lenso-operator", namespace: "lenso-system" },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-operator" } },
          template: {
            metadata: { labels: { app: "lenso-operator" } },
            spec: {
              serviceAccountName: "lenso-operator",
              containers: [{
                name: "operator",
                image: operatorImage,
                imagePullPolicy: "Never",
                env: [
                  { name: "LENSO_OPERATOR_CONTROLLERS", value: "autonomous" },
                  { name: "RUST_LOG", value: "info" },
                ],
              }],
            },
          },
        },
      },
    ],
  };
  const resourcesPath = path.join(workDir, "operator-resources.json");
  await writeJson(resourcesPath, resources);
  await run(kubectlBin, ["apply", "--filename", resourcesPath], false, kubeEnv);
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-operator", "--namespace", "lenso-system", "--timeout=120s"], false, kubeEnv);
}

async function prepareEnvironment(kubeEnv, namespace, maxConcurrency, configRevisionId, releaseId, postgresImage, systemPlaneImage, runtimeConsoleImage) {
  await run(kubectlBin, ["create", "namespace", namespace], false, kubeEnv);
  for (const secret of ["secret-support-database-v4", "secret-support-database-v5"]) {
    await run(kubectlBin, [
      "create", "secret", "generic", secret, "--namespace", namespace,
      "--from-literal=DB_PASSWORD=m5-ephemeral-only",
      "--from-literal=DATABASE_URL=postgres://postgres:m5-ephemeral-only@lenso-m5-postgres:5432/postgres",
    ], false, kubeEnv);
  }
  await applyEnvironmentConfig(kubeEnv, namespace, maxConcurrency, configRevisionId, releaseId);
  const coordination = {
    apiVersion: "v1",
    kind: "List",
    items: [
      {
        apiVersion: "apps/v1", kind: "Deployment",
        metadata: { name: "lenso-m5-postgres", namespace },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-m5-postgres" } },
          template: {
            metadata: { labels: { app: "lenso-m5-postgres" } },
            spec: { containers: [{
              name: "postgres", image: postgresImage,
              env: [{ name: "POSTGRES_PASSWORD", value: "m5-ephemeral-only" }],
              ports: [{ name: "postgres", containerPort: 5432 }],
              readinessProbe: { exec: { command: ["pg_isready", "-U", "postgres"] } },
            }] },
          },
        },
      },
      {
        apiVersion: "v1", kind: "Service",
        metadata: { name: "lenso-m5-postgres", namespace },
        spec: { selector: { app: "lenso-m5-postgres" }, ports: [{ name: "postgres", port: 5432, targetPort: "postgres" }] },
      },
      {
        apiVersion: "apps/v1", kind: "Deployment",
        metadata: { name: "lenso-system-plane", namespace },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-system-plane" } },
          template: {
            metadata: { labels: { app: "lenso-system-plane" } },
            spec: {
              initContainers: [{
                name: "migrate",
                image: systemPlaneImage,
                command: ["/bin/sh", "-c", "until /usr/local/bin/lenso-migrate; do sleep 2; done"],
                env: [{ name: "DATABASE_URL", value: "postgres://postgres:m5-ephemeral-only@lenso-m5-postgres:5432/postgres" }],
              }],
              containers: [{
                name: "system-plane", image: systemPlaneImage,
                env: [
                  { name: "DATABASE_URL", value: "postgres://postgres:m5-ephemeral-only@lenso-m5-postgres:5432/postgres" },
                  { name: "HTTP_HOST", value: "0.0.0.0" },
                  { name: "HTTP_PORT", value: "8080" },
                  { name: "LENSO_ALLOW_DEV_AUTH_ON_PUBLIC_BIND", value: "true" },
                  { name: "LENSO_DELIVERY_TRUST_KEYS", value: JSON.stringify({ "ci:m5-acceptance": "ephemeral-m5-key" }) },
                  { name: "LENSO_DELIVERY_SECRET_PROVIDER", value: "acceptance-local" },
                  { name: "LENSO_DELIVERY_SECRET_OBSERVATIONS", value: JSON.stringify({
                    "secret:support:database:v4": { status: "resolved", metadata: { rotationRevision: "6" } },
                    "secret:support:database:v5": { status: "resolved", metadata: { rotationRevision: "7" } },
                  }) },
                ],
                ports: [{ name: "http", containerPort: 8080 }],
                readinessProbe: { httpGet: { path: "/openapi.json", port: "http" } },
              }],
            },
          },
        },
      },
      {
        apiVersion: "v1", kind: "Service",
        metadata: { name: "lenso-system-plane", namespace },
        spec: { selector: { app: "lenso-system-plane" }, ports: [{ name: "http", port: 8080, targetPort: "http" }] },
      },
      {
        apiVersion: "apps/v1", kind: "Deployment",
        metadata: { name: "lenso-runtime-console", namespace },
        spec: {
          replicas: 1,
          selector: { matchLabels: { app: "lenso-runtime-console" } },
          template: {
            metadata: { labels: { app: "lenso-runtime-console" } },
            spec: { containers: [{
              name: "runtime-console", image: runtimeConsoleImage,
              ports: [{ name: "http", containerPort: 8080 }],
              readinessProbe: { httpGet: { path: "/health", port: "http" } },
            }] },
          },
        },
      },
      {
        apiVersion: "v1", kind: "Service",
        metadata: { name: "lenso-runtime-console", namespace },
        spec: { selector: { app: "lenso-runtime-console" }, ports: [{ name: "http", port: 8080, targetPort: "http" }] },
      },
    ],
  };
  const resources = path.join(os.tmpdir(), `lenso-m5-environment-${process.pid}-${namespace}.json`);
  await writeJson(resources, coordination);
  await run(kubectlBin, ["apply", "--filename", resources], false, kubeEnv);
  await rm(resources, { force: true });
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-m5-postgres", "--namespace", namespace, "--timeout=120s"], false, kubeEnv);
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-system-plane", "--namespace", namespace, "--timeout=120s"], false, kubeEnv);
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-runtime-console", "--namespace", namespace, "--timeout=120s"], false, kubeEnv);
}

async function applyEnvironmentConfig(kubeEnv, namespace, maxConcurrency, configRevisionId, releaseId) {
  const configMapName = `service-support-config-${configRevisionId.replace("config-revision:sha256:", "").replaceAll(/[^a-zA-Z0-9]/g, "-").toLowerCase().slice(0, 12)}`;
  const manifest = await run(
    kubectlBin,
    [
      "create", "configmap", configMapName,
      "--namespace", namespace,
      `--from-literal=MAX_CONCURRENCY=${maxConcurrency}`,
      `--from-literal=CONFIG_REVISION_ID=${configRevisionId}`,
      `--from-literal=RELEASE_ID=${releaseId}`,
      "--from-literal=SECRET_ROTATION_POLICY=preserve",
      "--from-literal=SYSTEM_PLANE_ENDPOINT=http://lenso-system-plane:8080",
      "--from-literal=SYSTEM_PLANE_HEALTH_PATH=/openapi.json",
      "--dry-run=client", "--output=json",
    ],
    true,
    kubeEnv,
  );
  const child = spawn(kubectlBin, ["apply", "--filename", "-"], {
    cwd: repoRoot,
    env: { ...process.env, ...kubeEnv },
    stdio: ["pipe", "inherit", "inherit"],
  });
  child.stdin.end(manifest);
  await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${kubectlBin} apply ConfigMap exited with ${code ?? signal}`));
    });
  });
}

async function proveStalePromotionIsNonMutating(
  workDir,
  kubeEnv,
  namespace,
  authorizedResource,
  expectedBaseline,
) {
  const authorized = JSON.parse(await readFile(authorizedResource, "utf8"));
  const name = authorized.metadata.name;
  const before = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", namespace, `lensoautonomousservice/${name}`, "--output=json",
  ], true, kubeEnv));
  assert.equal(before.metadata.uid, authorized.metadata.uid);
  let concurrent = before;
  if (before.metadata.resourceVersion === authorized.metadata.resourceVersion) {
    await run(kubectlBin, [
      "annotate", "--namespace", namespace, `lensoautonomousservice/${name}`,
      `lenso.dev/m5-cas-probe=${process.pid}`, "--overwrite",
    ], false, kubeEnv);
    concurrent = JSON.parse(await run(kubectlBin, [
      "get", "--namespace", namespace, `lensoautonomousservice/${name}`, "--output=json",
    ], true, kubeEnv));
  }
  assert.notEqual(concurrent.metadata.resourceVersion, authorized.metadata.resourceVersion);
  assert.deepEqual(concurrent.spec, expectedBaseline.spec, "CAS probe must not alter production intent");
  let rejected = false;
  try {
    await run(kubectlBin, [
      "replace", "--namespace", namespace, "--filename", authorizedResource,
    ], false, kubeEnv);
  } catch {
    rejected = true;
  }
  assert.equal(rejected, true, "stale Promotion replace must fail atomically");
  const after = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", namespace, `lensoautonomousservice/${name}`, "--output=json",
  ], true, kubeEnv));
  assert.equal(after.metadata.uid, concurrent.metadata.uid);
  assert.equal(after.metadata.resourceVersion, concurrent.metadata.resourceVersion);
  assert.deepEqual(after.spec, concurrent.spec, "stale Promotion must have zero production effects");
  await bridgeOperatorObservation(workDir, kubeEnv, "production-target-refreshed", after);
  return path.join(workDir, "production-target-refreshed.operator-observation.json");
}

async function applyAndWait({ kubeEnv, namespace, resource, migrationFirst, mutation = "apply" }) {
  const desired = JSON.parse(await readFile(resource, "utf8"));
  const name = desired.metadata.name;
  assert.ok(["apply", "replace"].includes(mutation));
  if (mutation === "replace") {
    assert.ok(desired.metadata.uid, "Promotion replace requires the observed target UID");
    assert.ok(desired.metadata.resourceVersion, "Promotion replace requires the observed target resourceVersion");
  }
  await run(kubectlBin, [mutation, "--namespace", namespace, "--filename", resource], false, kubeEnv);
  const job = await waitForMigrationJob(kubeEnv, namespace, name, desired.spec.releaseDigest);
  if (migrationFirst) {
    const deployments = JSON.parse(await run(kubectlBin, ["get", "deployments", "--namespace", namespace, "--selector", `lenso.dev/autonomous-service=${name}`, "--output=json"], true, kubeEnv));
    const previousReleaseId = desired.spec.rollbackReleaseId;
    if (previousReleaseId) {
      assert.ok(deployments.items.length > 0, "Previous production Workloads must remain available during candidate Migration");
      assert.ok(deployments.items.every((deployment) =>
        deployment.spec.template.metadata.annotations["lenso.dev/release-id"] === previousReleaseId
      ), "Candidate API or Worker appeared before Migration completion");
    } else {
      assert.equal(deployments.items.length, 0, "API and Worker Deployments appeared before initial Migration completion");
    }
  }
  await run(kubectlBin, ["wait", "--namespace", namespace, "--for=condition=complete", `job/${job.metadata.name}`, "--timeout=120s"], false, kubeEnv);
  await run(kubectlBin, ["wait", "--namespace", namespace, "--for=jsonpath={.status.state}=ready", `lensoautonomousservice/${name}`, "--timeout=180s"], false, kubeEnv);
  const observed = JSON.parse(await run(kubectlBin, ["get", "--namespace", namespace, `lensoautonomousservice/${name}`, "--output=json"], true, kubeEnv));
  assert.equal(observed.status.observedReleaseDigest, desired.spec.releaseDigest);
  assert.equal(observed.status.drifted, false);
  assert.ok(desired.spec.policyEvidenceReferences.length > 0, "Operator resource must bind Policy Evidence");
  assert.deepEqual(
    observed.status.policyEvidenceReferences,
    desired.spec.policyEvidenceReferences,
    "Operator status must report the exact Policy Evidence references",
  );
  return observed;
}

async function applyPreviouslyMigratedBaseline({ kubeEnv, namespace, resource }) {
  const desired = JSON.parse(await readFile(resource, "utf8"));
  const name = desired.metadata.name;
  await run(kubectlBin, ["apply", "--namespace", namespace, "--filename", resource], false, kubeEnv);
  await run(kubectlBin, [
    "wait", "--namespace", namespace, "--for=jsonpath={.status.state}=ready",
    `lensoautonomousservice/${name}`, "--timeout=180s",
  ], false, kubeEnv);
  const observed = JSON.parse(await run(kubectlBin, [
    "get", "--namespace", namespace,
    `lensoautonomousservice/${name}`, "--output=json",
  ], true, kubeEnv));
  assert.equal(observed.status.observedReleaseDigest, desired.spec.releaseDigest);
  assert.equal(observed.status.drifted, false);
  assert.deepEqual(
    observed.status.policyEvidenceReferences,
    desired.spec.policyEvidenceReferences,
    "pre-migrated canary baseline must preserve the exact Policy Evidence references",
  );
  return observed;
}

async function waitForMigrationJob(kubeEnv, namespace, serviceName, releaseDigest) {
  const suffix = releaseDigest.replace(/^sha256:/, "").slice(0, 12);
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const jobs = JSON.parse(await run(kubectlBin, ["get", "jobs", "--namespace", namespace, "--selector", `lenso.dev/autonomous-service=${serviceName},lenso.dev/workload-role=migration`, "--output=json"], true, kubeEnv));
    const current = jobs.items.find((job) => job.metadata.name.endsWith(suffix));
    if (current) return current;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`migration_incomplete: Operator did not create the release-bound Migration Job for ${serviceName}`);
}

async function assertGatewaySecurity(kubeEnv, image) {
  const script = `
const endpoint = 'http://lenso-m5-gateway/v1/tickets/security';
const unauthenticated = await fetch(endpoint);
if (unauthenticated.status !== 401) throw new Error('edge authentication was not enforced');
const wrongOrigin = await fetch(endpoint, { headers: { authorization: 'Bearer m5', origin: 'https://attacker.example' } });
if (wrongOrigin.status !== 403) throw new Error('edge CORS intent was not enforced');
const allowed = await fetch(endpoint, { headers: { authorization: 'Bearer m5', origin: 'https://production.support.example.test' } });
if (allowed.status !== 200 || allowed.headers.get('access-control-allow-origin') !== 'https://production.support.example.test') {
  throw new Error('allowed authenticated CORS request did not pass');
}
let rateBlocked = false;
for (let index = 0; index < 110; index += 1) {
  const response = await fetch(endpoint, { headers: { authorization: 'Bearer m5' } });
  if (response.status === 503 || response.status === 429) rateBlocked = true;
}
if (!rateBlocked) throw new Error('edge rate intent was not enforced');
console.log(JSON.stringify({ authentication: true, cors: true, rate: true }));
`;
  const output = await run(kubectlBin, [
    "run", `m5-edge-security-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  assert.deepEqual(JSON.parse(output.trim()), { authentication: true, cors: true, rate: true });
}

async function prepareDataPlaneOutage(kubeEnv, image) {
  const script = `
const service = 'http://service-support-production-support-api:8080';
const headers = {
  'x-service-principal': 'service:m5-acceptance-probe',
  'x-tenant-id': 'tenant:m5',
  'x-idempotency-key': 'outage-proof-1',
};
const preparedResponses = await Promise.all([
  fetch(service + '/internal/outage/prepare', { method: 'POST', headers }),
  fetch(service + '/internal/outage/prepare', { method: 'POST', headers }),
]);
const preparedBodies = await Promise.all(preparedResponses.map((response) => response.json()));
const body = preparedBodies[0];
if (preparedResponses.some((response) => response.status !== 200)
    || preparedBodies.some((item) => item.state !== 'prepared' || !item.durableCheckpointId)
    || preparedBodies[0].durableCheckpointId !== preparedBodies[1].durableCheckpointId
    || preparedBodies.filter((item) => item.mutated === true).length !== 1
    || preparedBodies.filter((item) => item.mutated === false).length !== 1) {
  throw new Error('durable operation was not prepared before coordination loss');
}

for (const pathname of ['/control/promotion', '/control/config-activation']) {
  const response = await fetch(service + pathname, { method: 'POST', headers });
  const control = await response.json();
  if (response.status !== 409 || control.code !== 'approval_required' || control.mutated !== false) {
    throw new Error('coordination surface was not live before outage: ' + pathname);
  }
}
console.log(JSON.stringify(body));
`;
  const output = await run(kubectlBin, [
    "run", `m5-outage-prepare-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  const prepared = JSON.parse(output.trim());
  assert.equal(prepared.state, "prepared");
  return prepared;
}

async function assertRuntimeConsoleIsNotAuthority(kubeEnv, image) {
  await run(kubectlBin, [
    "scale", "deployment/lenso-runtime-console", "--namespace", "lenso-m5-production", "--replicas=0",
  ], false, kubeEnv);
  await run(kubectlBin, [
    "rollout", "status", "deployment/lenso-runtime-console", "--namespace", "lenso-m5-production", "--timeout=90s",
  ], false, kubeEnv);
  const script = `
const response = await fetch('http://service-support-production-support-api:8080/control/promotion', { method: 'POST' });
const body = await response.json();
if (response.status !== 409 || body.code !== 'approval_required' || body.mutated !== false) {
  throw new Error('Runtime Console availability incorrectly controlled the protected-operation boundary');
}
console.log(JSON.stringify({ runtimeConsoleAuthoritative: false, systemPlaneAuthoritative: true }));
`;
  const output = await run(kubectlBin, [
    "run", `m5-console-authority-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  assert.deepEqual(JSON.parse(output.trim()), {
    runtimeConsoleAuthoritative: false,
    systemPlaneAuthoritative: true,
  });
}

async function evaluateSystemPlanePolicy(kubeEnv, image, input) {
  const script = `
const response = await fetch('http://lenso-system-plane:8080/system/delivery/policy/evaluate', {
  method: 'POST',
  headers: { authorization: 'Bearer dev-service:m5-policy-evaluator', 'content-type': 'application/json' },
  body: JSON.stringify(${JSON.stringify(input)}),
});
const body = await response.json();
if (!response.ok) throw new Error('actual System Plane policy evaluation failed: ' + JSON.stringify(body));
console.log(JSON.stringify(body));
`;
  const output = await run(kubectlBin, [
    "run", `m5-system-plane-policy-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  return JSON.parse(output.trim());
}

async function recordDeliveryArtifacts(kubeEnv, image, core) {
  const artifacts = [
    core.serviceRelease,
    core.trust,
    core.policy.local,
    core.previousConfigRevision,
    core.configRevision,
    core.configStage,
    core.configRollback,
    core.previousEdgeContract,
    core.stagingEdgeContract,
    core.productionEdgeContract,
    core.previousGatewayPlan,
    core.stagingGatewayPlan,
    core.productionGatewayPlan,
    core.previousGatewayObservation,
    core.stagingGatewayObservation,
    core.stagingDeploymentPlan,
    core.stagingDeploymentReceipt,
    core.stagingDeploymentObservation,
    core.previousDeploymentPlan,
    core.previousDeploymentReceipt,
    core.previousDeploymentObservation,
    core.productionDeploymentPlan,
    core.productionDeploymentObservation,
    core.environmentVerification,
    core.promotion,
    core.promotionApproval,
    core.promotionReceipt,
    core.canaryPlan,
    ...core.canaryObservations,
    ...core.canaryHistory,
    core.rollbackPlan,
    core.rollback,
    core.rollbackDeploymentObservation,
    core.rollbackGatewayObservation,
  ].filter(Boolean);
  const providerId = "ci:m5-acceptance";
  const batchSubject = digestJson([
    "lenso.delivery-artifact-batch.v1",
    core.serviceRelease.releaseId,
    canonicalJsonValue(artifacts),
  ]);
  const providerProof = `sha256:${createHash("sha256").update(`ephemeral-m5-key\0${batchSubject}`).digest("hex")}`;
  const script = `
const response = await fetch('http://lenso-system-plane:8080/system/delivery/deliveries/${encodeURIComponent(core.serviceRelease.releaseId)}/artifacts', {
  method: 'POST',
  headers: { authorization: 'Bearer dev-service:m5-delivery-recorder:runtime.deliveries.write', 'content-type': 'application/json' },
  body: JSON.stringify({
    providerId: '${providerId}',
    providerProof: '${providerProof}',
    artifacts: ${JSON.stringify(artifacts)},
  }),
});
const body = await response.json();
if (!response.ok || body.recorded !== ${artifacts.length}) throw new Error('System Plane did not persist delivery evidence: ' + JSON.stringify(body));
console.log(JSON.stringify(body));
`;
  const output = await run(kubectlBin, [
    "run", `m5-delivery-record-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  const receipt = JSON.parse(output.trim());
  assert.equal(receipt.effects.appendsLedger, true);
  assert.equal(receipt.batchSubject, batchSubject);
}

async function assertActualControlSurfaces(kubeEnv, image, core) {
  const script = `
const page = await fetch('http://lenso-runtime-console:8080/');
const html = await page.text();
if (!page.ok || !html.includes('<!doctype html>')) throw new Error('actual Runtime Console UI is unavailable');
const projection = await fetch('http://lenso-runtime-console:8080/admin/runtime/deliveries/current', {
  headers: { authorization: 'Bearer dev-service:admin:runtime.stories.read' },
});
const body = await projection.json();
if (!projection.ok || body.protocol !== 'lenso.delivery-console.v1') {
  throw new Error('actual Runtime Console did not read the System Plane delivery projection');
}
if (body.release?.releaseId !== '${core.serviceRelease.releaseId}' || body.state !== 'rolled_back') {
  throw new Error('Runtime Console projection did not explain the persisted rollback state: ' + JSON.stringify(body));
}
if (body.configuration.activeRevisionId !== '${core.previousConfigRevision.revisionId}'
    || body.configuration.previousRevisionId !== '${core.configRevision.revisionId}') {
  throw new Error('Runtime Console did not project the restored and previous Config Revisions: ' + JSON.stringify(body));
}
const production = body.deployments.find((deployment) => deployment.environment === 'production');
if (production?.observedReleaseId !== '${core.previousDeploymentPlan.releaseId}'
    || production?.configRevisionId !== '${core.previousConfigRevision.revisionId}') {
  throw new Error('Runtime Console did not project post-rollback production convergence: ' + JSON.stringify(body));
}
if (!body.edge?.publicRoutes?.includes('/v1/tickets/{ticketId}')
    || body.promotionHistory.length === 0
    || body.canaryObservations.length !== ${core.canaryObservations.length}
    || body.rollbackTimeline.length === 0) {
  throw new Error('Runtime Console delivery evidence is incomplete: ' + JSON.stringify(body));
}
const openapi = await fetch('http://lenso-system-plane:8080/openapi.json');
if (!openapi.ok) throw new Error('actual System Plane OpenAPI surface is unavailable');
console.log(JSON.stringify({ systemPlane: true, runtimeConsole: true, deliveryProjection: true }));
`;
  const output = await run(kubectlBin, [
    "run", `m5-control-surfaces-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  assert.deepEqual(JSON.parse(output.trim()), {
    systemPlane: true,
    runtimeConsole: true,
    deliveryProjection: true,
  });
}

async function observeDataPlaneOutage(workDir, kubeEnv, image, core, operatorObservation) {
  const script = `
const service = 'http://service-support-production-support-api:8080';
const headers = {
  'x-service-principal': 'service:m5-acceptance-probe',
  'x-tenant-id': 'tenant:m5',
  'x-idempotency-key': 'outage-proof-1',
};
const post = async (pathname, requestHeaders = headers) => {
  const response = await fetch(service + pathname, { method: 'POST', headers: requestHeaders });
  return { status: response.status, body: await response.json() };
};
const unauthorized = await post('/internal/outage/prove', {});
if (unauthorized.status !== 403 || unauthorized.body.mutated === true) throw new Error('service authorization weakened');
const first = await post('/internal/outage/prove');
const repeated = await post('/internal/outage/prove');
if (first.status !== 200 || repeated.status !== 200) throw new Error('established Data Plane proof failed');
if (first.body.durableCheckpointId !== repeated.body.durableCheckpointId || first.body.businessEffectCount !== repeated.body.businessEffectCount) {
  throw new Error('idempotent outage replay duplicated business effects');
}
const controlPaths = ['/control/promotion', '/control/config-activation', '/control/contract-retirement', '/control/release-mutation'];
const protectedOperations = {};
for (const pathname of controlPaths) {
  const result = await post(pathname);
  if (result.status !== 503 || result.body.code !== 'coordination_unavailable' || result.body.mutated !== false || !result.body.nextActions?.length) {
    throw new Error('protected mutation did not pause safely: ' + pathname);
  }
  protectedOperations[pathname] = result.body;
}
console.log(JSON.stringify({ ...first.body, protectedOperations, unauthorizedDenied: true }));
`;
  const output = await run(kubectlBin, [
    "run", `m5-outage-proof-${process.pid}`,
    "--namespace", "lenso-m5-production",
    `--image=${image}`,
    "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", script,
  ], true, kubeEnv);
  const raw = JSON.parse(output.trim());
  await writeJson(path.join(workDir, "outage-runtime-evidence.json"), raw);
  assert.equal(Object.values(raw.operationResults).every(Boolean), true);
  assert.equal(raw.unauthorizedDenied, true);
  assert.equal(raw.releaseId, core.previousDeploymentPlan.releaseId);
  assert.equal(raw.configRevisionId, core.previousDeploymentPlan.configRevisionId);
  const operationResults = canonicalDataPlaneOperationResults(raw.operationResults);
  const claims = {
    protocol: "lenso.coordination-outage-observation.v1",
    deploymentPlanId: core.previousDeploymentPlan.planId,
    deploymentPlanDigest: core.previousDeploymentPlan.planDigest,
    deploymentReceiptId: core.previousDeploymentReceipt.receiptId,
    deploymentObservationId: core.rollbackDeploymentObservation.observationId,
    operatorObservationId: operatorObservation.observationId,
    operatorObservationDigest: operatorObservation.observationDigest,
    environmentRevisionAfter: core.previousDeploymentReceipt.environmentRevisionAfter,
    releaseId: raw.releaseId,
    releaseDigest: core.previousDeploymentPlan.releaseDigest,
    configRevisionId: raw.configRevisionId,
    systemPlaneAvailable: false,
    runtimeConsoleAvailable: false,
    autonomousServiceRunning: true,
    selectedGatewayRunning: true,
    selectedTransportRunning: true,
    gatewayIsDataPlane: true,
    gatewayRequiresLivePolicy: false,
    gatewayRequiresLiveReleaseMetadata: false,
    lastValidConfigRevisionAvailable: raw.lastValidConfigRevisionAvailable,
    secretProviderLeaseValid: raw.secretProviderLeaseValid,
    secretRotationPolicyPreserved: raw.secretRotationPolicyPreserved,
    operationResults,
    security: raw.security,
    durableCheckpointId: raw.durableCheckpointId,
    evidenceReferences: [...raw.evidenceReferences].sort(),
  };
  const claimsFile = path.join(workDir, "outage-observation-claims.json");
  await writeJson(claimsFile, claims);
  const attestationOutput = await run(
    "cargo",
    [
      "run", "--quiet", "--locked", "--manifest-path", fixtureManifest,
      "--bin", "support-system-m5-attest-outage", "--", claimsFile,
    ],
    true,
  );
  const observation = JSON.parse(
    attestationOutput.match(/^M5_OUTAGE_OBSERVATION=(.+)$/m)?.[1] ?? "null",
  );
  assert.equal(observation?.claims?.durableCheckpointId, raw.durableCheckpointId);
  const file = path.join(workDir, "outage-observation.json");
  await writeJson(file, observation);
  return observation;
}

async function proveCoordinationResume(workDir, kubeEnv, namespace, resource, core, probeImage) {
  const before = JSON.parse(await run(kubectlBin, [
    "get", "jobs,deployments", "--namespace", namespace,
    "--selector", "lenso.dev/autonomous-service=service-support-production",
    "--output=json",
  ], true, kubeEnv));
  await run(kubectlBin, ["scale", "deployment/lenso-operator", "--namespace", "lenso-system", "--replicas=1"], false, kubeEnv);
  await run(kubectlBin, ["rollout", "status", "deployment/lenso-operator", "--namespace", "lenso-system", "--timeout=90s"], false, kubeEnv);
  for (const deployment of ["lenso-system-plane", "lenso-runtime-console"]) {
    await run(kubectlBin, ["scale", `deployment/${deployment}`, "--namespace", namespace, "--replicas=1"], false, kubeEnv);
    await run(kubectlBin, ["rollout", "status", `deployment/${deployment}`, "--namespace", namespace, "--timeout=90s"], false, kubeEnv);
  }
  const resumeInput = path.join(workDir, "coordination-resume-input.json");
  await writeJson(resumeInput, {
    outageProof: core.outage,
    deploymentSubject: core.previousDeploymentPlan,
    coordinationRevision: core.outage.environmentRevisionAfter + 1,
  });
  const interruptedResumeOutput = await run(
    "cargo",
    [
      "run", "--quiet", "--locked", "--manifest-path", fixtureManifest,
      "--bin", "support-system-m5-coordination-resume", "--", resumeInput,
    ],
    true,
    { M5_OPERATOR_OBSERVATION_PUBLIC_KEY: operatorObserverKeys.publicKeyBase64 },
  );
  const interruptedResumeEvidence = JSON.parse(interruptedResumeOutput.match(/^M5_COORDINATION_RESUME=(.+)$/m)?.[1] ?? "null");
  assert.equal(interruptedResumeEvidence.receiptCount, 1);
  assert.deepEqual(interruptedResumeEvidence.firstReceipt.effects, {
    mutatesConfiguration: false,
    mutatesGateway: false,
    mutatesDeployment: false,
    mutatesEnvironment: false,
    appendsLedger: false,
  }, "resume authorization must not claim the later Deployment effect");
  // Simulate a process crash after authorization and before the protected apply.
  const resumeOutput = await run(
    "cargo",
    [
      "run", "--quiet", "--locked", "--manifest-path", fixtureManifest,
      "--bin", "support-system-m5-coordination-resume", "--", resumeInput,
    ],
    true,
    { M5_OPERATOR_OBSERVATION_PUBLIC_KEY: operatorObserverKeys.publicKeyBase64 },
  );
  const resumeEvidence = JSON.parse(resumeOutput.match(/^M5_COORDINATION_RESUME=(.+)$/m)?.[1] ?? "null");
  assert.equal(resumeEvidence.receiptCount, 1);
  assert.deepEqual(resumeEvidence.firstReceipt, resumeEvidence.replayReceipt);
  assert.equal(resumeEvidence.firstReceipt.operationSubjectDigest, resumeEvidence.approval.operationSubjectDigest);
  assert.deepEqual(resumeEvidence.firstReceipt, interruptedResumeEvidence.firstReceipt, "restart changed the deterministic resume authorization");
  assert.equal(resumeEvidence.duplicateEffects, false);
  await run(kubectlBin, ["apply", "--namespace", namespace, "--filename", resource], false, kubeEnv);
  await run(kubectlBin, ["wait", "--namespace", namespace, "--for=jsonpath={.status.state}=ready", "lensoautonomousservice/service-support-production", "--timeout=180s"], false, kubeEnv);
  await run(kubectlBin, ["apply", "--namespace", namespace, "--filename", resource], false, kubeEnv);
  const after = JSON.parse(await run(kubectlBin, [
    "get", "jobs,deployments", "--namespace", namespace,
    "--selector", "lenso.dev/autonomous-service=service-support-production",
    "--output=json",
  ], true, kubeEnv));
  const beforeNames = before.items.map((item) => `${item.kind}/${item.metadata.name}`).sort();
  const afterNames = after.items.map((item) => `${item.kind}/${item.metadata.name}`).sort();
  assert.deepEqual(afterNames, beforeNames, "coordination resume duplicated Deployment or Migration effects");
  const expectedRuntime = JSON.parse(await readFile(path.join(workDir, "outage-runtime-evidence.json"), "utf8"));
  const replayScript = `
const response = await fetch('http://service-support-production-support-api:8080/internal/outage/prove', {
  method: 'POST',
  headers: {
    'x-service-principal': 'service:m5-acceptance-probe',
    'x-tenant-id': 'tenant:m5',
    'x-idempotency-key': 'outage-proof-1',
  },
});
const body = await response.json();
if (!response.ok) throw new Error(JSON.stringify(body));
console.log(JSON.stringify(body));
`;
  const replayOutput = await run(kubectlBin, [
    "run", `m5-resume-replay-${process.pid}`, "--namespace", namespace,
    `--image=${probeImage}`, "--restart=Never", "--rm", "--attach", "--command", "--",
    "node", "-e", replayScript,
  ], true, kubeEnv);
  const runtimeReplay = JSON.parse(replayOutput.trim());
  assert.equal(runtimeReplay.durableCheckpointId, expectedRuntime.durableCheckpointId);
  assert.equal(runtimeReplay.businessEffectCount, expectedRuntime.businessEffectCount);
  return {
    resumed: true,
    approval: resumeEvidence.approval,
    receipt: resumeEvidence.firstReceipt,
    receiptCount: resumeEvidence.receiptCount,
    duplicateEffects: false,
    businessEffectCount: runtimeReplay.businessEffectCount,
    resourceNames: afterNames,
  };
}

async function writeJson(file, value) {
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`);
}

function run(command, args, capture = false, extraEnv = {}, cwd = repoRoot, timeoutMs = 0) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: { ...process.env, ...extraEnv },
      stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit",
    });
    let settled = false;
    let stdout = "";
    let stderr = "";
    const finish = (callback) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      callback();
    };
    const timer = timeoutMs > 0
      ? setTimeout(() => {
        child.kill("SIGTERM");
        finish(() => reject(new Error(`${command} timed out after ${timeoutMs}ms`)));
      }, timeoutMs)
      : null;
    timer?.unref();
    child.stdout?.on("data", (chunk) => { stdout += chunk; });
    child.stderr?.on("data", (chunk) => { stderr += chunk; });
    child.once("error", (error) => finish(() =>
      reject(new Error(`${command} is unavailable: ${error.message}`))));
    child.once("exit", (code, signal) => {
      finish(() => {
        if (code === 0) resolve(stdout);
        else reject(new Error(`${command} exited with ${code ?? signal}${stderr ? `\n${stderr.trim()}` : ""}`));
      });
    });
  });
}
