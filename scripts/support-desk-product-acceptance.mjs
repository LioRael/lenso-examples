#!/usr/bin/env node

import assert from "node:assert/strict";
import {
  createHash,
  generateKeyPairSync,
  randomBytes,
} from "node:crypto";
import {
  mkdtemp,
  mkdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  buildEnrollmentExchange,
  buildSystemConnectRequest,
  digestJson,
  publicEd25519KeyBase64url,
  storyStatusRequest,
  SUPPORT_TICKET_CONTRACT_DIGEST,
  SUPPORT_TICKET_OPERATION_IDS,
} from "./support-desk-product-acceptance-contract.mjs";
import {
  assertBrowserSessionDoesNotReuseServerCredential,
  assertNoForbiddenEvidence,
  extractSameOriginConsoleBearerTokens,
  extractFirstJsonObject,
  fetchJson,
  findNewPlaywrightTicketTitleControl,
  findPlaywrightTicketTitleControl,
  formatPlaywrightGatewayCandidateDiagnostics,
  freePort,
  installSignalCleanup,
  isSuccessfulPlaywrightGatewayCandidate,
  pathExists,
  parsePlaywrightRequestEntries,
  parsePlaywrightTicketTitleControls,
  prepareProviderSandbox,
  redactSameOriginSessionAuthorization,
  redactedAccessibleSnapshotDiagnostic,
  resolveExecutable,
  runCommand,
  spawnManaged,
  startEphemeralPostgres,
  startStaticServer,
  terminateManaged,
  waitFor,
  waitForExit,
  waitForHttp,
  waitForJsonOutput,
} from "./support-desk-product-acceptance-runtime.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = path.join(root, "fixtures", "acceptance", "support-desk");
const systemId = "support-desk";
const serviceId = "support-ticket";
const servicePrincipal = "svc.support-ticket";
const workloadId = "support-ticket-api";
const consolePrincipal = "svc.lenso-console";
const operatorIdentifier = "acceptance-operator@example.test";
const tenantId = "tenant-acceptance";
const storyId = "support-desk.acceptance";
const consoleKeyId = "acceptance-console-key";
const serviceKeyId = "acceptance-support-ticket-key";
const policy = {
  digest: digestJson({ policy: "support-desk-management", revision: 1 }),
  policyId: "support-desk-management",
  revision: 1,
};

const adapterStatusReasons = {
  incompatible: "Workload Control Adapter authority is incompatible",
  unavailable: "Workload Control Adapter authority is unavailable",
  unmanaged: "Workload Control Adapter authority is unmanaged",
};
const storyIncompatibleReason =
  "Module workload is incompatible with the System topology";
const storyUnauthorizedReason =
  "Current operator lacks the required Surface Entry Capability: runtime.stories.read";
const availableStoriesNavigation = /^\s*-\s+link "Stories"(?:\s|\[|$)/mu;

const log = (message) => process.stderr.write(`\n[acceptance] ${message}\n`);

const siblingRoot = (name) => path.resolve(root, "..", name);

const resolveRoot = async (environmentName, sibling) => {
  const configured = process.env[environmentName]?.trim();
  const candidate = configured ? path.resolve(configured) : siblingRoot(sibling);
  if (!(await pathExists(candidate))) {
    throw new Error(
      `${environmentName} must name a checkout of ${sibling}; tried ${candidate}`
    );
  }
  return candidate;
};

const ensureCli = async ({ cargo, cliRoot, temporaryRoot }) => {
  const configured = process.env.LENSO_CLI_BIN?.trim();
  if (configured) {
    const executable = await resolveExecutable(configured);
    if (!executable) {
      throw new Error(`LENSO_CLI_BIN does not exist: ${configured}`);
    }
    return executable;
  }
  const target = path.join(temporaryRoot, "cli-target");
  await runCommand(
    cargo,
    ["build", "--locked", "--bin", "lenso", "--target-dir", target],
    { cwd: cliRoot, label: "cli-build" }
  );
  return path.join(target, "debug", "lenso");
};

const cleanupSystemDev = async ({
  adapterToken,
  appRoot,
  label,
  lenso,
  requireCommandSuccess = true,
  sandboxProcess,
}) => {
  let commandError;
  try {
    await runCommand(lenso, ["system", "dev", "--cleanup", "--json"], {
      cwd: appRoot,
      env: adapterToken
        ? { LENSO_WORKLOAD_CONTROL_TOKEN: adapterToken }
        : undefined,
      label,
      quiet: label.endsWith("final"),
    });
  } catch (error) {
    commandError = error;
  }
  try {
    await waitForExit(sandboxProcess, 20_000);
  } catch {
    await terminateManaged(sandboxProcess);
  }
  if (commandError && requireCommandSuccess) {
    throw commandError;
  }
};

const postJson = (url, authorization, body, expectedStatuses = [200]) =>
  fetchJson(
    url,
    {
      body: JSON.stringify(body),
      headers: {
        accept: "application/json",
        authorization,
        "content-type": "application/json",
      },
      method: "POST",
    },
    expectedStatuses
  );

const getJson = (url, authorization, expectedStatuses = [200]) =>
  fetchJson(
    url,
    { headers: { accept: "application/json", authorization } },
    expectedStatuses
  );

const unsignedDigest = (signedArtifact) => {
  return signedArtifact.signature.subjectDigest;
};

const artifactContract = (artifact) => ({
  module_id: artifact.moduleId,
  module_release_digest: artifact.moduleReleaseDigest,
  locator: artifact.locator,
  digest: artifact.artifactDigest,
  format: artifact.format,
  protocol_major: 1,
  entry: artifact.entry,
  entries: artifact.entries,
  style_assets: artifact.styleAssets,
  manifest: artifact.manifest,
  requested_permissions: artifact.requestedPermissions,
});

const statusRequest = (connected, status) => {
  const request = structuredClone(connected);
  request.topology.adapters[0].workloadControl.status = status;
  request.topologyDigest = digestJson(request.topology);
  request.managementBinding.topologyDigest = request.topologyDigest;
  return request;
};

const workloadUrl = (consoleUrl) =>
  `${consoleUrl}/api/console/v1/systems/${systemId}/workloads/${serviceId}/${workloadId}`;

const waitForOperation = async (consoleUrl, operationId, authorization) =>
  waitFor(
    async () => {
      const { body } = await getJson(
        `${workloadUrl(consoleUrl)}/operations/${encodeURIComponent(operationId)}`,
        authorization
      );
      return ["succeeded", "failed", "denied"].includes(body.phase)
        ? body
        : null;
    },
    { description: `operation ${operationId}`, intervalMs: 250, timeoutMs: 30_000 }
  );

const playwrightRef = (snapshot, matcher) => {
  for (const line of snapshot.split("\n")) {
    if (matcher.test(line)) {
      const match = line.match(/\[ref=([^\]]+)\]/u);
      if (match) {
        return match[1];
      }
    }
  }
  throw new Error(`Playwright snapshot did not contain ${matcher}`);
};

const playwrightRefNear = (snapshot, anchor, matcher) => {
  const lines = snapshot.split("\n");
  const anchorIndex = lines.findIndex((line) => line.includes(anchor));
  if (anchorIndex === -1) {
    throw new Error(`Playwright snapshot did not contain ${anchor}`);
  }
  for (const line of lines.slice(anchorIndex, anchorIndex + 24)) {
    if (matcher.test(line)) {
      const match = line.match(/\[ref=([^\]]+)\]/u);
      if (match) {
        return match[1];
      }
    }
  }
  throw new Error(`Playwright snapshot near ${anchor} did not contain ${matcher}`);
};

