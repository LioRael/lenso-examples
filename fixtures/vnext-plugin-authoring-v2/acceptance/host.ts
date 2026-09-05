import { defineHost, pluginBundle } from "@lenso/cli/host";

export default defineHost({
  id: "example.document-sync-acceptance",
  plugins: [],
  slots: [
    {
      id: "document-sync",
      cardinality: "many",
      maxInstances: 2,
      allow: [
        pluginBundle("../document-sync.lenso-plugin", { execution: "process" }),
      ],
    },
  ],
});
