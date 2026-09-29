export class BodyReadError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

export async function boundedText(body, limit, { signal, timeoutMs = 3000 } = {}) {
  if (signal?.aborted) throw new BodyReadError(408, "body read cancelled");
  if (!body) return "";
  const reader = body.getReader();
  let aborted = false;
  let timedOut = false;
  const cancel = () => {
    // A stream's cancellation hook may itself never settle. Cancelling closes
    // pending reads; do not let that hook defeat this reader's deadline.
    void reader.cancel().catch(() => {});
  };
  const abort = () => { aborted = true; cancel(); };
  const timer = setTimeout(() => { timedOut = true; abort(); }, timeoutMs);
  signal?.addEventListener("abort", abort, { once: true });
  const chunks = [];
  let length = 0;
  let emptyChunks = 0;
  try {
    if (signal?.aborted) abort();
    for (;;) {
      const { done, value } = await reader.read();
      if (aborted) throw new BodyReadError(408, timedOut ? "body read timed out" : "body read cancelled");
      if (done) break;
      if (!(value instanceof Uint8Array)) throw new BodyReadError(400, "invalid body chunk");
      if (value.byteLength === 0) {
        if (++emptyChunks > 16) throw new BodyReadError(400, "body stream made no progress");
        continue;
      }
      emptyChunks = 0;
      length += value.byteLength;
      if (length > limit) throw new BodyReadError(413, "body exceeds fixture limit");
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch (error) {
    cancel();
    if (error instanceof BodyReadError) throw error;
    if (aborted) throw new BodyReadError(408, "body read cancelled");
    throw new BodyReadError(400, "invalid request body");
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
    reader.releaseLock();
  }
}