const runStoriesAvailabilityAcceptance = async ({
  cleanup,
  consoleUrl,
  evidenceRoot,
  expectedReason,
  expectedStatus,
  operatorIdentifier,
  operatorPassword,
  playwrightCli,
  scenario,
  temporaryRoot,
}) => {
  const session = `support-desk-${scenario}-${process.pid}`;
  const environment = {
    PLAYWRIGHT_MCP_OUTPUT_DIR: path.join(
      temporaryRoot,
      "playwright-output",
      scenario
    ),
    PLAYWRIGHT_CLI_SESSION: session,
    PWTEST_DAEMON_SESSION_DIR: path.join(temporaryRoot, "playwright-daemon"),
  };
  const snapshotSensitiveValues = [operatorIdentifier, operatorPassword];
  const invoke = (args, quiet = true) =>
    runCommand(playwrightCli, ["--session", session, ...args], {
      cwd: root,
      env: environment,
      label: `browser-${scenario}-${args[0]}`,
      quiet,
    });
  const closeBrowser = () => invoke(["close"], true);
  cleanup.push(closeBrowser);
  const snapshot = async () => (await invoke(["snapshot", "--raw"])).stdout;
  const waitSnapshot = async (text) => {
    let lastSnapshot = "";
    try {
      return await waitFor(
        async () => {
          lastSnapshot = await snapshot();
          return lastSnapshot.includes(text) ? lastSnapshot : null;
        },
        {
          description: `${scenario} browser text ${text}`,
          intervalMs: 300,
          timeoutMs: 30_000,
        }
      );
    } catch (error) {
      const safeReason = redactedAccessibleSnapshotDiagnostic(
        error instanceof Error ? error.message : String(error),
        { maxChars: 2_048, sensitiveValues: snapshotSensitiveValues }
      );
      const diagnostic = redactedAccessibleSnapshotDiagnostic(lastSnapshot, {
        maxChars: 8_192,
        sensitiveValues: snapshotSensitiveValues,
      });
      throw new Error(
        `${safeReason}\nLast accessible snapshot (redacted):\n${diagnostic}`
      );
    }
  };
  try {
    await invoke(["open", `${consoleUrl}/`, "--json"], false);
    const signInPage = await waitSnapshot("Sign in");
    const identifier = playwrightRef(signInPage, /textbox "Identifier"/u);
    const password = playwrightRef(signInPage, /textbox "Password"/u);
    const signIn = playwrightRef(signInPage, /button "Sign in"/u);
    await invoke(["fill", identifier, operatorIdentifier]);
    await invoke(["fill", password, operatorPassword]);
    await invoke(["click", signIn]);

    const overviewPage = await waitSnapshot(expectedReason);
    assert.ok(
      overviewPage.includes(`Stories · ${expectedStatus}`),
      `${scenario} must show the object-level Story status on Overview`
    );
    assert.doesNotMatch(overviewPage, availableStoriesNavigation);
    await mkdir(evidenceRoot, { recursive: true });
    await invoke([
      "screenshot",
      "--filename",
      path.join(evidenceRoot, `${scenario}.png`),
      "--full-page",
    ]);

    await invoke(["goto", `${consoleUrl}/stories`]);
    const routePage = await waitSnapshot(expectedReason);
    assert.ok(
      routePage.includes(`Stories ${expectedStatus}`),
      `${scenario} must stop at the Story route boundary with the exact status`
    );
    assert.doesNotMatch(routePage, availableStoriesNavigation);
    assert.equal(
      routePage.includes(storyId),
      false,
      `${scenario} must not load Story business content`
    );
    return {
      reason: expectedReason,
      screenshot: `${scenario}.png`,
      status: expectedStatus.toLowerCase(),
    };
  } finally {
    await closeBrowser();
    const cleanupIndex = cleanup.indexOf(closeBrowser);
    if (cleanupIndex !== -1) {
      cleanup.splice(cleanupIndex, 1);
    }
  }
};

