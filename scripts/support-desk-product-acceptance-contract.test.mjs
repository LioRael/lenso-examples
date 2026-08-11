import assert from "node:assert/strict";
import { generateKeyPairSync, verify } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import {
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
  WORKLOAD_CONTROL_SCHEMA_DIGEST,
  SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS,
  buildEnrollmentExchange,
  buildSystemConnectRequest,
  digestCanonicalJson,
  digestJson,
  publicEd25519KeyBase64url,
  storyStatusRequest,
} from "./support-desk-product-acceptance-contract.mjs";
import {
  assertBrowserSessionDoesNotReuseServerCredential,
  assertNoForbiddenEvidence,
  extractSameOriginConsoleBearerTokens,
  extractFirstJsonObject,
  findNewPlaywrightTicketTitleControl,
  findPlaywrightTicketTitleControl,
  formatPlaywrightGatewayCandidateDiagnostics,
  isSuccessfulPlaywrightGatewayCandidate,
  parsePlaywrightRequestEntries,
  parsePlaywrightTicketTitleControls,
  redactSameOriginSessionAuthorization,
  redactedAccessibleSnapshotDiagnostic,
} from "./support-desk-product-acceptance-runtime.mjs";

const root = path.resolve(import.meta.dirname, "..");
const fixtureRoot = path.join(
  root,
  "fixtures",
  "acceptance",
  "support-desk"
);

const digest = (character) => `sha256:${character.repeat(64)}`;

test("extracts one complete Adapter state around arbitrary log output", () => {
  const state = {
    message: "a quoted } and escaped \\\" brace",
    phase: "ready",
    schema: "lenso.local-control-adapter-state.v2",
  };
  const output = `prefix {not-json}\n${JSON.stringify(state, null, 2)}\ntrailing {log}`;

  assert.deepEqual(extractFirstJsonObject(output), state);
});

test("redacts and bounds a failed accessibility snapshot diagnostic", () => {
  const identifier = "acceptance-operator@example.test";
  const password = "correct horse battery staple";
  const snapshot = [
    "TOP OF SNAPSHOT",
    `- textbox \"Identifier\" [ref=e1]: ${identifier}`,
    `- textbox \"Password\" [ref=e2]: ${password}`,
    '- textbox "Notes" [ref=e3]: private form input',
    '- generic [ref=e4] value="another private value"',
    "x".repeat(1_000),
    "BOTTOM OF SNAPSHOT",
  ].join("\n");

  const diagnostic = redactedAccessibleSnapshotDiagnostic(snapshot, {
    maxChars: 512,
    sensitiveValues: [identifier, password],
  });

  assert.match(diagnostic, /TOP OF SNAPSHOT/u);
  assert.match(diagnostic, /BOTTOM OF SNAPSHOT/u);
  assert.match(diagnostic, /accessible snapshot truncated/u);
  assert.doesNotMatch(diagnostic, /acceptance-operator@example\.test/u);
  assert.doesNotMatch(diagnostic, /correct horse battery staple/u);
  assert.doesNotMatch(diagnostic, /private form input/u);
  assert.doesNotMatch(diagnostic, /another private value/u);
  assert.ok(diagnostic.length <= 512);
});

test("reports forbidden browser evidence by label without echoing the value", () => {
  const secret = "never-print-this-session-token";

  assert.throws(
    () =>
      assertNoForbiddenEvidence(`request authorization: Bearer ${secret}`, [
        { label: "operator session token", value: secret },
      ]),
    (error) => {
      assert.match(error.message, /operator session token/u);
      assert.doesNotMatch(error.message, /never-print-this-session-token/u);
      return true;
    }
  );
});

