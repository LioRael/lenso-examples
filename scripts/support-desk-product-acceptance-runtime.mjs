import { spawn } from "node:child_process";
import { createServer } from "node:http";
import {
  access,
  cp,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { basename, extname, join } from "node:path";
import { pathToFileURL } from "node:url";

const sleep = (milliseconds) =>
  new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });

const tail = (value, length = 12_000) =>
  value.length <= length ? value : value.slice(-length);
const managedChildren = new Set();

export const assertNoForbiddenEvidence = (evidence, forbiddenValues) => {
  const exposedLabels = forbiddenValues
    .filter(
      ({ value }) =>
        typeof value === "string" && value && String(evidence).includes(value)
    )
    .map(({ label }) => label);
  if (exposedLabels.length > 0) {
    throw new Error(
      `browser evidence exposed forbidden values: ${exposedLabels.join(", ")}`
    );
  }
};

export const assertBrowserSessionDoesNotReuseServerCredential = (
  sessionTokens,
  serverOwnedCredentials
) => {
  const browserSessions = new Set(
    sessionTokens.filter((token) => typeof token === "string" && token)
  );
  const reusedLabels = [
    ...new Set(
      serverOwnedCredentials
        .filter(
          ({ value }) =>
            typeof value === "string" &&
            value &&
            browserSessions.has(value)
        )
        .map(({ label }) => label)
    ),
  ];
  if (reusedLabels.length > 0) {
    throw new Error(
      `browser session reused server-owned credentials: ${reusedLabels.join(", ")}`
    );
  }
};

const bearerHeaderLine =
  /^(\s*authorization:\s*Bearer\s+)(\S+)(\s*)$/iu;

export const extractSameOriginConsoleBearerTokens = (
  requestRecords,
  { consoleOrigin }
) => {
  const tokens = new Set();
  for (const record of requestRecords) {
    let url;
    try {
      url = new URL(record.url);
    } catch {
      continue;
    }
    if (
      url.origin !== consoleOrigin ||
      !url.pathname.startsWith("/api/console/")
    ) {
      continue;
    }
    for (const line of String(record.details ?? "").split("\n")) {
      const match = line.match(bearerHeaderLine);
      if (match) {
        tokens.add(match[2]);
      }
    }
  }
  return [...tokens];
};

export const redactSameOriginSessionAuthorization = (
  record,
  { consoleOrigin, sessionTokens }
) => {
  let url;
  try {
    url = new URL(record.url);
  } catch {
    return String(record.details ?? "");
  }
  if (url.origin !== consoleOrigin) {
    return String(record.details ?? "");
  }
  const allowed = new Set(
    sessionTokens.filter((token) => typeof token === "string" && token)
  );
  return String(record.details ?? "")
    .split("\n")
    .map((line) => {
      const match = line.match(bearerHeaderLine);
      if (!match || !allowed.has(match[2])) {
        return line;
      }
      return `${match[1]}[redacted same-origin session]${match[3]}`;
    })
    .join("\n");
};

export const redactedAccessibleSnapshotDiagnostic = (
  snapshot,
  { maxChars = 8_192, sensitiveValues = [] } = {}
) => {
  let redacted = typeof snapshot === "string" ? snapshot : String(snapshot ?? "");
  redacted = redacted
    .replace(
      /\bvalue\s*=\s*(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s\]]+)/giu,
      'value="[redacted]"'
    )
    .replace(
      /^(\s*(?:-\s*)?(?:textbox|searchbox|combobox|spinbutton|slider)\b.*?)(\s*:\s*).+$/gimu,
      "$1$2[redacted]"
    );
  for (const sensitiveValue of sensitiveValues) {
    if (typeof sensitiveValue === "string" && sensitiveValue) {
      redacted = redacted.split(sensitiveValue).join("[redacted]");
    }
  }
  if (!redacted) {
    return "[no accessible snapshot captured]";
  }
  const limit = Number.isSafeInteger(maxChars) && maxChars >= 256
    ? maxChars
    : 8_192;
  if (redacted.length <= limit) {
    return redacted;
  }
  const marker = "\n... [accessible snapshot truncated] ...\n";
  const available = limit - marker.length;
  const headLength = Math.ceil(available / 2);
  const tailLength = Math.floor(available / 2);
  return `${redacted.slice(0, headLength)}${marker}${redacted.slice(-tailLength)}`;
};

export const parsePlaywrightTicketTitleControls = (snapshot) =>
  String(snapshot ?? "")
    .split("\n")
    .flatMap((line) => {
      const match = line.match(
        /\btextbox "Title for ([^"\r\n]+)" \[ref=([^\]\r\n]+)\](?::\s*(.*))?$/u
      );
      if (!match) {
        return [];
      }
      return [
        {
          line,
          ref: match[2],
          ticketId: match[1],
          value: (match[3] ?? "").trim(),
        },
      ];
    });

