import type { EnqueueRequest, Timestamp } from "./jobs.generated.js";

export interface ExcerptEnqueueInput {
  availableAt: string;
  excerptLimit: number;
  noteId: string;
  ownerId: string;
  text: string;
}

export function excerptEnqueueRequest(input: ExcerptEnqueueInput): EnqueueRequest {
  return {
    available_at: input.availableAt as Timestamp,
    idempotency_key: `knowledge-excerpt:${input.noteId}`,
    kind: "knowledge.excerpt",
    max_attempts: 3,
    payload: {
      excerpt_limit: input.excerptLimit,
      note_id: input.noteId,
      owner_id: input.ownerId,
      text: input.text,
    },
    queue: "knowledge",
  };
}
