# Document sync Plugin

This fixture proves one Plugin Contract with two replaceable implementations:

- `src/bin/document_sync_process.rs` uses the Rust Process SDK;
- `typescript/src/plugin.ts` uses the generic TypeScript Plugin SDK and Bun;
- both publish the same configuration schema, `document-sync` Capability, and
  named `source` and `destination` document-store dependencies.

The standalone workspace also contains the two Capability crates under
`capabilities/` and an Agent-owned Rust Tool Provider under `agent/`. Keeping
this current runtime generation separate lets the repository retain older
examples without linking incompatible pre-1.0 runtime ABIs into one graph.

The Host selects one implementation from the packaged Bundle. Plugin code does
not inspect a generation, choose provider instances, or implement transport
framing.

```sh
cd typescript
bun install --frozen-lockfile
cd ..
cargo test --locked --workspace
lenso plugin check
lenso plugin pack --output document-sync.lenso-plugin
```

`plugin check` builds both implementations and rejects the project unless their
Contracts are identical. `plugin pack` produces one Bundle 4 archive containing
the native Process executable and portable Bun artifact.

The ordinary Process integration test runs with the workspace suite. To run the
Bun Host proof, extract `implementations/typescript-bun/plugin.js` from the
Bundle and pass its path explicitly:

```sh
LENSO_DOCUMENT_SYNC_BUN_ARTIFACT=/absolute/path/plugin.js \
  cargo test -p lenso-vnext-plugin-authoring-v2 --test process_sync \
  typescript_bun_syncs_through_the_same_named_host_dependencies -- --ignored
```