const runBrowserAcceptance = async ({
  adapterEndpoint,
  adapterToken,
  artifactSetupToken,
  cleanup,
  consoleUrl,
  evidenceRoot,
  playwrightCli,
  privateKeyFragments,
  operatorIdentifier,
  operatorPassword,
  providerBaseUrl,
  recoveryToken,
  sessionToken,
  systemPlaneToken,
  temporaryRoot,
}) => {
  const session = `support-desk-${process.pid}`;
  const environment = {
    PLAYWRIGHT_MCP_OUTPUT_DIR: path.join(temporaryRoot, "playwright-output"),
    PLAYWRIGHT_CLI_SESSION: session,
    PWTEST_DAEMON_SESSION_DIR: path.join(temporaryRoot, "playwright-daemon"),
  };
  const snapshotSensitiveValues = [
    new URL(adapterEndpoint).origin,
    adapterToken,
    artifactSetupToken,
    operatorIdentifier,
    operatorPassword,
    new URL(providerBaseUrl).origin,
    recoveryToken,
    sessionToken,
    systemPlaneToken,
    ...privateKeyFragments,
  ];
  const serverOwnedCredentials = [
    { label: "Local Adapter bearer token", value: adapterToken },
    { label: "artifact setup token", value: artifactSetupToken },
    { label: "recovery token", value: recoveryToken },
    { label: "operator session token", value: sessionToken },
    { label: "System Plane bearer token", value: systemPlaneToken },
    ...privateKeyFragments.map((value, index) => ({
      label: `private key fragment ${index + 1}`,
      value,
    })),
  ];
  const consoleOrigin = new URL(consoleUrl).origin;
  const surfaceGatewayPath = `/api/console/v1/services/${encodeURIComponent(
    serviceId
  )}/surface-gateway`;
  const surfaceGatewayRequestFilter = `${surfaceGatewayPath}(?:\\?.*)?$`;
  const isPasswordLogin = (entry) => {
    const url = new URL(entry.url);
    return (
      entry.method === "POST" &&
      url.origin === consoleOrigin &&
      /\/auth\/password\/login$/u.test(url.pathname)
    );
  };
  const invoke = (args, quiet = true) =>
    runCommand(playwrightCli, ["--session", session, ...args], {
      cwd: root,
      env: environment,
      label: `browser-${args[0]}`,
      quiet,
    });
  const closeBrowser = () => invoke(["close"], true);
  cleanup.push(closeBrowser);
  const snapshot = async () => (await invoke(["snapshot", "--raw"])).stdout;
  const snapshotFailure = (error, lastSnapshot) => {
    const safeReason = redactedAccessibleSnapshotDiagnostic(
      error instanceof Error ? error.message : String(error),
      { maxChars: 2_048, sensitiveValues: snapshotSensitiveValues }
    );
    const diagnostic = redactedAccessibleSnapshotDiagnostic(lastSnapshot, {
      maxChars: 8_192,
      sensitiveValues: snapshotSensitiveValues,
    });
    return new Error(
      `${safeReason}\nLast accessible snapshot (redacted):\n${diagnostic}`
    );
  };
  const waitForAccessibleSnapshot = async (description, select) => {
    let lastSnapshot = "";
    try {
      return await waitFor(
        async () => {
          lastSnapshot = await snapshot();
          return select(lastSnapshot);
        },
        {
          description,
          intervalMs: 300,
          timeoutMs: 30_000,
        }
      );
    } catch (error) {
      throw snapshotFailure(error, lastSnapshot);
    }
  };
  const waitSnapshot = (text) =>
    waitForAccessibleSnapshot(`browser text ${text}`, (value) =>
      value.includes(text) ? value : null
    );
  const waitForTicketTitleControl = ({
    description,
    existingTicketIds,
    ticketId,
    title,
  }) =>
    waitForAccessibleSnapshot(description, (value) => {
      const control = ticketId
        ? findPlaywrightTicketTitleControl(value, { ticketId, title })
        : findNewPlaywrightTicketTitleControl(value, {
            existingTicketIds,
            title,
          });
      return control ? { control, page: value } : null;
    });
  let surfaceGatewayRequestListState = "not-read";
  const browserRequestEntries = async () => {
    const output = (
      await invoke([
        "requests",
        "--filter",
        surfaceGatewayRequestFilter,
        "--raw",
      ])
    ).stdout;
    const entries = parsePlaywrightRequestEntries(output);
    surfaceGatewayRequestListState = entries.length
      ? `parsed:${entries.length}`
      : output.trim()
        ? "unparsed-nonempty"
        : "empty";
    return entries;
  };
  const browserRequestIndexes = async () =>
    new Set((await browserRequestEntries()).map(({ index }) => index));
  const collectBrowserNetworkEvidence = async () => {
    const requests = (await invoke(["requests", "--raw"])).stdout;
    const entries = parsePlaywrightRequestEntries(requests);
    assert.ok(entries.length > 0, "browser network evidence must not be empty");
    const evidence = [requests];
    const requestRecords = [];
    for (const entry of entries) {
      const details = (await invoke(["request", entry.index, "--raw"])).stdout;
      const privateAuthExchange = isPasswordLogin(entry);
      const record = { ...entry, details, privateAuthExchange };
      requestRecords.push(record);
      // The password and newly issued browser session are private parts of the
      // exact same-origin login exchange. Do not collect either body or the
      // combined request details, which also include the Set-Cookie response.
      if (privateAuthExchange) {
        continue;
      }
      record.requestBody = (
        await invoke(["request-body", entry.index, "--raw"])
      ).stdout;
      record.responseBody = (
        await invoke(["response-body", entry.index, "--raw"])
      ).stdout;
    }
    const browserSessionTokens = extractSameOriginConsoleBearerTokens(
      requestRecords,
      { consoleOrigin }
    );
    assertBrowserSessionDoesNotReuseServerCredential(
      browserSessionTokens,
      serverOwnedCredentials
    );
    for (const record of requestRecords) {
      if (record.privateAuthExchange) {
        continue;
      }
      evidence.push(
        redactSameOriginSessionAuthorization(record, {
          consoleOrigin,
          sessionTokens: browserSessionTokens,
        }),
        record.requestBody,
        record.responseBody
      );
    }
    return { browserSessionTokens, entries, evidence, requestRecords };
  };
  const readBrowserJsonBody = async (command, index) => {
    try {
      const output = (await invoke([command, index, "--raw"])).stdout;
      if (!output.trim()) {
        return { state: "unavailable", value: null };
      }
      const value = extractFirstJsonObject(output);
      return value
        ? { state: "json", value }
        : { state: "non-json", value: null };
    } catch {
      return { state: "command-error", value: null };
    }
  };
  const waitForSurfaceGatewayResponse = async ({
    afterIndexes,
    operationId,
  }) => {
    let lastCandidates = [];
    try {
      return await waitFor(
        async () => {
          const entries = await browserRequestEntries();
          const candidates = [];
          for (const entry of entries) {
            if (afterIndexes.has(entry.index) || entry.method !== "POST") {
              continue;
            }
            const url = new URL(entry.url);
            if (
              url.origin !== consoleOrigin ||
              url.pathname !== surfaceGatewayPath
            ) {
              continue;
            }
            const requestBody = await readBrowserJsonBody(
              "request-body",
              entry.index
            );
            const responseBody = await readBrowserJsonBody(
              "response-body",
              entry.index
            );
            const candidate = {
              entry,
              request: requestBody.value,
              requestBodyState: requestBody.state,
              response: responseBody.value,
              responseBodyState: responseBody.state,
            };
            candidates.push(candidate);
            if (
              isSuccessfulPlaywrightGatewayCandidate(candidate, {
                consoleOrigin,
                operationId,
                surfaceGatewayPath,
              })
            ) {
              return {
                entry,
                request: requestBody.value,
                response: responseBody.value,
              };
            }
          }
          lastCandidates = candidates;
          return null;
        },
        {
          description: `browser ${operationId} Surface Gateway response`,
          intervalMs: 300,
          timeoutMs: 30_000,
        }
      );
    } catch {
      throw new Error(
        `browser ${operationId} Surface Gateway response did not become ready within 30000ms\nSurface Gateway request list: ${surfaceGatewayRequestListState}\nSurface Gateway candidates (metadata only):\n${formatPlaywrightGatewayCandidateDiagnostics(
          lastCandidates
        )}`
      );
    }
  };
  try {
    await invoke(["open", `${consoleUrl}/support/tickets`, "--json"], false);
    let page = await waitSnapshot("Sign in");
    const identifier = playwrightRef(page, /textbox "Identifier"/u);
    const password = playwrightRef(page, /textbox "Password"/u);
    const signIn = playwrightRef(page, /button "Sign in"/u);
    await invoke(["fill", identifier, operatorIdentifier]);
    await invoke(["fill", password, operatorPassword]);
    const initialListRequestIndexes = await browserRequestIndexes();
    await invoke(["click", signIn]);
    const listExchange = await waitForSurfaceGatewayResponse({
      afterIndexes: initialListRequestIndexes,
      operationId: SUPPORT_TICKET_OPERATION_IDS.list,
    });
    assert.equal(listExchange.entry.status, "200");
    assert.equal(
      listExchange.response.operationId,
      SUPPORT_TICKET_OPERATION_IDS.list
    );
    assert.ok(Array.isArray(listExchange.response.output.records));
    const seededTicket = listExchange.response.output.records.find(
      (ticket) => ticket.id === "ticket_1"
    );
    assert.ok(seededTicket, "initial browser list must include seeded ticket_1");
    page = await waitSnapshot("New ticket");
    const seededRow = await waitForTicketTitleControl({
      description: "rendered seeded browser ticket row",
      ticketId: seededTicket.id,
      title: seededTicket.title,
    });
    assert.equal(seededRow.control.ticketId, seededTicket.id);
    assert.equal(seededRow.control.value, seededTicket.title);
    page = seededRow.page;
    const title = playwrightRef(page, /textbox "New ticket"/u);
    const priority = playwrightRef(page, /combobox "Priority"/u);
    const create = playwrightRef(page, /button "Create ticket"/u);
    const createdTitle = "Browser acceptance ticket";
    const updatedTitle = "Browser acceptance ticket updated";
    const existingTicketIds = parsePlaywrightTicketTitleControls(page).map(
      ({ ticketId }) => ticketId
    );
    await invoke(["fill", title, createdTitle]);
    await invoke(["select", priority, "high"]);
    const createRequestIndexes = await browserRequestIndexes();
    await invoke(["click", create]);
    const createExchange = await waitForSurfaceGatewayResponse({
      afterIndexes: createRequestIndexes,
      operationId: SUPPORT_TICKET_OPERATION_IDS.create,
    });
    const createdTicket = createExchange.response.output.ticket;
    assert.ok(createdTicket?.id);
    assert.equal(createdTicket.title, createdTitle);
    assert.equal(createdTicket.priority, "high");
    assert.equal(existingTicketIds.includes(createdTicket.id), false);
    const createdRow = await waitForTicketTitleControl({
      description: "newly rendered browser ticket row",
      existingTicketIds,
      title: createdTitle,
    });
    assert.equal(createdRow.control.ticketId, createdTicket.id);
    page = createdRow.page;
    const ticketTitle = createdRow.control.ref;
    const save = playwrightRefNear(
      page,
      createdRow.control.line,
      /button "Save"/u
    );
    await invoke(["fill", ticketTitle, updatedTitle]);
    const updateRequestIndexes = await browserRequestIndexes();
    await invoke(["click", save]);
    const updateExchange = await waitForSurfaceGatewayResponse({
      afterIndexes: updateRequestIndexes,
      operationId: SUPPORT_TICKET_OPERATION_IDS.update,
    });
    assert.equal(updateExchange.response.output.ticket.id, createdTicket.id);
    assert.equal(updateExchange.response.output.ticket.title, updatedTitle);
    const updatedRow = await waitForTicketTitleControl({
      description: "rendered updated browser ticket row",
      ticketId: createdTicket.id,
      title: updatedTitle,
    });
    page = updatedRow.page;
    const close = playwrightRefNear(
      page,
      updatedRow.control.line,
      /button "Close"/u
    );
    const closeRequestIndexes = await browserRequestIndexes();
    await invoke(["click", close]);
    const closeExchange = await waitForSurfaceGatewayResponse({
      afterIndexes: closeRequestIndexes,
      operationId: SUPPORT_TICKET_OPERATION_IDS.close,
    });
    assert.equal(closeExchange.response.output.ticket.id, createdTicket.id);
    assert.equal(closeExchange.response.output.ticket.status, "closed");
    page = await waitForAccessibleSnapshot(
      "rendered closed browser ticket row",
      (value) => {
        const control = findPlaywrightTicketTitleControl(value, {
          ticketId: createdTicket.id,
          title: updatedTitle,
        });
        if (!control) {
          return null;
        }
        const lines = value.split("\n");
        const controlIndex = lines.indexOf(control.line);
        return lines
          .slice(controlIndex, controlIndex + 24)
          .some((line) => /\bclosed\b/iu.test(line))
          ? value
          : null;
      }
    );
    await mkdir(evidenceRoot, { recursive: true });
    await invoke([
      "screenshot",
      "--filename",
      path.join(evidenceRoot, "support-tickets.png"),
      "--full-page",
    ]);
    const supportNetworkEvidence = await collectBrowserNetworkEvidence();
    assert.equal(
      supportNetworkEvidence.entries.filter(isPasswordLogin).length,
      1,
      "the browser must perform exactly one same-origin password login"
    );
    assert.equal(
      supportNetworkEvidence.browserSessionTokens.length,
      1,
      "password login must produce an authenticated same-origin Console API request"
    );
    await invoke(["goto", `${consoleUrl}/stories`]);
    const storyPage = await waitSnapshot(storyId);
    await invoke([
      "screenshot",
      "--filename",
      path.join(evidenceRoot, "stories.png"),
      "--full-page",
    ]);
    const storyNetworkEvidence = await collectBrowserNetworkEvidence();
    const browserSessionTokens = [
      ...new Set([
        ...supportNetworkEvidence.browserSessionTokens,
        ...storyNetworkEvidence.browserSessionTokens,
      ]),
    ];
    assert.equal(
      browserSessionTokens.length,
      1,
      "the browser must use one ordinary operator session"
    );
    assert.equal(
      browserSessionTokens.includes(sessionToken),
      false,
      "the browser must not reuse the installation-authority bootstrap session"
    );
    const publicEvidence = `${page}\n${storyPage}\n${[
      ...supportNetworkEvidence.evidence,
      ...storyNetworkEvidence.evidence,
    ].join("\n")}`;
    const forbiddenEvidence = [
      { label: "Local Adapter origin", value: new URL(adapterEndpoint).origin },
      { label: "operator password", value: operatorPassword },
      { label: "managed Provider origin", value: new URL(providerBaseUrl).origin },
      ...serverOwnedCredentials,
      ...browserSessionTokens.map((value, index) => ({
        label: `browser operator session token ${index + 1}`,
        value,
      })),
    ];
    assertNoForbiddenEvidence(publicEvidence, forbiddenEvidence);
    assert.match(storyPage, /Stories/u);
    return {
      browserTicket: "closed",
      browserTicketId: createdTicket.id,
      crudProof: "surface-gateway-responses-and-rendered-state",
      screenshots: ["support-tickets.png", "stories.png"],
      storyArtifact: "loaded",
    };
  } finally {
    await closeBrowser();
    const cleanupIndex = cleanup.indexOf(closeBrowser);
    if (cleanupIndex !== -1) {
      cleanup.splice(cleanupIndex, 1);
    }
  }
};

