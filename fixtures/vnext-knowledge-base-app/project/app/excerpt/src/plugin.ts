import { definePlugin } from "@lenso/bun-plugin";
import { tool, tools } from "@lenso/agent-tool-sdk";
import * as schema from "@lenso/agent-tool-sdk/schema";

const EXCERPT_LIMIT = 96;

function excerpt(text: string): string {
  const normalized = text.trim().replace(/\s+/gu, " ");
  const characters = Array.from(normalized);
  if (characters.length <= EXCERPT_LIMIT) return normalized;
  return `${characters.slice(0, EXCERPT_LIMIT - 1).join("")}…`;
}

export default definePlugin({
  providers: [
    tools([
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
    ]),
  ],
});
