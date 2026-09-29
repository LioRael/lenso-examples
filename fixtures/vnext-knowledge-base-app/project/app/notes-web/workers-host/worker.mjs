import coreModule from "./guest.core.wasm";
import { instantiate } from "./guest.js";
import plan from "./plan.mjs";
import verifiedArtifact from "./artifact.mjs";
import { createKnowledgeSettingsLocalWorkerAdapter } from "./knowledge-settings-local.mjs";

export default {
  async fetch(request, env) {
    const bridgeOrigin = env?.KNOWLEDGE_SETTINGS_BRIDGE_ORIGIN;
    if (typeof bridgeOrigin !== "string") {
      return Response.json({ error: "knowledge_storage_unavailable" }, { status: 503 });
    }
    const adapter = createKnowledgeSettingsLocalWorkerAdapter({
      plan,
      coreModule,
      instantiate,
      verifiedArtifact,
      bridgeOrigin,
      fetchBridge: fetch,
      bridgeTimeoutMs: 1000,
    });
    return adapter.handle(request);
  },
};
