import { definePlugin } from "@lenso/bun-plugin";
import { tool, tools } from "@lenso/agent-tool-sdk";
import * as schema from "@lenso/agent-tool-sdk/schema";

export default definePlugin({
  providers: [
    tools([
      tool(
        {
          name: "example.uppercase",
          description: "Uppercase one UTF-8 string.",
          input: schema.object({ text: schema.string() }),
          output: schema.string(),
          execution: "parallel_safe",
        },
        ({ text }) => ({ ok: true, value: text.toUpperCase() }),
      ),
    ]),
  ],
});
