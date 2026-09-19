import { definePlugin } from "@lenso/bun-plugin";
import { Text, type TextProvider } from "../generated/text.ts";

export default definePlugin({
  provides: [Text],
  create(): TextProvider {
    return {
      async uppercase(_context, request) {
        return { ok: true, value: { text: request.text.toUpperCase() } };
      },
    };
  },
});