const main = async () => {
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "lenso-support-desk-"));
  const expectedActorFile = path.join(temporaryRoot, "console-actor");
  const observedContextFile = path.join(temporaryRoot, "surface-context.jsonl");
  await writeFile(observedContextFile, "", { mode: 0o600 });
  const keep = process.env.LENSO_ACCEPTANCE_KEEP_TEMP === "1";
  const cleanup = [];
  let sandboxProcess;
  let appRoot;
  let lenso;
  let cleanupPromise;
  const cleanupAll = () => {
    cleanupPromise ??= (async () => {
      const errors = [];
      if (sandboxProcess && appRoot && lenso) {
        await cleanupSystemDev({
          appRoot,
          label: "system-cleanup-final",
          lenso,
          requireCommandSuccess: false,
          sandboxProcess,
        }).catch((error) => errors.push(error));
      }
      for (const dispose of cleanup.reverse()) {
        await dispose().catch((error) => errors.push(error));
      }
      if (!keep) {
        await rm(temporaryRoot, { force: true, recursive: true }).catch((error) =>
          errors.push(error)
        );
      } else {
        log(`kept acceptance evidence at ${temporaryRoot}`);
      }
      if (errors.length > 0) {
        throw new AggregateError(errors, "acceptance cleanup failed");
      }
    })();
    return cleanupPromise;
  };
  const removeSignalCleanup = installSignalCleanup(cleanupAll);
  try {
    const frameworkRoot = await resolveRoot("LENSO_FRAMEWORK_ROOT", "lenso");
    const cliRoot = await resolveRoot("LENSO_CLI_ROOT", "lenso-cli");
    const consoleRoot = await resolveRoot("LENSO_CONSOLE_ROOT", "lenso-console");
    const surfaceContractDocument = await readFile(
      path.join(
        root,
        "examples",
        "support-ticket",
        "contracts",
        "support-ticket-business-api.v1.json"
      ),
      "utf8"
    );
    assert.equal(
      `sha256:${createHash("sha256")
        .update(surfaceContractDocument)
        .digest("hex")}`,
      SUPPORT_TICKET_CONTRACT_DIGEST,
      "Support Ticket implementation and Console Surface must share one exact Business API contract"
    );
    const pnpm = await resolveExecutable("pnpm");
    if (!pnpm) {
      throw new Error("pnpm is required");
    }
    const cargo = await resolveExecutable("cargo");
    if (!cargo) {
      throw new Error(
        "cargo is required to migrate and start the Console Service"
      );
    }
    lenso = await ensureCli({ cargo, cliRoot, temporaryRoot });

    log("building the exact linked Provider Core and Console surfaces");
    const typescriptSdkRoot = path.join(frameworkRoot, "sdk", "typescript");
    await runCommand(pnpm, ["install", "--frozen-lockfile"], {
      cwd: typescriptSdkRoot,
      label: "service-kit-install",
    });
    await runCommand(pnpm, ["--dir", typescriptSdkRoot, "build"], {
      label: "service-kit-build",
    });
    await runCommand(pnpm, ["install", "--frozen-lockfile"], {
      cwd: consoleRoot,
      label: "console-install",
    });

    const playwrightCli = path.join(root, "node_modules", ".bin", "playwright-cli");
    if (!(await pathExists(playwrightCli))) {
      throw new Error("Run pnpm install before pnpm acceptance:support-desk");
    }

    appRoot = path.join(temporaryRoot, "support-desk");
    log("materializing the exact App Composition through lenso app compose");
    await runCommand(
      lenso,
      [
        "app",
        "compose",
        appRoot,
        "--blueprint",
        "support-desk",
        "--pack",
        path.join(fixtureRoot, "capability"),
        "--implementation",
        "support-api=linked",
        "--implementation",
        "notification-worker=linked",
        "--implementation",
        "lenso/platform-story=linked",
        "--apply",
      ],
      { cwd: root, label: "app-compose" }
    );
    const composition = JSON.parse(
      await readFile(path.join(appRoot, "lenso.app.json"), "utf8")
    );
    const expectedComposition = JSON.parse(
      await readFile(path.join(fixtureRoot, "lenso.app.json"), "utf8")
    );
    assert.deepEqual(composition, expectedComposition);

    const providerRoot = path.join(temporaryRoot, "support-ticket-provider");
    await prepareProviderSandbox({ frameworkRoot, sourceRoot: root, targetRoot: providerRoot });
    const providerPort = await freePort();
    const workspaceFile = path.join(appRoot, "lenso.workspace.json");
    await runCommand(
      lenso,
      ["service", "workspace", "init", "--workspace-file", workspaceFile, "--force"],
      { cwd: appRoot, label: "workspace-init" }
    );
    await runCommand(
      lenso,
      [
        "service",
        "workspace",
        "add",
        serviceId,
        "--cwd",
        providerRoot,
        "--lang",
        "ts",
        "--command",
        "node src/server.ts",
        "--ready-url",
        `http://127.0.0.1:${providerPort}/lenso/service/v1/status`,
        "--module",
        "support-ticket",
        "--manifest",
        "lenso.service.json",
        "--workspace-file",
        workspaceFile,
      ],
      { cwd: appRoot, label: "workspace-add" }
    );

    const consoleKeys = generateKeyPairSync("ed25519");
    const serviceKeys = generateKeyPairSync("ed25519");
    const nowUnixMs = Date.now();
    const exchange = buildEnrollmentExchange({
      consoleKeyId,
      consolePrivateKey: consoleKeys.privateKey,
      consoleServicePrincipal: consolePrincipal,
      expiresAtUnixMs: nowUnixMs + 60 * 60_000,
      issuedAtUnixMs: nowUnixMs,
      managedServiceId: serviceId,
      managedServicePrincipal: servicePrincipal,
      managedServiceRevision: "1",
      policy: {
        policyDigest: policy.digest,
        policyId: policy.policyId,
        policyRevision: String(policy.revision),
      },
      serviceKeyId,
      servicePrivateKey: serviceKeys.privateKey,
      systemId,
    });
    const receiptDigest = unsignedDigest(exchange.receipt);
    const systemPlaneToken = randomBytes(32).toString("base64url");
    const adapterToken = randomBytes(32).toString("base64url");
    const providerBaseUrl = `http://127.0.0.1:${providerPort}/lenso/service/v1`;

    log("starting lenso system dev and its Local Control Adapter");
    sandboxProcess = spawnManaged(lenso, ["system", "dev", "--json"], {
      cwd: appRoot,
      env: {
        LENSO_ACCEPTANCE_EXPECTED_ACTOR: "",
        LENSO_ACCEPTANCE_EXPECTED_ACTOR_FILE: expectedActorFile,
        LENSO_ACCEPTANCE_EXPECTED_AUTHORITY_DIGEST: receiptDigest,
        LENSO_ACCEPTANCE_OBSERVED_CONTEXT_FILE: observedContextFile,
        LENSO_ACCEPTANCE_EXPECTED_STORY_ID: storyId,
        LENSO_ACCEPTANCE_EXPECTED_TENANT_ID: tenantId,
        LENSO_SERVICE_PRINCIPAL: servicePrincipal,
        LENSO_SERVICE_REVISION: "1",
        LENSO_SUPPORT_DESK_ACCEPTANCE: "1",
        LENSO_SYSTEM_PLANE_BEARER_TOKEN: systemPlaneToken,
        LENSO_WORKLOAD_CONTROL_TOKEN: adapterToken,
        PORT: String(providerPort),
      },
      label: "system-dev",
    });
    const adapterState = await waitForJsonOutput(
      sandboxProcess,
      "lenso system dev",
      90_000
    );
    assert.equal(adapterState.phase, "ready");
    assert.equal(adapterState.schema, "lenso.local-control-adapter-state.v2");
    assert.equal(adapterState.protocol, "lenso.local-control-adapter.v1");
    assert.equal(adapterState.appId, systemId);
    assert.equal(adapterState.adapterOwnershipToken, undefined);
    await waitForHttp(`${providerBaseUrl}/status`, 30_000);

    const artifactRoot = path.join(temporaryRoot, "module-artifacts");
    await mkdir(artifactRoot, { recursive: true });
    const artifactServer = await startStaticServer(artifactRoot);
    cleanup.push(() => artifactServer.close());
    const platformArtifactRoot = path.join(artifactRoot, "platform");
    const supportArtifactRoot = path.join(artifactRoot, "support-ticket");
    const systemRegistryDigest = digestJson({ moduleId: "lenso/system-registry", revision: 1 });
    const releaseDigests = Object.fromEntries(
      composition.modules
        .filter((module) =>
          ["lenso/platform-story", "support/tickets"].includes(module.moduleId)
        )
        .map((module) => [module.moduleId, module.release.contentDigest])
    );
    releaseDigests["lenso/system-registry"] = systemRegistryDigest;
    await runCommand(pnpm, ["build:module-artifacts"], {
      cwd: consoleRoot,
      env: {
        LENSO_CONSOLE_MODULE_ARTIFACT_BASE_URL: `${artifactServer.baseUrl}/platform`,
        LENSO_CONSOLE_MODULE_ARTIFACT_DIR: platformArtifactRoot,
        LENSO_MODULE_RELEASE_DIGESTS: JSON.stringify(releaseDigests),
      },
      label: "platform-console-artifacts",
    });
    await runCommand(pnpm, ["build:console-artifact:support-ticket"], {
      cwd: root,
      env: {
        LENSO_CONSOLE_MODULE_ARTIFACT_BASE_URL: `${artifactServer.baseUrl}/support-ticket`,
        LENSO_SUPPORT_TICKET_CONSOLE_ARTIFACT_DIR: supportArtifactRoot,
        LENSO_SUPPORT_TICKET_MODULE_RELEASE_DIGEST:
          releaseDigests["support/tickets"],
      },
      label: "support-ticket-console-artifact",
    });
    const platformArtifactIndex = JSON.parse(
      await readFile(path.join(platformArtifactRoot, "artifact-index.json"), "utf8")
    );
    const supportArtifactIndex = JSON.parse(
      await readFile(path.join(supportArtifactRoot, "artifact-index.json"), "utf8")
    );
    const artifacts = [
      ...platformArtifactIndex.artifacts,
      ...supportArtifactIndex.artifacts,
    ];
    const artifact = (moduleId) => {
      const value = artifacts.find((entry) => entry.moduleId === moduleId);
      assert.ok(value, `missing ${moduleId} artifact`);
      return value;
    };
    const supportArtifact = artifact("support/tickets");
    const storyArtifact = artifact("lenso/platform-story");

    const connectedRequest = buildSystemConnectRequest({
      adapterState,
      artifacts: {
        story: storyArtifact,
        supportTicket: supportArtifact,
      },
      composition,
      policy,
      supportTicket: {
        serviceId,
        servicePrincipal,
        surfaceContractDocument,
        workloadId,
      },
    });

    let databaseUrl = process.env.LENSO_ACCEPTANCE_DATABASE_URL?.trim();
    if (databaseUrl && !new URL(databaseUrl).pathname.includes("acceptance")) {
      throw new Error(
        "LENSO_ACCEPTANCE_DATABASE_URL must name a disposable acceptance database"
      );
    }
    if (!databaseUrl) {
      const postgres = await startEphemeralPostgres(temporaryRoot);
      databaseUrl = postgres.url;
      cleanup.push(() => postgres.dispose());
    }

    const consolePort = await freePort();
    const consoleUrl = `http://127.0.0.1:${consolePort}`;
    const consoleArtifactRoot = path.join(temporaryRoot, "console-artifact-store");
    const operatorPassword = `Lenso-Acceptance-${randomBytes(18).toString(
      "base64url"
    )}!9aA`;
    const recoveryToken = randomBytes(32).toString("base64url");
    const enrollmentTrust = {
      consoleAuthorityKeys: [
        {
          consoleServicePrincipal: consolePrincipal,
          keyId: consoleKeyId,
          publicKeyBase64url: publicEd25519KeyBase64url(consoleKeys.publicKey),
        },
      ],
      managedServiceKeys: [
        {
          baseUrl: providerBaseUrl,
          keyId: serviceKeyId,
          managedServiceId: serviceId,
          managedServicePrincipal: servicePrincipal,
          publicKeyBase64url: publicEd25519KeyBase64url(serviceKeys.publicKey),
          systemId,
          systemPlaneBearerToken: systemPlaneToken,
        },
      ],
    };
    const consoleEnvironment = {
      CARGO_TARGET_DIR:
        process.env.LENSO_CONSOLE_CARGO_TARGET_DIR ??
        path.join(temporaryRoot, "console-target"),
      CONSOLE_ARTIFACT_ROOT: consoleArtifactRoot,
      DATABASE_URL: databaseUrl,
      HTTP_PORT: String(consolePort),
      LENSO_COMPOSITION_PROFILE: "core",
      LENSO_CONSOLE_WORKLOAD_CONTROL_ADAPTERS: JSON.stringify([
        {
          adapterId: adapterState.adapterId,
          baseUrl: adapterState.endpoint,
          bearerToken: adapterToken,
          systemId,
        },
      ]),
      LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST:
        JSON.stringify(enrollmentTrust),
      LENSO_MODULE_PLATFORM_STORY_ENABLED: "false",
      SERVICE_NAME: "lenso-console",
      // Delete any caller shell override so the built browser authenticates
      // through Console Auth and reads the password-login session at runtime.
      VITE_API_AUTH_TOKEN: undefined,
    };

    log("migrating and bootstrapping a real Console operator session");
    await runCommand(pnpm, ["service:migrate"], {
      cwd: consoleRoot,
      env: { ...consoleEnvironment, CONSOLE_RECOVERY_MODE: "normal" },
      label: "console-migrate",
    });
    const recoveryProcess = spawnManaged(pnpm, ["service:serve"], {
      cwd: consoleRoot,
      env: {
        ...consoleEnvironment,
        CONSOLE_BOOTSTRAP_RECOVERY_TOKEN: recoveryToken,
        CONSOLE_RECOVERY_MODE: "restore",
      },
      label: "console-recovery",
    });
    cleanup.push(() => terminateManaged(recoveryProcess));
    await waitForHttp(`${consoleUrl}/health/ready`, 180_000);
    const bootstrap = await fetchJson(
      `${consoleUrl}/bootstrap/v1/recovery`,
      {
        body: JSON.stringify({
          identifier: operatorIdentifier,
          password: operatorPassword,
        }),
        headers: {
          accept: "application/json",
          "content-type": "application/json",
          "x-lenso-console-recovery-token": recoveryToken,
        },
        method: "POST",
      },
      [200]
    );
    assert.equal(bootstrap.body.authority, "console.superadmin");
    assert.match(bootstrap.body.userId, /^usr_/u);
    assert.ok(bootstrap.body.token);
    const operatorActor = bootstrap.body.userId;
    const operatorAuthorization = `Bearer ${bootstrap.body.token}`;
    const artifactSetupToken = `dev-user:acceptance-artifact-${randomBytes(8).toString("hex")}:console.artifacts.manage`;
    const artifactSetupAuthorization = `Bearer ${artifactSetupToken}`;
    await writeFile(expectedActorFile, `${operatorActor}\n`, { mode: 0o600 });
    await terminateManaged(recoveryProcess);

    log("starting the Console in normal mode without a browser bearer");
    const consoleProcess = spawnManaged(pnpm, ["service:serve"], {
      cwd: consoleRoot,
      env: {
        ...consoleEnvironment,
        CONSOLE_BOOTSTRAP_RECOVERY_TOKEN: "",
        CONSOLE_RECOVERY_MODE: "normal",
      },
      label: "console",
    });
    cleanup.push(() => terminateManaged(consoleProcess));
    await waitForHttp(`${consoleUrl}/health/ready`, 180_000);
    const consoleAuthority = await fetchJson(`${consoleUrl}/health/authority`);
    assert.equal(consoleAuthority.body.workloadMode, "normal");

    log("reconciling reviewed console_ui_esm artifacts through the Console API");
    await postJson(
      `${consoleUrl}/api/console/v1/artifacts/reconcile`,
      artifactSetupAuthorization,
      {
        artifacts: artifactIndex.artifacts.map(artifactContract),
        candidate_lock_digest: composition.contentDigest,
        console_service_id: "lenso-console",
        effect_id: "support-desk-acceptance-console-composition",
        kind: "console_composition",
        theme_bundles: [],
      }
    );

    log("registering the bilateral enrollment and connecting the exact System");
    const enrollment = await postJson(
      `${consoleUrl}/api/console/v1/enrollment-receipts`,
      operatorAuthorization,
      { baseUrl: providerBaseUrl, ...exchange },
      [201]
    );
    assert.equal(enrollment.body.receiptDigest, receiptDigest);
    const connected = await postJson(
      `${consoleUrl}/api/console/v1/system/connect`,
      operatorAuthorization,
      connectedRequest
    );
    assert.equal(connected.body.status, "connected");
    assert.equal(connected.body.reason, null);

    for (const expectedStatus of ["unavailable", "incompatible", "unmanaged"]) {
      const projected = await postJson(
        `${consoleUrl}/api/console/v1/system/connect`,
        operatorAuthorization,
        statusRequest(connectedRequest, expectedStatus)
      );
      assert.equal(projected.body.status, expectedStatus);
      assert.equal(projected.body.reason, adapterStatusReasons[expectedStatus]);
      const adapterService = projected.body.services.find(
        (service) => service.serviceId === "lenso-local-control-adapter"
      );
      assert.equal(adapterService.status, expectedStatus);
      assert.equal(adapterService.reason, adapterStatusReasons[expectedStatus]);
      assert.equal(
        projected.body.adapters[0].workloadControl.status,
        expectedStatus
      );
      const persisted = await getJson(
        `${consoleUrl}/api/console/v1/system`,
        operatorAuthorization
      );
      assert.equal(persisted.body.status, expectedStatus);
      assert.equal(persisted.body.reason, adapterStatusReasons[expectedStatus]);
    }
    const restored = await postJson(
      `${consoleUrl}/api/console/v1/system/connect`,
      operatorAuthorization,
      connectedRequest
    );
    assert.equal(restored.body.status, "connected");

    log("proving the incompatible Story Surface in a real browser session");
    const incompatibleStoryRequest = storyStatusRequest(
      connectedRequest,
      "incompatible"
    );
    const incompatibleStoryProjection = await postJson(
      `${consoleUrl}/api/console/v1/system/connect`,
      operatorAuthorization,
      incompatibleStoryRequest
    );
    const incompatibleStoryModule = incompatibleStoryProjection.body.modules.find(
      (module) => module.moduleId === "lenso/platform-story"
    );
    assert.ok(incompatibleStoryModule);
    assert.equal(incompatibleStoryModule.status, "incompatible");
    assert.equal(incompatibleStoryModule.reason, storyIncompatibleReason);
    const incompatibleStoryBrowser = await runStoriesAvailabilityAcceptance({
      cleanup,
      consoleUrl,
      evidenceRoot: path.join(temporaryRoot, "evidence"),
      expectedReason: storyIncompatibleReason,
      expectedStatus: "Incompatible",
      operatorIdentifier,
      operatorPassword,
      playwrightCli,
      scenario: "stories-incompatible",
      temporaryRoot,
    });
    const restoredAfterIncompatible = await postJson(
      `${consoleUrl}/api/console/v1/system/connect`,
      operatorAuthorization,
      connectedRequest
    );
    assert.equal(restoredAfterIncompatible.body.status, "connected");
    assert.equal(
      restoredAfterIncompatible.body.modules.find(
        (module) => module.moduleId === "lenso/platform-story"
      )?.status,
      "connected"
    );

    log("proving the unauthorized Story Surface with a limited password user");
    const limitedOperatorIdentifier = `stories-limited-${randomBytes(8).toString("hex")}@example.test`;
    const limitedOperatorPassword = randomBytes(24).toString("base64url");
    const limitedOperator = await postJson(
      `${consoleUrl}/api/console/v1/access/users`,
      operatorAuthorization,
      {
        identifier: limitedOperatorIdentifier,
        password: limitedOperatorPassword,
      }
    );
    assert.match(limitedOperator.body.user.id, /^usr_/u);
    assert.ok(limitedOperator.body.session.token);
    const limitedAuthorization = `Bearer ${limitedOperator.body.session.token}`;
    const limitedAccessContext = await getJson(
      `${consoleUrl}/api/console/v1/access/context`,
      limitedAuthorization
    );
    assert.equal(
      limitedAccessContext.body.actor.user_id,
      limitedOperator.body.user.id
    );
    assert.deepEqual(limitedAccessContext.body.capabilities, []);
    assert.deepEqual(
      limitedAccessContext.body.managed_service_capabilities,
      {}
    );
    const deniedSystem = await getJson(
      `${consoleUrl}/api/console/v1/system`,
      limitedAuthorization,
      [403]
    );
    assert.equal(deniedSystem.status, 403);
    const unauthorizedStoryBrowser = await runStoriesAvailabilityAcceptance({
      cleanup,
      consoleUrl,
      evidenceRoot: path.join(temporaryRoot, "evidence"),
      expectedReason: storyUnauthorizedReason,
      expectedStatus: "Unavailable",
      operatorIdentifier: limitedOperatorIdentifier,
      operatorPassword: limitedOperatorPassword,
      playwrightCli,
      scenario: "stories-unauthorized",
      temporaryRoot,
    });
    const restoredForAuthorizedBrowser = await postJson(
      `${consoleUrl}/api/console/v1/system/connect`,
      operatorAuthorization,
      connectedRequest
    );
    assert.equal(restoredForAuthorizedBrowser.body.status, "connected");
    const authorizedAccessContext = await getJson(
      `${consoleUrl}/api/console/v1/access/context`,
      operatorAuthorization
    );
    assert.deepEqual(authorizedAccessContext.body.capabilities, ["*"]);

    log("running the generated Support Ticket client through Surface Gateway");
    const managedContext = {
      callerModuleId: "support/tickets",
      capabilities: [
        "support_ticket.tickets.read",
        "support_ticket.tickets.write",
      ],
      delegatedActorSubject: operatorActor,
      delegatedAuthorityDigest: receiptDigest,
      // ManagedServiceContext v1 still carries this compatibility field. Bind
      // it to the System identity so the scenario has no environment choice.
      environmentId: systemId,
      serviceId,
      systemId,
      targetServicePrincipal: servicePrincipal,
    };
    const observedBeforeGeneratedClient = (await readFile(
      observedContextFile,
      "utf8"
    ))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    const generatedClientRun = await runCommand(
      pnpm,
      [
        "exec",
        "tsx",
        path.join(root, "scripts", "support-desk-generated-client-acceptance.mjs"),
      ],
      {
        cwd: consoleRoot,
        env: {
          LENSO_ACCEPTANCE_CONSOLE_AUTHORIZATION: operatorAuthorization,
          LENSO_ACCEPTANCE_CONSOLE_URL: consoleUrl,
          LENSO_ACCEPTANCE_GENERATED_CLIENT: path.join(
            root,
            "examples",
            "support-ticket-console",
            "src",
            "business-api.ts"
          ),
          LENSO_ACCEPTANCE_MANAGED_CONTEXT: JSON.stringify(managedContext),
          LENSO_ACCEPTANCE_MODULE_IDENTITY: JSON.stringify({
            moduleId: "support/tickets",
            moduleReleaseDigest: supportArtifact.moduleReleaseDigest,
            uiArtifactDigest: supportArtifact.artifactDigest,
          }),
        },
        label: "generated-client",
      }
    );
    const generatedClientEvidence = extractFirstJsonObject(
      generatedClientRun.stdout
    );
    assert.ok(generatedClientEvidence);
    assert.equal(generatedClientEvidence.operationCount, 6);
    assert.equal(generatedClientEvidence.positiveInvocationCount, 6);
    assert.deepEqual(generatedClientEvidence.authorizationLayerRejections, [
      {
        label: "surface-grant-denied-detail",
        operationId: SUPPORT_TICKET_OPERATION_IDS.detail,
        status: 403,
      },
      {
        label: "module-authority-denied-restricted-detail",
        operationId: SUPPORT_TICKET_OPERATION_IDS.restrictedDetail,
        status: 403,
      },
    ]);
    assert.deepEqual(generatedClientEvidence.rejectedTamperVectors, [
      "wrong-module-release",
      "wrong-ui-artifact",
      "wrong-delegated-actor",
      "wrong-target-principal",
    ]);
    const observedContexts = (await readFile(observedContextFile, "utf8"))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line));
    assert.equal(
      observedContexts.length - observedBeforeGeneratedClient.length,
      generatedClientEvidence.positiveInvocationCount + 1,
      "rejected Surface Gateway tamper vectors must not execute the Provider; the exact Grant denial must also stay before the Provider, while the Module denial must execute it once"
    );
    const generatedObservedContexts = observedContexts.slice(
      observedBeforeGeneratedClient.length
    );
    const generatedOperationCounts = Object.fromEntries(
      Object.values(SUPPORT_TICKET_OPERATION_IDS).map((operationId) => [
        operationId,
        generatedObservedContexts.filter(
          (candidate) => candidate.operationId === operationId
        ).length,
      ])
    );
    assert.deepEqual(generatedOperationCounts, {
      [SUPPORT_TICKET_OPERATION_IDS.close]: 1,
      [SUPPORT_TICKET_OPERATION_IDS.create]: 2,
      [SUPPORT_TICKET_OPERATION_IDS.detail]: 0,
      [SUPPORT_TICKET_OPERATION_IDS.list]: 2,
      [SUPPORT_TICKET_OPERATION_IDS.restrictedDetail]: 1,
      [SUPPORT_TICKET_OPERATION_IDS.update]: 1,
    });
    const restrictedContext = generatedObservedContexts.find(
      (candidate) =>
        candidate.operationId === SUPPORT_TICKET_OPERATION_IDS.restrictedDetail
    );
    assert.ok(
      restrictedContext,
      "Provider did not observe the restricted detail alias"
    );
    assert.equal(restrictedContext.accepted, false);
    assert.equal(restrictedContext.actor, operatorActor);
    assert.equal(restrictedContext.authority, receiptDigest);
    assert.equal(restrictedContext.capability, "support_ticket.tickets.read");
    assert.equal(restrictedContext.serviceId, serviceId);
    assert.equal(
      restrictedContext.contractDigest,
      SUPPORT_TICKET_CONTRACT_DIGEST
    );
    assert.equal(restrictedContext.tenantId, tenantId);
    assert.equal(restrictedContext.story.storyId, storyId);
    assert.ok(
      restrictedContext.deadlineUnixMs > restrictedContext.observedAtUnixMs
    );
    for (const operationId of [
      SUPPORT_TICKET_OPERATION_IDS.close,
      SUPPORT_TICKET_OPERATION_IDS.create,
      SUPPORT_TICKET_OPERATION_IDS.list,
      SUPPORT_TICKET_OPERATION_IDS.update,
    ]) {
      const context = generatedObservedContexts.find(
        (candidate) => candidate.operationId === operationId
      );
      assert.ok(context, `provider did not observe ${operationId}`);
      assert.equal(context.accepted, true);
      assert.equal(context.actor, operatorActor);
      assert.equal(context.authority, receiptDigest);
      assert.equal(context.serviceId, serviceId);
      assert.equal(context.contractDigest, SUPPORT_TICKET_CONTRACT_DIGEST);
      assert.equal(context.tenantId, tenantId);
      assert.equal(context.story.storyId, storyId);
      assert.ok(context.deadlineUnixMs > context.observedAtUnixMs);
      assert.match(context.idempotencyKey, /^support-desk-/u);
    }

    const browserAccessContext = await getJson(
      `${consoleUrl}/api/console/v1/access/context`,
      operatorAuthorization
    );
    assert.equal(browserAccessContext.body.actor.user_id, operatorActor);
    assert.deepEqual(browserAccessContext.body.capabilities, ["*"]);
    const browserServices = await getJson(
      `${consoleUrl}/api/console/v1/services`,
      operatorAuthorization
    );
    const browserService = browserServices.body.find(
      (service) => service.serviceId === serviceId
    );
    assert.ok(browserService);
    assert.equal(browserService.enrollmentState, "active");
    assert.equal(browserService.connectionState, "ready");
    assert.ok(browserService.enrollmentExpiresAtUnixMs > Date.now());

    log("driving the real Console UI with playwright-cli");
    const browserEvidence = await runBrowserAcceptance({
      adapterEndpoint: adapterState.endpoint,
      adapterToken,
      artifactSetupToken,
      cleanup,
      consoleUrl,
      evidenceRoot: path.join(temporaryRoot, "evidence"),
      playwrightCli,
      privateKeyFragments: [
        serviceKeys.privateKey
          .export({ format: "der", type: "pkcs8" })
          .toString("base64")
          .slice(-32),
        consoleKeys.privateKey
          .export({ format: "der", type: "pkcs8" })
          .toString("base64")
          .slice(-32),
      ],
      operatorIdentifier,
      operatorPassword,
      providerBaseUrl,
      recoveryToken,
      sessionToken: bootstrap.body.token,
      systemPlaneToken,
      temporaryRoot,
    });

    log("proving asynchronous Suspend and Resume through the Local Adapter");
    let observation = (
      await getJson(workloadUrl(consoleUrl), operatorAuthorization)
    ).body;
    assert.equal(observation.state, "running");
    assert.ok(observation.observedRevision);
    const suspend = await postJson(
      `${workloadUrl(consoleUrl)}/operations`,
      operatorAuthorization,
      {
        action: { kind: "suspend" },
        idempotencyKey: "support-desk-suspend-0001",
        observedRevision: observation.observedRevision,
      },
      [202]
    );
    const suspended = await waitForOperation(
      consoleUrl,
      suspend.body.operationId,
      operatorAuthorization
    );
    assert.equal(suspended.phase, "succeeded");
    assert.equal(suspended.result.state, "suspended");
    observation = (
      await getJson(workloadUrl(consoleUrl), operatorAuthorization)
    ).body;
    assert.equal(observation.state, "suspended");
    const resume = await postJson(
      `${workloadUrl(consoleUrl)}/operations`,
      operatorAuthorization,
      {
        action: { kind: "resume" },
        idempotencyKey: "support-desk-resume-0001",
        observedRevision: observation.observedRevision,
      },
      [202]
    );
    const running = await waitForOperation(
      consoleUrl,
      resume.body.operationId,
      operatorAuthorization
    );
    assert.equal(running.phase, "succeeded");
    assert.equal(running.result.state, "running");

    log("stopping the Adapter and proving fail-closed Unknown with no queue");
    await cleanupSystemDev({
      adapterToken,
      appRoot,
      label: "system-cleanup",
      lenso,
      sandboxProcess,
    });
    sandboxProcess = undefined;
    observation = await waitFor(
      async () => {
        const value = (
          await getJson(workloadUrl(consoleUrl), operatorAuthorization)
        ).body;
        return value.state === "unknown" ? value : null;
      },
      { description: "Unknown workload observation", intervalMs: 250, timeoutMs: 10_000 }
    );
    assert.equal(observation.observedRevision, undefined);
    assert.equal(observation.activeOperation, undefined);
    const rejected = await postJson(
      `${workloadUrl(consoleUrl)}/operations`,
      operatorAuthorization,
      {
        action: { kind: "suspend" },
        idempotencyKey: "support-desk-outage-0001",
        observedRevision: running.result.observedRevision,
      },
      [502]
    );
    assert.notEqual(rejected.status, 202);
    await new Promise((resolve) => setTimeout(resolve, 750));
    observation = (
      await getJson(workloadUrl(consoleUrl), operatorAuthorization)
    ).body;
    assert.equal(observation.state, "unknown");
    assert.equal(observation.activeOperation, undefined);

    const evidence = {
      appCompositionDigest: composition.contentDigest,
      browser: browserEvidence,
      enrollmentReceiptDigest: receiptDigest,
      generatedClient: "list/create/update/close",
      localAdapter: "suspend/resume/unavailable-fail-closed",
      stateProjection: ["connected", "unavailable", "incompatible", "unmanaged"],
      storyAvailability: {
        authorized: browserEvidence.storyArtifact,
        unavailable: [incompatibleStoryBrowser, unauthorizedStoryBrowser],
      },
      systemId,
    };
    process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
  } finally {
    try {
      await cleanupAll();
    } finally {
      removeSignalCleanup();
    }
  }
};

await main();
