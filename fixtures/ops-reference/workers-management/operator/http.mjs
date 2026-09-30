// Private envelopes arrive on stdin; neither credentials nor responses are logged.
let input = "";
for await (const chunk of process.stdin) {
  input += chunk;
  if (input.length > 131072) throw new Error("operator_input_too_large");
}
try {
  const { url, capability, request } = JSON.parse(input);
  const response = await fetch(url, {
    method: "POST", redirect: "error", signal: AbortSignal.timeout(35000),
    headers: { authorization: `Bearer ${capability}`, "content-type": "application/json" },
    body: JSON.stringify(request),
  });
  const body = await response.text();
  if (body.length > 65536) throw new Error("operator_response_too_large");
  process.stdout.write(JSON.stringify({ status: response.status, body: JSON.parse(body) }));
} catch {
  process.stdout.write(JSON.stringify({ status: null, body: { state: "unknown" } }));
  process.exitCode = 1;
}