test("redacts a browser session only from an exact same-origin Authorization header", () => {
  const consoleOrigin = "http://127.0.0.1:3000";
  const token = "sess_browser_operator";
  const sameOrigin = {
    method: "GET",
    url: `${consoleOrigin}/api/console/v1/system`,
  };

  const redacted = redactSameOriginSessionAuthorization(
    {
      ...sameOrigin,
      details: `  Request headers\n    authorization: Bearer ${token}`,
    },
    { consoleOrigin, sessionTokens: [token] }
  );
  assert.match(redacted, /authorization: Bearer \[redacted same-origin session\]/u);
  assert.doesNotMatch(redacted, /sess_browser_operator/u);

  for (const record of [
    {
      ...sameOrigin,
      details: `  Request headers\n    x-debug-token: ${token}`,
    },
    {
      ...sameOrigin,
      details: `  Response headers\n    set-cookie: lenso_session=${token}`,
    },
    {
      details: `  Request headers\n    authorization: Bearer ${token}`,
      method: "GET",
      url: "http://provider.internal/api/tickets",
    },
  ]) {
    assert.match(
      redactSameOriginSessionAuthorization(record, {
        consoleOrigin,
        sessionTokens: [token],
      }),
      /sess_browser_operator/u,
      "the session must remain forbidden outside the exact same-origin Authorization header"
    );
  }
});

test("extracts one browser session from authenticated same-origin Console API requests", () => {
  const consoleOrigin = "http://127.0.0.1:3000";
  const token = "sess_browser_operator";
  const details = `  Request headers\n    authorization: Bearer ${token}`;

  assert.deepEqual(
    extractSameOriginConsoleBearerTokens(
      [
        {
          details,
          method: "GET",
          url: `${consoleOrigin}/api/console/v1/system`,
        },
        {
          details,
          method: "POST",
          url: `${consoleOrigin}/api/console/v1/services/support-ticket/surface-gateway`,
        },
        {
          details: "  Request headers\n    authorization: Bearer cross-origin-token",
          method: "GET",
          url: "http://provider.internal/api/tickets",
        },
      ],
      { consoleOrigin }
    ),
    [token]
  );
});

test("rejects a browser session that reuses a server-owned credential without echoing it", () => {
  const credential = "server-owned-adapter-token";

  assert.throws(
    () =>
      assertBrowserSessionDoesNotReuseServerCredential(
        [credential],
        [{ label: "Local Adapter bearer token", value: credential }]
      ),
    (error) => {
      assert.match(error.message, /Local Adapter bearer token/u);
      assert.doesNotMatch(error.message, /server-owned-adapter-token/u);
      return true;
    }
  );
});

test("locates a newly rendered ticket row without matching the create draft", () => {
  const snapshot = [
    '- textbox "New ticket" [ref=draft]: Browser acceptance ticket',
    '- textbox "Title for ticket_1" [ref=existing]: Existing ticket',
    '- textbox "Title for ticket_2" [ref=created]: Browser acceptance ticket',
  ].join("\n");

  assert.deepEqual(parsePlaywrightTicketTitleControls(snapshot), [
    {
      line: '- textbox "Title for ticket_1" [ref=existing]: Existing ticket',
      ref: "existing",
      ticketId: "ticket_1",
      value: "Existing ticket",
    },
    {
      line: '- textbox "Title for ticket_2" [ref=created]: Browser acceptance ticket',
      ref: "created",
      ticketId: "ticket_2",
      value: "Browser acceptance ticket",
    },
  ]);
  assert.deepEqual(
    findNewPlaywrightTicketTitleControl(snapshot, {
      existingTicketIds: ["ticket_1"],
      title: "Browser acceptance ticket",
    }),
    {
      line: '- textbox "Title for ticket_2" [ref=created]: Browser acceptance ticket',
      ref: "created",
      ticketId: "ticket_2",
      value: "Browser acceptance ticket",
    }
  );
  assert.equal(
    findPlaywrightTicketTitleControl(snapshot, {
      ticketId: "ticket_2",
      title: "Browser acceptance ticket",
    })?.ref,
    "created"
  );
});