export const findPlaywrightTicketTitleControl = (
  snapshot,
  { ticketId, title }
) =>
  parsePlaywrightTicketTitleControls(snapshot).find(
    (control) =>
      control.ticketId === ticketId &&
      (title === undefined || control.value === title)
  );

export const findNewPlaywrightTicketTitleControl = (
  snapshot,
  { existingTicketIds, title }
) => {
  const existing = new Set(existingTicketIds);
  return parsePlaywrightTicketTitleControls(snapshot).find(
    (control) => !existing.has(control.ticketId) && control.value === title
  );
};

export const parsePlaywrightRequestEntries = (output) =>
  Array.from(
    String(output ?? "").matchAll(
      /^(\d+)\. \[([A-Z]+)\] (\S+?)(?: => \[([^\]\r\n]+)\](?: [^\r\n]*)?)?$/gmu
    ),
    (match) => ({
      index: match[1],
      method: match[2],
      status: match[4] ?? "pending",
      url: match[3],
    })
  );

const diagnosticField = (value) =>
  typeof value === "string" && value
    ? JSON.stringify(value.slice(0, 256))
    : "<missing>";

export const formatPlaywrightGatewayCandidateDiagnostics = (candidates) => {
  if (!Array.isArray(candidates) || candidates.length === 0) {
    return "[no matching POST candidates]";
  }
  const diagnostic = candidates
    .map(
      ({
        entry,
        request,
        requestBodyState,
        response,
        responseBodyState,
      }) => {
        let pathname = "<invalid-url>";
        try {
          pathname = new URL(entry.url).pathname;
        } catch {
          // A malformed URL is useful candidate metadata but must not expose
          // the original value, query parameters, or credentials.
        }
        return [
          `#${entry.index}`,
          entry.method,
          pathname,
          `status=${entry.status}`,
          `requestBody=${requestBodyState}`,
          `requestOperationId=${diagnosticField(request?.operationId)}`,
          `responseBody=${responseBodyState}`,
          `responseProtocol=${diagnosticField(response?.protocol)}`,
          `responseOperationId=${diagnosticField(response?.operationId)}`,
          `responseHasOutput=${Boolean(
            response && Object.hasOwn(response, "output")
          )}`,
        ].join(" ");
      }
    )
    .join("\n");
  return redactedAccessibleSnapshotDiagnostic(diagnostic, { maxChars: 8_192 });
};

export const isSuccessfulPlaywrightGatewayCandidate = (
  { entry, response },
  { consoleOrigin, operationId, surfaceGatewayPath }
) => {
  let url;
  try {
    url = new URL(entry.url);
  } catch {
    return false;
  }
  return (
    entry.method === "POST" &&
    entry.status === "200" &&
    url.origin === consoleOrigin &&
    url.pathname === surfaceGatewayPath &&
    response?.protocol === "lenso.console-surface-gateway.v1" &&
    response.operationId === operationId &&
    response.output !== undefined &&
    response.output !== null
  );
};

const terminateAllManaged = async () => {
  const results = await Promise.allSettled(
    Array.from(managedChildren, (child) => terminateManaged(child))
  );
  const errors = results
    .filter((result) => result.status === "rejected")
    .map((result) => result.reason);
  if (errors.length > 0) {
    throw new AggregateError(errors, "managed process cleanup failed");
  }
};

export const installSignalCleanup = (cleanup) => {
  let handlingSignal = false;
  const handlers = new Map();
  const remove = () => {
    for (const [signal, handler] of handlers) {
      process.off(signal, handler);
    }
    handlers.clear();
  };
  for (const signal of ["SIGINT", "SIGTERM"]) {
    const handler = () => {
      if (handlingSignal) {
        return;
      }
      handlingSignal = true;
      void (async () => {
        const errors = [];
        try {
          await cleanup();
        } catch (error) {
          errors.push(error);
        }
        try {
          await terminateAllManaged();
        } catch (error) {
          errors.push(error);
        }
        if (errors.length > 0) {
          process.stderr.write(
            `[acceptance] signal cleanup failed: ${errors
              .map((error) => error?.message ?? error)
              .join("; ")}\n`
          );
        }
        remove();
        process.kill(process.pid, signal);
      })();
    };
    handlers.set(signal, handler);
    process.once(signal, handler);
  }
  return remove;
};

export const pathExists = async (path) =>
  access(path).then(
    () => true,
    () => false
  );

export const resolveExecutable = async (name) => {
  if (name.includes("/")) {
    if (await pathExists(name)) {
      return name;
    }
    return null;
  }
  for (const directory of (process.env.PATH ?? "").split(":")) {
    const candidate = join(directory, name);
    if (await pathExists(candidate)) {
      return candidate;
    }
  }
  return null;
};

