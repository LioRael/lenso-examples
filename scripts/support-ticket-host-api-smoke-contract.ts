export const auditLogEventsReadCapability = "audit_log.events.read";

export const assertEqual = (actual, expected, message) => {
  if (actual !== expected) {
    throw new Error(
      `${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(
        actual,
      )}`,
    );
  }
};

export const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

export const expectLoadedModule = (modules, name) => {
  const module = modules.modules?.find((item) => item.module_name === name);
  assert(module, `${name} module was not listed`);
  assertEqual(module.status, "loaded", `${name} load status`);
  if (module.governance?.activation_state !== "active") {
    throw new Error(
      `${name} activation state: expected "active", got ${JSON.stringify(
        module.governance?.activation_state,
      )}\n${JSON.stringify(
        {
          activation_reasons: module.governance?.activation_reasons,
          manifest_lints: module.manifest_lints,
        },
        null,
        2,
      )}`,
    );
  }
  return module;
};

export const expectServiceLifecycle = (serviceModules, name, supportServer) => {
  const module = serviceModules.modules?.find((item) => item.moduleName === name);
  assert(module, `${name} service lifecycle was not listed`);
  assertEqual(module.providerName, "support-suite-provider", `${name} provider`);
  assertEqual(module.status, "ready", `${name} service lifecycle status`);
  assertEqual(
    module.manifestStatus,
    "reachable",
    `${name} service lifecycle manifest status`,
  );
  assertEqual(
    module.statusUrl,
    supportServer.statusUrl ?? `${supportServer.baseUrl}/status`,
    `${name} service status URL`,
  );
  assertEqual(
    module.serviceStatus?.state,
    "ready",
    `${name} service status endpoint state`,
  );
  assertEqual(
    module.compatibility?.state,
    "compatible",
    `${name} service compatibility state`,
  );
  return module;
};

export const expectAuditLogDataSurface = (module) => {
  assert(
    module.capabilities?.includes(auditLogEventsReadCapability),
    "audit-log events read capability was not listed",
  );
  assert(
    module.manifest_lints?.every((lint) => lint.severity === "ok"),
    "audit-log manifest lints were not all ok",
  );
  assertEqual(
    module.admin?.kind,
    "schema",
    "audit-log admin surface kind",
  );

  const eventEntity = module.admin?.entities?.find(
    (entity) => entity.name === "events",
  );
  assert(eventEntity, "audit-log events data surface was not listed");
  assertEqual(eventEntity.label, "Audit Events", "audit-log events label");
  assertEqual(
    eventEntity.read_capability,
    auditLogEventsReadCapability,
    "audit-log events read capability",
  );
  assert(
    eventEntity.fields?.some((field) => field.name === "event_name"),
    "audit-log events data surface did not expose event_name",
  );
};