test("parses Playwright network rows for Surface Gateway response proof", () => {
  assert.deepEqual(
    parsePlaywrightRequestEntries(
      [
        "12. [GET] http://127.0.0.1:3000/api/console/v1/system => [200] OK",
        "13. [POST] http://127.0.0.1:3000/api/console/v1/services/support-ticket/surface-gateway => [200] OK",
        "14. [POST] http://127.0.0.1:3000/api/console/v1/services/support-ticket/surface-gateway => [FAILED] net::ERR_CONNECTION_RESET",
      ].join("\n")
    ),
    [
      {
        index: "12",
        method: "GET",
        status: "200",
        url: "http://127.0.0.1:3000/api/console/v1/system",
      },
      {
        index: "13",
        method: "POST",
        status: "200",
        url: "http://127.0.0.1:3000/api/console/v1/services/support-ticket/surface-gateway",
      },
      {
        index: "14",
        method: "POST",
        status: "FAILED",
        url: "http://127.0.0.1:3000/api/console/v1/services/support-ticket/surface-gateway",
      },
    ]
  );
});

test("reports only safe Surface Gateway candidate metadata", () => {
  const diagnostics = formatPlaywrightGatewayCandidateDiagnostics([
    {
      entry: {
        index: "13",
        method: "POST",
        status: "200",
        url: "http://acceptance-secret.example/api/console/v1/services/support-ticket/surface-gateway?token=query-secret",
      },
      request: {
        input: { password: "request-secret" },
        operationId: "support-ticket/http/POST:/tickets",
      },
      requestBodyState: "json",
      response: {
        operationId: "support-ticket/http/POST:/tickets",
        output: { token: "response-secret" },
        protocol: "lenso.console-surface-gateway.v1",
      },
      responseBodyState: "json",
    },
  ]);

  assert.match(
    diagnostics,
    /#13 POST \/api\/console\/v1\/services\/support-ticket\/surface-gateway status=200/u
  );
  assert.match(
    diagnostics,
    /requestOperationId="support-ticket\/http\/POST:\/tickets"/u
  );
  assert.match(
    diagnostics,
    /responseProtocol="lenso\.console-surface-gateway\.v1"/u
  );
  assert.match(diagnostics, /responseHasOutput=true/u);
  for (const secret of [
    "acceptance-secret.example",
    "query-secret",
    "request-secret",
    "response-secret",
  ]) {
    assert.equal(diagnostics.includes(secret), false);
  }
});

test("accepts an exact successful Gateway response when Playwright omits the request body", () => {
  assert.equal(
    isSuccessfulPlaywrightGatewayCandidate(
      {
        entry: {
          index: "41",
          method: "POST",
          status: "200",
          url: "http://127.0.0.1:3000/api/console/v1/services/support-ticket/surface-gateway",
        },
        request: null,
        requestBodyState: "unavailable",
        response: {
          operationId: "support-ticket/http/POST:/tickets",
          output: { ticket: { id: "ticket-browser" } },
          protocol: "lenso.console-surface-gateway.v1",
        },
        responseBodyState: "json",
      },
      {
        consoleOrigin: "http://127.0.0.1:3000",
        operationId: "support-ticket/http/POST:/tickets",
        surfaceGatewayPath:
          "/api/console/v1/services/support-ticket/surface-gateway",
      }
    ),
    true
  );
});