const childEnvironment = (overrides) => {
  const environment = { ...process.env };
  for (const [name, value] of Object.entries(overrides)) {
    if (value === undefined || value === null) {
      delete environment[name];
    } else {
      environment[name] = String(value);
    }
  }
  return environment;
};

export const runCommand = async (
  command,
  args,
  { cwd, env = {}, label = basename(command), quiet = false } = {}
) =>
  new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: childEnvironment(env),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
      if (!quiet) {
        process.stderr.write(`[${label}] ${chunk}`);
      }
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
      if (!quiet) {
        process.stderr.write(`[${label}] ${chunk}`);
      }
    });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) {
        resolve({ stderr, stdout });
        return;
      }
      reject(
        new Error(
          `${label} exited with ${signal ?? `code ${code}`}\n${tail(
            `${stdout}\n${stderr}`
          )}`
        )
      );
    });
  });

export const spawnManaged = (
  command,
  args,
  { cwd, env = {}, label = basename(command) } = {}
) => {
  const child = spawn(command, args, {
    cwd,
    detached: process.platform !== "win32",
    env: childEnvironment(env),
    stdio: ["ignore", "pipe", "pipe"],
  });
  const output = { stderr: "", stdout: "" };
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    output.stdout += chunk;
    process.stderr.write(`[${label}] ${chunk}`);
  });
  child.stderr.on("data", (chunk) => {
    output.stderr += chunk;
    process.stderr.write(`[${label}] ${chunk}`);
  });
  child.acceptanceOutput = output;
  child.acceptanceLabel = label;
  managedChildren.add(child);
  child.once("exit", () => managedChildren.delete(child));
  return child;
};

export const waitForExit = async (child, timeoutMs = 15_000) => {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  await new Promise((resolve, reject) => {
    let settled = false;
    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      child.off("exit", onExit);
      result();
    };
    const onExit = () => finish(resolve);
    const timer = setTimeout(
      () =>
        finish(() =>
          reject(
            new Error(`${child.acceptanceLabel ?? "process"} did not exit`)
          )
        ),
      timeoutMs
    );
    child.once("exit", onExit);
    if (child.exitCode !== null || child.signalCode !== null) {
      onExit();
    }
  });
};

export const terminateManaged = async (child) => {
  if (!child) {
    return;
  }
  const signalProcessTree = (signal) => {
    if (process.platform !== "win32" && child.pid) {
      try {
        process.kill(-child.pid, signal);
        return;
      } catch (error) {
        if (error?.code !== "ESRCH") {
          throw error;
        }
      }
    }
    if (child.exitCode === null && child.signalCode === null) {
      child.kill(signal);
    }
  };
  signalProcessTree("SIGTERM");
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  try {
    await waitForExit(child, 5_000);
  } catch {
    signalProcessTree("SIGKILL");
    await waitForExit(child, 5_000);
  }
};

export const waitFor = async (
  probe,
  { description, intervalMs = 200, timeoutMs = 60_000 }
) => {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const value = await probe();
      if (value) {
        return value;
      }
    } catch (error) {
      lastError = error;
    }
    await sleep(intervalMs);
  }
  throw new Error(
    `${description} did not become ready within ${timeoutMs}ms${
      lastError ? `: ${lastError.message}` : ""
    }`
  );
};

export const extractFirstJsonObject = (output) => {
  for (let start = output.indexOf("{"); start !== -1; start = output.indexOf("{", start + 1)) {
    let depth = 0;
    let escaped = false;
    let inString = false;
    for (let index = start; index < output.length; index += 1) {
      const character = output[index];
      if (inString) {
        if (escaped) {
          escaped = false;
        } else if (character === "\\") {
          escaped = true;
        } else if (character === '"') {
          inString = false;
        }
        continue;
      }
      if (character === '"') {
        inString = true;
      } else if (character === "{") {
        depth += 1;
      } else if (character === "}") {
        depth -= 1;
        if (depth === 0) {
          try {
            return JSON.parse(output.slice(start, index + 1));
          } catch {
            break;
          }
        }
      }
    }
  }
  return null;
};

export const waitForJsonOutput = async (child, description, timeoutMs = 60_000) =>
  waitFor(
    () => {
      const output = child.acceptanceOutput?.stdout ?? "";
      const value = extractFirstJsonObject(output);
      if (value === null) {
        if (child.exitCode !== null || child.signalCode !== null) {
          throw new Error(
            `${description} exited early\n${tail(
              `${output}\n${child.acceptanceOutput?.stderr ?? ""}`
            )}`
          );
        }
        return null;
      }
      return value;
    },
    { description, timeoutMs }
  );

