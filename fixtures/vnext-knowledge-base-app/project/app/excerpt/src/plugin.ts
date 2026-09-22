import { definePlugin, type DependencyDeclaration } from "@lenso/bun-plugin";
import type { InvocationContext } from "@lenso/contract-runtime";
import { tool, tools } from "@lenso/agent-tool-sdk";
import * as schema from "@lenso/agent-tool-sdk/schema";

import { Jobs, type JobsClient, type Timestamp } from "./jobs.generated.js";

const EXCERPT_LIMIT = 96;

function excerpt(text: string): string {
  const normalized = text.trim().replace(/\s+/gu, " ");
  const characters = Array.from(normalized);
  if (characters.length <= EXCERPT_LIMIT) return normalized;
  return `${characters.slice(0, EXCERPT_LIMIT - 1).join("")}…`;
}

interface ExcerptInstance {
  readonly jobs: JobsClient | undefined;
}

const jobsDependency: DependencyDeclaration<JobsClient, "optional"> = Jobs.optional("jobs");

function failure(operation: string, error: unknown) {
  return {
    ok: false as const,
    error: {
      code: "execution_failed" as const,
      payload: {
        reason_code: `jobs_${operation}_failed`,
        message: `Jobs ${operation} failed`,
        details_json: JSON.stringify(error),
      },
    },
  };
}

function now(): Timestamp {
  return new Date().toISOString() as Timestamp;
}

function context(call: InvocationContext): InvocationContext {
  return call;
}

export default definePlugin({
  dependencies: { jobs: jobsDependency },
  create({ dependencies }) {
    return { jobs: dependencies.jobs } satisfies ExcerptInstance;
  },
  providers: [
    tools<ExcerptInstance>([
      tool(
        {
          name: "knowledge.excerpt",
          description: "Create a deterministic UTF-8-safe excerpt for a knowledge note.",
          input: schema.object({ text: schema.string() }),
          output: schema.string(),
          execution: "parallel_safe",
        },
        ({ text }) => ({ ok: true, value: excerpt(text) }),
      ),
      tool(
        {
          name: "knowledge.enqueue-excerpt",
          description: "Queue deterministic excerpt processing for one knowledge note.",
          input: schema.object({ noteId: schema.string(), text: schema.string() }),
          output: schema.object({
            excerpt: schema.string(),
            jobId: schema.string(),
            status: schema.string(),
          }),
          execution: "parallel_safe",
        },
        async ({ noteId, text }, call, instance) => {
          if (instance.jobs === undefined) {
            return {
              ok: true,
              value: {
                excerpt: excerpt(text),
                jobId: `inline:${noteId}`,
                status: "succeeded",
              },
            };
          }
          const result = await instance.jobs.enqueue(
            {
              available_at: now(),
              idempotency_key: `knowledge-excerpt:${noteId}`,
              kind: "knowledge.excerpt",
              max_attempts: 3,
              payload: { note_id: noteId, text },
              queue: "knowledge",
            },
            context(call),
          );
          if (!result.ok) return failure("enqueue", result.error);
          return {
            ok: true,
            value: { excerpt: "", jobId: result.value.job_id, status: "queued" },
          };
        },
      ),
      tool(
        {
          name: "knowledge.process-excerpt",
          description: "Claim and complete the next durable knowledge excerpt job.",
          input: schema.object({}),
          output: schema.object({
            excerpt: schema.string(),
            jobId: schema.string(),
            noteId: schema.string(),
          }),
          execution: "exclusive",
        },
        async (_input, call, instance) => {
          if (instance.jobs === undefined) return failure("unavailable", "Jobs is not bound");
          const claimed = await instance.jobs.claim({ queue: "knowledge" }, context(call));
          if (!claimed.ok) return failure("claim", claimed.error);
          const noteId = claimed.value.payload.note_id;
          const text = claimed.value.payload.text;
          if (typeof noteId !== "string" || typeof text !== "string") {
            await instance.jobs.fail(
              {
                failure_code: "invalid_payload",
                job_id: claimed.value.job_id,
                lease_token: claimed.value.lease_token,
                retryable: false,
              },
              context(call),
            );
            return failure("payload", "excerpt payload is invalid");
          }
          const completed = await instance.jobs.complete(
            {
              job_id: claimed.value.job_id,
              lease_token: claimed.value.lease_token,
            },
            context(call),
          );
          if (!completed.ok || !completed.value.completed) {
            return failure("complete", completed.ok ? "lease was not completed" : completed.error);
          }
          return {
            ok: true,
            value: {
              excerpt: excerpt(text),
              jobId: claimed.value.job_id,
              noteId,
            },
          };
        },
      ),
      tool(
        {
          name: "knowledge.inspect-excerpt",
          description: "Read durable processing state for one knowledge excerpt job.",
          input: schema.object({ jobId: schema.string() }),
          output: schema.object({
            attempts: schema.number(),
            jobId: schema.string(),
            status: schema.string(),
          }),
          execution: "parallel_safe",
        },
        async ({ jobId }, call, instance) => {
          if (instance.jobs === undefined) return failure("unavailable", "Jobs is not bound");
          const inspected = await instance.jobs.inspect({ job_id: jobId }, context(call));
          if (!inspected.ok) return failure("inspect", inspected.error);
          return {
            ok: true,
            value: {
              attempts: inspected.value.attempts,
              jobId: inspected.value.job_id,
              status: inspected.value.status,
            },
          };
        },
      ),
    ]),
  ],
});