test("builds a framework-verifiable bilateral enrollment exchange", () => {
  const consoleKeys = generateKeyPairSync("ed25519");
  const serviceKeys = generateKeyPairSync("ed25519");
  const nowUnixMs = 1_782_000_000_000;
  const exchange = buildEnrollmentExchange({
    consoleKeyId: "acceptance-console-key",
    consolePrivateKey: consoleKeys.privateKey,
    consoleServicePrincipal: "svc.lenso-console",
    expiresAtUnixMs: nowUnixMs + 300_000,
    issuedAtUnixMs: nowUnixMs,
    managedServiceId: "support-ticket",
    managedServicePrincipal: "svc.support-ticket",
    managedServiceRevision: "1",
    policy: {
      policyDigest: digest("a"),
      policyId: "support-desk-management",
      policyRevision: "1",
    },
    serviceKeyId: "acceptance-support-ticket-key",
    servicePrivateKey: serviceKeys.privateKey,
    systemId: "support-desk",
  });

  assert.equal(exchange.offer.protocol, "lenso.system-plane.enrollment-offer.v1");
  assert.equal(
    exchange.receipt.protocol,
    "lenso.system-plane.enrollment-receipt.v1"
  );
  assert.equal(exchange.receipt.offerDigest, exchange.offer.signature.subjectDigest);
  const unsignedOffer = structuredClone(exchange.offer);
  delete unsignedOffer.signature;
  const unsignedReceipt = structuredClone(exchange.receipt);
  delete unsignedReceipt.signature;
  assert.equal(
    exchange.offer.signature.subjectDigest,
    digestCanonicalJson(unsignedOffer)
  );
  assert.equal(
    exchange.receipt.signature.subjectDigest,
    digestCanonicalJson(unsignedReceipt)
  );
  assert.equal(exchange.offer.signature.algorithm, "ed25519");
  assert.equal(exchange.receipt.signature.algorithm, "ed25519");
  assert.equal(exchange.offer.signature.value.length, 86);
  assert.equal(exchange.receipt.signature.value.length, 86);
  assert.equal(
    verify(
      null,
      Buffer.from(exchange.offer.signature.subjectDigest),
      consoleKeys.publicKey,
      Buffer.from(exchange.offer.signature.value, "base64url")
    ),
    true
  );
  assert.equal(
    verify(
      null,
      Buffer.from(exchange.receipt.signature.subjectDigest),
      serviceKeys.publicKey,
      Buffer.from(exchange.receipt.signature.value, "base64url")
    ),
    true
  );
  assert.match(publicEd25519KeyBase64url(consoleKeys.publicKey), /^[\w-]{43}$/u);
});

test("builds the exact connected topology from composition, artifacts, and Adapter state", () => {
  const moduleReleaseDigest = digest("b");
  const supportArtifactDigest = digest("c");
  const storyArtifactDigest = digest("d");
  const request = buildSystemConnectRequest({
    adapterState: {
      adapterId: "workload-control:support-desk",
      adapterWorkload: {
        serviceId: "lenso-local-control-adapter",
        systemId: "support-desk",
        workloadId: "workload-control:support-desk",
      },
      capabilities: ["resume", "suspend"],
      workloadControlProtocol: "lenso.workload-control.v1",
      workloadControlSchemaDigest: WORKLOAD_CONTROL_SCHEMA_DIGEST,
    },
    artifacts: {
      story: {
        artifactDigest: storyArtifactDigest,
        moduleId: "lenso/platform-story",
      },
      supportTicket: {
        artifactDigest: supportArtifactDigest,
        moduleId: "support/tickets",
      },
    },
    composition: {
      appId: "support-desk",
      modules: [
        {
          implementation: { kind: "linked" },
          moduleId: "auth",
          release: { contentDigest: digest("e") },
        },
        {
          implementation: { kind: "linked" },
          moduleId: "lenso/platform-story",
          release: { contentDigest: digest("f") },
        },
        {
          implementation: {
            kind: "service",
            serviceReference: "service:support-suite-provider/support-ticket",
          },
          moduleId: "support/tickets",
          release: { contentDigest: moduleReleaseDigest },
        },
      ],
    },
    policy: {
      digest: digest("a"),
      policyId: "support-desk-management",
      revision: 1,
    },
    supportTicket: {
      serviceId: "support-ticket",
      servicePrincipal: "svc.support-ticket",
      workloadId: "support-ticket-api",
    },
  });

  assert.equal(request.systemId, "support-desk");
  assert.equal(request.topologyDigest, digestJson(request.topology));
  assert.deepEqual(request.managementBinding.serviceIds, [
    "lenso-local-control-adapter",
    "support-ticket",
  ]);
  assert.deepEqual(request.managementBinding.adapterIds, [
    "workload-control:support-desk",
  ]);
  assert.deepEqual(Object.keys(request.topology.adapters[0].workload), [
    "systemId",
    "serviceId",
    "workloadId",
  ]);
  assert.equal(
    request.topology.modules.find((module) => module.moduleId === "support/tickets")
      ?.surfaceApiGrant?.contractDigest,
    SUPPORT_TICKET_CONTRACT_DIGEST
  );
  const operationIds = request.topology.modules.find(
    (module) => module.moduleId === "support/tickets"
  )?.surfaceApiGrant?.operationIds;
  assert.deepEqual(operationIds, SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS);
  assert.deepEqual(operationIds, [...new Set(operationIds)].sort());
  assert.equal(operationIds.includes(SUPPORT_TICKET_OPERATION_IDS.detail), false);
  assert.equal(
    operationIds.includes(SUPPORT_TICKET_OPERATION_IDS.restrictedDetail),
    true
  );
  assert.equal(
    request.topology.adapters[0]?.workloadControl?.schemaDigest,
    WORKLOAD_CONTROL_SCHEMA_DIGEST
  );

  const incompatibleStory = storyStatusRequest(request, "incompatible");
  assert.equal(
    request.topology.modules.find(
      (module) => module.moduleId === "lenso/platform-story"
    )?.runtimeStatus,
    "active",
    "the connected request must remain reusable after the negative vector"
  );
  assert.equal(
    incompatibleStory.topology.modules.find(
      (module) => module.moduleId === "lenso/platform-story"
    )?.runtimeStatus,
    "incompatible"
  );
  assert.equal(
    incompatibleStory.topology.modules.find(
      (module) => module.moduleId === "support/tickets"
    )?.runtimeStatus,
    "active",
    "the incompatibility vector must target only the exact Story Surface"
  );
  assert.equal(
    incompatibleStory.topologyDigest,
    digestJson(incompatibleStory.topology)
  );
  assert.equal(
    incompatibleStory.managementBinding.topologyDigest,
    incompatibleStory.topologyDigest
  );
});

