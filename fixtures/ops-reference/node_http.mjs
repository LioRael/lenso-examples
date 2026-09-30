import { readFileSync } from "node:fs";

const input = JSON.parse(readFileSync(0, "utf8"));
try {
  const response = await fetch(input.url, {
    method: input.payload === null ? "GET" : "POST",
    headers: {
      "content-type": "application/json",
      "user-agent": "lenso-reference-qualification/1",
      connection: "close",
    },
    body: input.payload === null ? undefined : JSON.stringify(input.payload),
    redirect: "error",
    signal: AbortSignal.timeout(10000),
  });
  process.stdout.write(JSON.stringify([response.status, await response.json()]));
} catch (error) {
  process.stderr.write(JSON.stringify({ name: error.name, code: error.cause?.code }));
  process.exitCode = 1;
}
