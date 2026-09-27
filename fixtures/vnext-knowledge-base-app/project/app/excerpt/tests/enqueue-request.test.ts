import { expect, test } from "bun:test";
import { excerptEnqueueRequest } from "../src/enqueue-request.ts";
import type { EnqueueRequest } from "../src/jobs.generated.ts";

test("the same note replays the exact Jobs request and receives its original job ID", () => {
  const saved = {
    availableAt: "2026-09-27T08:00:00Z",
    excerptLimit: 96,
    noteId: "note-123",
    ownerId: "user-456",
    text: "The note body",
  };
  const first = excerptEnqueueRequest(saved);
  const replay = excerptEnqueueRequest({ ...saved });
  expect(replay).toEqual(first);
  expect(first.available_at).toBe(saved.availableAt);

  let stored: { request: EnqueueRequest; jobId: string } | undefined;
  const enqueue = (request: EnqueueRequest) => {
    if (stored === undefined) {
      stored = { request, jobId: "job_original" };
      return { jobId: stored.jobId, created: true };
    }
    if (JSON.stringify(request) !== JSON.stringify(stored.request)) {
      throw new Error("idempotency_conflict");
    }
    return { jobId: stored.jobId, created: false };
  };

  expect(enqueue(first)).toEqual({ jobId: "job_original", created: true });
  expect(enqueue(replay)).toEqual({ jobId: "job_original", created: false });
});
