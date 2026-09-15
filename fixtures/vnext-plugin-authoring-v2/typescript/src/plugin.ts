import {
  configuration,
  definePlugin,
  dependency,
  provider,
} from "@lenso/bun-plugin";
import {
  DOCUMENT_STORE_CONTRACT,
  type DocumentStoreClient,
} from "../generated/document-store.ts";
import {
  bindDocumentSyncProvider,
  CAPABILITY_ID,
  DESCRIPTOR_DIGEST,
  DESCRIPTOR_VERSION,
  type DocumentSyncProvider,
  type SyncResult,
} from "../generated/document-sync.ts";

interface SyncConfig {
  readonly document: string;
  readonly ruleset: "identity-v1";
}

const config = configuration<SyncConfig>(
  {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    title: "DocumentSyncConfig",
    type: "object",
    properties: {
      document: { type: "string", minLength: 1, maxLength: 128 },
      ruleset: { const: "identity-v1" },
    },
    required: ["document", "ruleset"],
    additionalProperties: false,
  },
  (input) => {
    const value = input as Partial<SyncConfig>;
    if (typeof value.document !== "string" || value.document.length === 0) {
      throw new Error("config requires document");
    }
    if (value.ruleset !== "identity-v1") {
      throw new Error("config requires ruleset identity-v1");
    }
    return { document: value.document, ruleset: value.ruleset };
  },
);

const source = dependency({
  id: "source",
  contract: DOCUMENT_STORE_CONTRACT,
});
const destination = dependency({
  id: "destination",
  contract: DOCUMENT_STORE_CONTRACT,
});

class DocumentSync implements DocumentSyncProvider {
  #running = false;
  #stopped = false;

  constructor(
    readonly config: SyncConfig,
    readonly source: DocumentStoreClient,
    readonly destination: DocumentStoreClient,
  ) {}

  async sync(context: Parameters<DocumentSyncProvider["sync"]>[0], request: Parameters<DocumentSyncProvider["sync"]>[1]): Promise<SyncResult> {
    if (this.#running || this.#stopped) {
      return { ok: false, error: { kind: "domain", error: "already_running" } };
    }
    this.#running = true;
    try {
      const read = await this.source.read({ document: request.document }, context);
      if (!read.ok) {
        return read.error.kind === "domain" && read.error.error === "not_found"
          ? { ok: false, error: { kind: "domain", error: "not_found" } }
          : { ok: false, error: { kind: "domain", error: "write_failed" } };
      }
      const write = await this.destination.put(
        { document: request.document, text: read.value.text },
        context,
      );
      if (!write.ok) {
        return { ok: false, error: { kind: "domain", error: "write_failed" } };
      }
      return {
        ok: true,
        value: { document: request.document, text: read.value.text },
      };
    } finally {
      this.#running = false;
    }
  }

  stop(): void {
    this.#stopped = true;
  }
}

const syncDescriptor = {
  capability_id: CAPABILITY_ID,
  descriptor_version: DESCRIPTOR_VERSION,
  descriptor_digest: DESCRIPTOR_DIGEST,
  operations: ["sync"],
  stream_operations: [],
  event_operations: [],
} as const;

export default definePlugin({
  config,
  dependencies: { source, destination },
  create({ config: selected, dependencies }) {
    return new DocumentSync(selected, dependencies.source, dependencies.destination);
  },
  providers: [
    provider(syncDescriptor, (instance: DocumentSync) => bindDocumentSyncProvider(instance)),
  ],
  stop(instance) {
    instance.stop();
  },
});
