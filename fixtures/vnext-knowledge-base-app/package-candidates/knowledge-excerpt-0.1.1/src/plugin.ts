import { definePlugin, type DependencyDeclaration } from "@lenso/bun-plugin";
import type { InvocationContext } from "@lenso/contract-runtime";
import { tool, tools } from "@lenso/agent-tool-sdk";
import * as schema from "@lenso/agent-tool-sdk/schema";

import { Jobs, type JobsClient, type Timestamp } from "./jobs.generated.js";
import { excerpt } from "./excerpt.js";

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
          input: schema.object({
            excerptLimit: schema.number(),
            noteId: schema.string(),
            ownerId: schema.string(),
            text: schema.string(),
          }),
          output: schema.object({
            excerpt: schema.string(),
            jobId: schema.string(),
            status: schema.string(),
          }),
          execution: "parallel_safe",
        },
        async ({ excerptLimit, noteId, ownerId, text }, call, instance) => {
          if (!Number.isInteger(excerptLimit) || excerptLimit < 16 || excerptLimit > 512) {
            return failure("configuration", "excerptLimit must be an integer from 16 through 512");
          }
          if (instance.jobs === undefined) {
            return {
              ok: true,
              value: {
                excerpt: excerpt(text, excerptLimit),
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
              payload: { excerpt_limit: excerptLimit, note_id: noteId, owner_id: ownerId, text },
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
          name: "knowledge.claim-excerpt",
          description: "Claim the next durable knowledge excerpt job for idempotent persistence.",
          input: schema.object({}),
          output: schema.object({
            excerpt: schema.string(),
            jobId: schema.string(),
            leaseToken: schema.string(),
            noteId: schema.string(),
            ownerId: schema.string(),
          }),
          execution: "exclusive",
        },
        async (_input, call, instance) => {
          if (instance.jobs === undefined) return failure("unavailable", "Jobs is not bound");
          const claimed = await instance.jobs.claim({ queue: "knowledge" }, context(call));
          if (!claimed.ok) return failure("claim", claimed.error);
          const noteId = claimed.value.payload.note_id;
          const ownerId = claimed.value.payload.owner_id;
          const text = claimed.value.payload.text;
          const excerptLimit = claimed.value.payload.excerpt_limit;
          if (
            typeof noteId !== "string" ||
            typeof ownerId !== "string" ||
            typeof text !== "string" ||
            typeof excerptLimit !== "number" ||
            !Number.isInteger(excerptLimit) ||
            excerptLimit < 16 ||
            excerptLimit > 512
          ) {
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
          return {
            ok: true,
            value: {
              excerpt: excerpt(text, excerptLimit),
              jobId: claimed.value.job_id,
              leaseToken: claimed.value.lease_token,
              noteId,
              ownerId,
            },
          };
        },
      ),
      tool(
        {
          name: "knowledge.complete-excerpt",
          description: "Complete a durable excerpt job after its result is persisted.",
          input: schema.object({ jobId: schema.string(), leaseToken: schema.string() }),
          output: schema.object({ completed: schema.boolean() }),
          execution: "parallel_safe",
        },
        async ({ jobId, leaseToken }, call, instance) => {
          if (instance.jobs === undefined) return failure("unavailable", "Jobs is not bound");
          const completed = await instance.jobs.complete(
            { job_id: jobId, lease_token: leaseToken },
            context(call),
          );
          if (!completed.ok) return failure("complete", completed.error);
          return { ok: true, value: { completed: completed.value.completed } };
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
