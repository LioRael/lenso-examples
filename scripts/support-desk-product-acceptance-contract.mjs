import { createHash, sign } from "node:crypto";
import {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
  SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS,
} from "../examples/support-ticket/src/contract.ts";

export {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
  SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS,
};
export const WORKLOAD_CONTROL_SCHEMA_DIGEST =
  "sha256:d3666bb1fd85576f9af4205dbcc70029acd81462678c47d2b315c40ef1a9161d";

export const digestJson = (value) =>
  `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;

const canonicalJson = (value) => {
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(",")}]`;
  }
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
};

export const digestCanonicalJson = (value) =>
  `sha256:${createHash("sha256").update(canonicalJson(value)).digest("hex")}`;

const signedArtifact = ({ artifact, keyId, privateKey }) => {
  const unsigned = structuredClone(artifact);
  delete unsigned.signature;
  // Enrollment contracts are serialized through serde_json::Value before
  // hashing, which recursively orders object keys. Keep this separate from
  // App Composition digests, whose contract preserves declared field order.
  const subjectDigest = digestCanonicalJson(unsigned);
  return {
    ...artifact,
    signature: {
      algorithm: "ed25519",
      keyId,
      subjectDigest,
      value: sign(null, Buffer.from(subjectDigest), privateKey).toString(
        "base64url"
      ),
    },
  };
};

export const publicEd25519KeyBase64url = (publicKey) => {
  const exported = publicKey.export({ format: "jwk" });
  if (exported.kty !== "OKP" || exported.crv !== "Ed25519" || !exported.x) {
    throw new Error("Expected an Ed25519 public key");
  }
  return exported.x;
};

export const buildEnrollmentExchange = ({
  consoleKeyId,
  consolePrivateKey,
  consoleServicePrincipal,
  expiresAtUnixMs,
  issuedAtUnixMs,
  managedServiceId,
  managedServicePrincipal,
  managedServiceRevision,
  policy,
  serviceKeyId,
  servicePrivateKey,
  systemId,
}) => {
  const nonce = `acceptance-${systemId}-0001`;
  const offer = signedArtifact({
    artifact: {
      protocol: "lenso.system-plane.enrollment-offer.v1",
      systemId,
      consoleServicePrincipal,
      nonce,
      issuedAtUnixMs,
      expiresAtUnixMs,
      requestedCapabilities: [],
      requestedPolicy: policy,
      signature: {},
    },
    keyId: consoleKeyId,
    privateKey: consolePrivateKey,
  });
  const receipt = signedArtifact({
    artifact: {
      protocol: "lenso.system-plane.enrollment-receipt.v1",
      offerDigest: offer.signature.subjectDigest,
      systemId,
      managedServiceId,
      managedServicePrincipal,
      managedServiceRevision,
      consoleServicePrincipal,
      nonce,
      issuedAtUnixMs,
      expiresAtUnixMs,
      grantRevision: 1,
      authorizationEpoch: 1,
      grantedCapabilities: [],
      grantedPolicy: policy,
      signature: {},
    },
    keyId: serviceKeyId,
    privateKey: servicePrivateKey,
  });
  return { offer, receipt };
};

const artifactFor = (artifacts, moduleId) => {
  if (moduleId === artifacts.supportTicket.moduleId) {
    return artifacts.supportTicket;
  }
  if (moduleId === artifacts.story.moduleId) {
    return artifacts.story;
  }
  return undefined;
};

const topologyModule = (module, artifacts, supportTicket) => {
  const artifact = artifactFor(artifacts, module.moduleId);
  const serviceReference = module.implementation.serviceReference;
  const serviceId =
    module.implementation.kind === "service"
      ? serviceReference.split("/").at(-1)
      : null;
  const projected = {
    moduleId: module.moduleId,
    delivery: module.implementation.kind,
    serviceId,
    moduleReleaseDigest: module.release.contentDigest,
    consoleUiArtifactDigest: artifact?.artifactDigest ?? null,
    runtimeStatus: "active",
  };
  if (module.moduleId === artifacts.supportTicket.moduleId) {
    projected.surfaceApiGrant = {
      artifactDigest: artifacts.supportTicket.artifactDigest,
      moduleReleaseDigest: module.release.contentDigest,
      contractDigest: SUPPORT_TICKET_CONTRACT_DIGEST,
      operationIds: [...SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS],
      contractArtifact: {
        format: "openapi_3_1_json",
        document: supportTicket.surfaceContractDocument,
      },
    };
    const { runtimeStatus } = projected;
    delete projected.runtimeStatus;
    projected.runtimeStatus = runtimeStatus;
  }
  return projected;
};

export const buildSystemConnectRequest = ({
  adapterState,
  artifacts,
  composition,
  policy,
  supportTicket,
}) => {
  if (composition.appId !== adapterState.adapterWorkload.systemId) {
    throw new Error("Adapter state belongs to a different App Composition");
  }
  const adapterCapabilities = ["suspend", "resume"].filter((capability) =>
    adapterState.capabilities.includes(capability)
  );
  if (adapterCapabilities.length !== 2) {
    throw new Error("Local Adapter must expose Suspend and Resume");
  }
  const services = [
    {
      serviceId: adapterState.adapterWorkload.serviceId,
      servicePrincipal: "svc.lenso-local-control-adapter",
      revision: 1,
      workloads: [
        {
          workloadId: adapterState.adapterWorkload.workloadId,
          role: "control_adapter",
        },
      ],
    },
    {
      serviceId: supportTicket.serviceId,
      servicePrincipal: supportTicket.servicePrincipal,
      revision: 1,
      workloads: [
        { workloadId: supportTicket.workloadId, role: "api" },
      ],
    },
  ].sort((left, right) => left.serviceId.localeCompare(right.serviceId));
  const modules = composition.modules
    .map((module) => topologyModule(module, artifacts, supportTicket))
    .sort((left, right) => left.moduleId.localeCompare(right.moduleId));
  const topology = {
    protocol: "lenso.system.v2",
    systemId: composition.appId,
    services,
    modules,
    adapters: [
      {
        adapterId: adapterState.adapterId,
        capabilities: ["workload_control"],
        // Preserve the framework WorkloadReference serialization order because
        // the topology digest binds the exact typed JSON document.
        workload: {
          systemId: adapterState.adapterWorkload.systemId,
          serviceId: adapterState.adapterWorkload.serviceId,
          workloadId: adapterState.adapterWorkload.workloadId,
        },
        workloadControl: {
          protocol: adapterState.workloadControlProtocol,
          schemaDigest: adapterState.workloadControlSchemaDigest,
          status: "connected",
          capabilities: adapterCapabilities,
        },
      },
    ],
  };
  const topologyDigest = digestJson(topology);
  return {
    systemId: composition.appId,
    topologyDigest,
    topology,
    managementBinding: {
      systemId: composition.appId,
      topologyDigest,
      serviceIds: services.map((service) => service.serviceId),
      adapterIds: [adapterState.adapterId],
      permissions: [
        "console.module.business.read",
        "console.module.business.write",
        "console.workload.control",
        "console.workload.operation.read",
        "console.workload.read",
      ],
      policy,
    },
  };
};

export const storyStatusRequest = (connected, status) => {
  const request = structuredClone(connected);
  const story = request.topology.modules.find(
    (module) => module.moduleId === "lenso/platform-story"
  );
  if (!story) {
    throw new Error("Connected topology does not contain the Story Module");
  }
  story.runtimeStatus = status;
  request.topologyDigest = digestJson(request.topology);
  request.managementBinding.topologyDigest = request.topologyDigest;
  return request;
};
