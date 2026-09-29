import assert from "node:assert/strict";
import test from "node:test";
import { boundedText } from "./body.mjs";

const bytes = (text) => new TextEncoder().encode(text);

test("reads an exact byte-bound body and releases its lock", async () => {
  const body = new ReadableStream({
    start(controller) { controller.enqueue(bytes("value")); controller.close(); },
  });
  assert.equal(await boundedText(body, 5), "value");
  assert.equal(body.locked, false);
});

test("oversized bodies are cancelled and report 413", async () => {
  let cancelled = false;
  const body = new ReadableStream({
    start(controller) { controller.enqueue(bytes("too large")); },
    cancel() { cancelled = true; },
  });
  await assert.rejects(boundedText(body, 5), { status: 413 });
  assert.equal(cancelled, true);
  assert.equal(body.locked, false);
});

test("unfinished body times out even if its cancellation hook never settles",
  { timeout: 1000 }, async () => {
    let cancelled = false;
    const body = new ReadableStream({
      start(controller) { controller.enqueue(bytes("{")); },
      cancel() { cancelled = true; return new Promise(() => {}); },
    });
    await assert.rejects(boundedText(body, 4096, { timeoutMs: 20 }), { status: 408 });
    assert.equal(cancelled, true);
    assert.equal(body.locked, false);
  });

test("caller cancellation interrupts a pending read", { timeout: 1000 }, async () => {
  let cancelled = false;
  const body = new ReadableStream({ cancel() { cancelled = true; } });
  const controller = new AbortController();
  const reading = boundedText(body, 4096, { signal: controller.signal });
  controller.abort();
  await assert.rejects(reading, { status: 408 });
  assert.equal(cancelled, true);
  assert.equal(body.locked, false);
});

test("invalid UTF-8 is a bad body, not an oversized body", async () => {
  const body = new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array([255])); controller.close(); },
  });
  await assert.rejects(boundedText(body, 4096), { status: 400 });
});

test("empty chunks cannot evade the byte bound or starve the deadline", async () => {
  let cancelled = false;
  const body = new ReadableStream({
    pull(controller) { controller.enqueue(new Uint8Array()); },
    cancel() { cancelled = true; },
  });
  await assert.rejects(boundedText(body, 4096), { status: 400 });
  assert.equal(cancelled, true);
  assert.equal(body.locked, false);
});