export const fetchJson = async (url, init = {}, expectedStatuses = [200]) => {
  const response = await fetch(url, init);
  const text = await response.text();
  let body = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = text;
    }
  }
  if (!expectedStatuses.includes(response.status)) {
    throw new Error(
      `${init.method ?? "GET"} ${url} returned ${response.status}: ${text}`
    );
  }
  return { body, headers: response.headers, status: response.status };
};

export const waitForHttp = async (url, timeoutMs = 90_000) =>
  waitFor(
    async () => {
      const response = await fetch(url);
      return response.ok ? response : null;
    },
    { description: url, timeoutMs }
  );

export const freePort = async () =>
  new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("Could not allocate a loopback port"));
        return;
      }
      const { port } = address;
      server.close((error) => (error ? reject(error) : resolve(port)));
    });
  });

const contentType = (path) => {
  switch (extname(path)) {
    case ".gz":
      return "application/gzip";
    case ".json":
      return "application/json";
    default:
      return "application/octet-stream";
  }
};

export const startStaticServer = async (root) => {
  const server = createServer(async (request, response) => {
    try {
      const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
      const name = decodeURIComponent(requestUrl.pathname).replace(/^\/+/, "");
      if (!name || name.includes("/") || name.includes("..")) {
        response.writeHead(404).end();
        return;
      }
      const bytes = await readFile(join(root, name));
      response.writeHead(200, {
        "content-length": bytes.byteLength,
        "content-type": contentType(name),
      });
      response.end(bytes);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("Artifact server did not bind a loopback port");
  }
  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
};

export const prepareProviderSandbox = async ({ frameworkRoot, sourceRoot, targetRoot }) => {
  const source = join(sourceRoot, "examples", "support-ticket");
  const serviceKit = join(
    frameworkRoot,
    "sdk",
    "typescript",
    "packages",
    "service-kit"
  );
  await mkdir(join(targetRoot, "node_modules", "@lenso"), { recursive: true });
  await cp(join(source, "src"), join(targetRoot, "src"), { recursive: true });
  await cp(
    join(source, "lenso.module.json"),
    join(targetRoot, "lenso.module.json")
  );
  await writeFile(
    join(targetRoot, "package.json"),
    `${JSON.stringify(
      {
        name: "support-desk-acceptance-provider",
        private: true,
        type: "module",
      },
      null,
      2
    )}\n`,
    "utf8"
  );
  await symlink(serviceKit, join(targetRoot, "node_modules", "@lenso", "service-kit"));
  const moduleUrl = pathToFileURL(join(targetRoot, "src", "module.ts"));
  moduleUrl.searchParams.set("acceptance", String(Date.now()));
  const { manifest } = await import(moduleUrl.href);
  await writeFile(
    join(targetRoot, "lenso.service.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8"
  );
};

export const startEphemeralPostgres = async (root) => {
  const initdb = await resolveExecutable("initdb");
  const postgres = await resolveExecutable("postgres");
  const pgIsReady = await resolveExecutable("pg_isready");
  if (!(initdb && postgres && pgIsReady)) {
    throw new Error(
      "initdb, postgres, and pg_isready are required when LENSO_ACCEPTANCE_DATABASE_URL is unset"
    );
  }
  const data = join(root, "postgres-data");
  // PostgreSQL caps Unix-domain socket paths at roughly 100 bytes. macOS
  // expands its temporary root through /private/var/folders, so keep only the
  // socket in a separately randomized short directory.
  const socket = await mkdtemp("/tmp/lenso-support-pg-");
  let child;
  try {
    await runCommand(
      initdb,
      [
        "-D",
        data,
        "-A",
        "trust",
        "--no-locale",
        "--encoding=UTF8",
        "--username=postgres",
      ],
      { label: "postgres-init", quiet: true }
    );
    const port = await freePort();
    child = spawnManaged(
      postgres,
      ["-D", data, "-h", "127.0.0.1", "-p", String(port), "-k", socket],
      { label: "postgres" }
    );
    await waitFor(
      async () => {
        try {
          await runCommand(
            pgIsReady,
            [
              "-h",
              "127.0.0.1",
              "-p",
              String(port),
              "-U",
              "postgres",
              "-d",
              "postgres",
            ],
            { label: "pg-isready", quiet: true }
          );
          return true;
        } catch {
          return false;
        }
      },
      { description: "ephemeral PostgreSQL", timeoutMs: 30_000 }
    );
    return {
      child,
      dispose: async () => {
        await terminateManaged(child);
        await rm(socket, { force: true, recursive: true });
      },
      url: `postgresql://postgres@127.0.0.1:${port}/postgres?sslmode=disable`,
    };
  } catch (error) {
    await terminateManaged(child);
    await rm(socket, { force: true, recursive: true });
    throw error;
  }
};