test("acceptance fixture contains only current application-model entrypoints", async () => {
  const composition = JSON.parse(
    await readFile(path.join(fixtureRoot, "lenso.app.json"), "utf8")
  );
  assert.equal(composition.protocol, "lenso.app-composition.v1");
  assert.equal(composition.appId, "support-desk");
  assert.equal(composition.revision, 1);
  assert.equal(
    composition.contentDigest,
    digestJson({
      protocol: composition.protocol,
      appId: composition.appId,
      revision: composition.revision,
      modules: composition.modules,
      provenance: composition.provenance,
    })
  );
  assert.equal(
    composition.modules.filter(
      (module) => module.implementation.kind === "service"
    ).length,
    1
  );
  assert.equal(
    composition.modules.find((module) => module.moduleId === "support/tickets")
      ?.implementation.serviceReference,
    "service:support-suite-provider/support-ticket"
  );

  const pack = JSON.parse(
    await readFile(path.join(fixtureRoot, "capability", "lenso.capability.json"), "utf8")
  );
  assert.equal(pack.protocol, "lenso.capability-pack.v1");
  assert.deepEqual(
    pack.modules.map((module) => module.name).sort(),
    ["lenso/platform-story", "support/tickets"]
  );

  const acceptanceSource = (
    await Promise.all(
      [
        "support-desk-product-acceptance.mjs",
        "support-desk-product-acceptance-runtime.mjs",
        "support-desk-generated-client-acceptance.mjs",
      ].map((file) => readFile(path.join(root, "scripts", file), "utf8"))
    )
  ).join("\n");
  for (const retired of [
    "/admin/data",
    "admin_action",
    "isolated_web",
    "lenso.console-bridge",
    "app-change-plan",
  ]) {
    assert.equal(
      acceptanceSource.includes(retired),
      false,
      `acceptance must not use retired ${retired}`
    );
  }
  assert.match(acceptanceSource, /"app",\s*"compose"/u);
  assert.match(acceptanceSource, /"system",\s*"dev"/u);
  assert.match(acceptanceSource, /enrollment-receipts/u);
  assert.match(acceptanceSource, /surface-gateway/u);
  assert.match(acceptanceSource, /workloads/u);
  assert.match(acceptanceSource, /bootstrap\/v1\/recovery/u);
  assert.match(
    acceptanceSource,
    /cargo is required to migrate and start the Console Service/u
  );
  assert.equal(
    acceptanceSource.includes('resolveExecutable("lenso")'),
    false,
    "without an explicit binary override, acceptance must build the exact CLI checkout"
  );
  assert.match(acceptanceSource, /cwd: cliRoot, label: "cli-build"/u);
  assert.match(
    acceptanceSource,
    /expiresAtUnixMs: nowUnixMs \+ 60 \* 60_000/u,
    "the signed enrollment must outlive the 45-minute cold-CI budget"
  );
  assert.match(
    acceptanceSource,
    /catch \{\s*await terminateManaged\(sandboxProcess\);\s*\}/u,
    "cleanup must terminate a Local Adapter process that misses its graceful deadline"
  );
  assert.equal(
    acceptanceSource.includes(
      "waitForExit(child, 5_000).catch(() => undefined)"
    ),
    false,
    "forced process cleanup must not hide a surviving process tree"
  );
  assert.match(acceptanceSource, /environmentId: systemId/u);
  assert.match(acceptanceSource, /VITE_API_AUTH_TOKEN: undefined/u);
  assert.match(acceptanceSource, /waitSnapshot\("New ticket"\)/u);
  assert.doesNotMatch(
    acceptanceSource,
    /waitSnapshot\("Support tickets"\)/u,
    "browser readiness must wait for the Support Ticket form, not its sidebar label"
  );
  assert.match(
    acceptanceSource,
    /const initialListRequestIndexes = await browserRequestIndexes\(\);[\s\S]*?await invoke\(\["click", signIn\]\);[\s\S]*?operationId: SUPPORT_TICKET_OPERATION_IDS\.list/u,
    "browser login must wait for a new exact Surface Gateway list exchange"
  );
  assert.match(
    acceptanceSource,
    /assert\.equal\(listExchange\.entry\.status, "200"\)/u,
    "the initial list proof must require a successful Gateway response"
  );
  assert.match(
    acceptanceSource,
    /assert\.equal\(\s*listExchange\.response\.operationId,\s*SUPPORT_TICKET_OPERATION_IDS\.list/u,
    "the initial Gateway response must be bound to the list operation"
  );
  assert.match(
    acceptanceSource,
    /listExchange\.response\.output\.records\.find\([\s\S]*?ticket\.id === "ticket_1"/u,
    "the initial list response must include the seeded ticket"
  );
  assert.match(
    acceptanceSource,
    /ticketId: seededTicket\.id,\s*title: seededTicket\.title/u,
    "the rendered seeded row must be selected using the Gateway response identity and title"
  );
  assert.match(
    acceptanceSource,
    /assert\.equal\(seededRow\.control\.ticketId, seededTicket\.id\)/u
  );
  assert.match(acceptanceSource, /redactedAccessibleSnapshotDiagnostic/u);
  assert.match(acceptanceSource, /Last accessible snapshot/u);
  assert.match(acceptanceSource, /findNewPlaywrightTicketTitleControl/u);
  assert.match(acceptanceSource, /waitForSurfaceGatewayResponse/u);
  assert.doesNotMatch(
    acceptanceSource,
    /\["requests",\s*"--static"/u,
    "browser request polling must not include successful static resources"
  );
  assert.match(
    acceptanceSource,
    /\[\s*"requests",\s*"--filter",\s*surfaceGatewayRequestFilter,\s*"--raw",?\s*\]/u
  );
  assert.match(acceptanceSource, /Surface Gateway candidates \(metadata only\)/u);
  assert.match(
    acceptanceSource,
    /assertNoForbiddenEvidence\(publicEvidence, forbiddenEvidence\)/u,
    "browser evidence failures must report only stable forbidden-value labels"
  );
  assert.match(
    acceptanceSource,
    /extractSameOriginConsoleBearerTokens\(\s*requestRecords,\s*\{ consoleOrigin \}\s*\)/u,
    "the browser session allowlist must come from authenticated same-origin Console requests"
  );
  assert.match(
    acceptanceSource,
    /redactSameOriginSessionAuthorization\(record, \{/u,
    "only the public evidence copy may redact the verified same-origin session header"
  );
  const sessionCredentialGuard = acceptanceSource.indexOf(
    "assertBrowserSessionDoesNotReuseServerCredential("
  );
  const sessionHeaderRedaction = acceptanceSource.indexOf(
    "redactSameOriginSessionAuthorization(record,"
  );
  assert.ok(
    sessionCredentialGuard !== -1 &&
      sessionHeaderRedaction !== -1 &&
      sessionCredentialGuard < sessionHeaderRedaction,
    "every browser session must be checked against server-owned credentials before evidence redaction"
  );
  for (const label of [
    "Local Adapter bearer token",
    "artifact setup token",
    "recovery token",
    "operator session token",
    "System Plane bearer token",
    "private key fragment",
  ]) {
    assert.match(acceptanceSource, new RegExp(`label: [^\\n]*${label}`, "u"));
  }
  assert.match(
    acceptanceSource,
    /if \(privateAuthExchange\) \{\s*continue;\s*\}/u,
    "the private password-login exchange must not collect request or response bodies"
  );
  assert.match(
    acceptanceSource,
    /if \(record\.privateAuthExchange\) \{\s*continue;\s*\}/u,
    "password-login headers and bodies must stay outside public evidence"
  );
  assert.match(
    acceptanceSource,
    /browserSessionTokens\.includes\(sessionToken\)/u,
    "the bootstrap session must remain globally forbidden, including same-origin headers"
  );
  assert.match(acceptanceSource, /label: `browser operator session token /u);
  assert.match(acceptanceSource, /label: "operator session token"/u);
  assert.doesNotMatch(acceptanceSource, /for \(const secret of \[/u);
  assert.doesNotMatch(
    acceptanceSource,
    /waitSnapshot\("Browser acceptance ticket(?: updated)?"\)/u
  );
  assert.doesNotMatch(
    acceptanceSource,
    /invoke\(\["goto", `\$\{consoleUrl\}\/support\/tickets`\]\)/u,
    "browser CRUD must retain the network history that proves its Gateway exchanges"
  );
  const supportNetworkEvidence = acceptanceSource.indexOf(
    "const supportNetworkEvidence = await collectBrowserNetworkEvidence()"
  );
  const storiesNavigation = acceptanceSource.indexOf(
    'await invoke(["goto", `${consoleUrl}/stories`])',
    supportNetworkEvidence
  );
  assert.ok(
    supportNetworkEvidence !== -1 &&
      storiesNavigation !== -1 &&
      supportNetworkEvidence < storiesNavigation,
    "Support Ticket network evidence must be captured before Stories navigation clears it"
  );
  assert.match(
    acceptanceSource,
    /artifactSetupToken = `dev-user:acceptance-artifact-[^`]+:console\.artifacts\.manage`/u
  );
  assert.match(
    acceptanceSource,
    /artifacts\/reconcile`,\s*artifactSetupAuthorization/u
  );
  assert.match(
    acceptanceSource,
    /LENSO_ACCEPTANCE_CONSOLE_AUTHORIZATION: operatorAuthorization/u
  );
  assert.equal(
    (acceptanceSource.match(/dev-user:/gu) ?? []).length,
    1,
    "the loopback artifact installer is the only dev actor"
  );
  for (const vector of [
    "wrong-module-release",
    "wrong-ui-artifact",
    "wrong-delegated-actor",
    "wrong-target-principal",
  ]) {
    assert.match(acceptanceSource, new RegExp(vector, "u"));
  }
  assert.match(
    acceptanceSource,
    /rejected Surface Gateway tamper vectors must not execute the Provider/u
  );
  for (const vector of [
    "surface-grant-denied-detail",
    "module-authority-denied-restricted-detail",
  ]) {
    assert.match(acceptanceSource, new RegExp(vector, "u"));
  }
  assert.match(
    acceptanceSource,
    /SUPPORT_TICKET_OPERATION_IDS\.detail\]: 0/u,
    "the exact Surface Grant denial must prove the Provider did not execute"
  );
  assert.match(
    acceptanceSource,
    /SUPPORT_TICKET_OPERATION_IDS\.restrictedDetail\]: 1/u,
    "the Module-authority denial must prove the Gateway forwarded once"
  );
  assert.match(
    acceptanceSource,
    /assert\.equal\(restrictedContext\.accepted, false\)/u,
    "the Provider observation must retain the final Module authorization decision"
  );
});

test("product acceptance proves unavailable Stories in real browser sessions before the authorized path", async () => {
  const acceptanceSource = await readFile(
    path.join(root, "scripts", "support-desk-product-acceptance.mjs"),
    "utf8"
  );

  assert.match(
    acceptanceSource,
    /Module workload is incompatible with the System topology/u
  );
  assert.match(
    acceptanceSource,
    /Current operator lacks the required Surface Entry Capability: runtime\.stories\.read/u
  );
  assert.match(
    acceptanceSource,
    /assert\.doesNotMatch\(overviewPage, availableStoriesNavigation/u,
    "each negative browser session must prove Stories is absent from available navigation"
  );
  assert.match(
    acceptanceSource,
    /await invoke\(\["goto", `\$\{consoleUrl\}\/stories`\]\)/u,
    "each vector must also prove the direct route fails before Story business behavior"
  );
  assert.match(
    acceptanceSource,
    /api\/console\/v1\/access\/users/u,
    "the unauthorized vector must use a real password operator rather than a synthetic bearer"
  );
  assert.match(
    acceptanceSource,
    /assert\.deepEqual\(limitedAccessContext\.body\.capabilities, \[\]\)/u
  );
  assert.match(
    acceptanceSource,
    /assert\.deepEqual\(\s*limitedAccessContext\.body\.managed_service_capabilities,\s*\{\}\s*\)/u,
    "actor-scoped managed-service grants must remain empty instead of being unioned globally"
  );

  const incompatibleProjection = acceptanceSource.search(
    /storyStatusRequest\(\s*connectedRequest,\s*"incompatible"/u
  );
  const incompatibleBrowser = acceptanceSource.indexOf(
    'scenario: "stories-incompatible"'
  );
  const restoredAfterIncompatible = acceptanceSource.indexOf(
    "const restoredAfterIncompatible"
  );
  const unauthorizedBrowser = acceptanceSource.indexOf(
    'scenario: "stories-unauthorized"'
  );
  const restoredForAuthorizedBrowser = acceptanceSource.indexOf(
    "const restoredForAuthorizedBrowser"
  );
  const happyBrowser = acceptanceSource.indexOf(
    "const browserEvidence = await runBrowserAcceptance"
  );
  assert.ok(
    incompatibleProjection !== -1 &&
      incompatibleProjection < incompatibleBrowser &&
      incompatibleBrowser < restoredAfterIncompatible &&
      restoredAfterIncompatible < unauthorizedBrowser &&
      unauthorizedBrowser < restoredForAuthorizedBrowser &&
      restoredForAuthorizedBrowser < happyBrowser,
    "incompatible and unauthorized browser proofs must restore connected/authorized before CRUD and Story"
  );
});

test("CI resolves pnpm from the nested examples checkout", async () => {
  const [workflow, gitignore] = await Promise.all([
    readFile(
      path.join(root, ".github", "workflows", "support-desk-acceptance.yml"),
      "utf8"
    ),
    readFile(path.join(root, ".gitignore"), "utf8"),
  ]);

  assert.match(workflow, /uses: pnpm\/action-setup@v6/u);
  assert.match(workflow, /package_json_file: examples\/package\.json/u);
  assert.match(workflow, /working-directory: examples/u);
  assert.match(gitignore, /^\.playwright-cli\/$/mu);
});

test("README publishes only the current Support Desk product lifecycle", async () => {
  const readme = await readFile(path.join(root, "README.md"), "utf8");

  assert.match(readme, /Compose → Run locally → signed Connect → Status/u);
  assert.match(readme, /pnpm acceptance:support-desk/u);
  assert.match(readme, /compatibility and regression inputs/u);
  assert.doesNotMatch(readme, /lenso system (?:plan|apply|release|runbook)/u);
  assert.doesNotMatch(readme, /lenso service release (?:plan|apply)/u);
  assert.doesNotMatch(readme, /Console Services shows/u);
});
