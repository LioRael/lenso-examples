# Document sync Plugin

This fixture proves one Plugin Contract with two replaceable implementations:

- `src/bin/document_sync_process.rs` uses the Rust Process SDK;
- `typescript/src/plugin.ts` uses the generic TypeScript Plugin SDK and Bun;
- both publish the same configuration schema, `document-sync` Capability, and
  named `source` and `destination` document-store dependencies.

The standalone workspace also contains the two Capability crates under
`capabilities/` and an Agent-owned Rust Tool Provider under `agent/`. The Host
tests invoke that Tool Provider, which preserves the invocation context while
calling the selected document-sync implementation. Keeping this current runtime
generation separate lets the repository retain older examples without linking
incompatible pre-1.0 runtime ABIs into one graph.

The Host selects one implementation from the packaged Bundle. Plugin code does
not inspect a generation, choose provider instances, or implement transport
framing.

[`ACCEPTANCE.md`](ACCEPTANCE.md) records the released dependency set, complete
installation path, and the owner-local evidence for lifecycle and failure
cases.

```sh
cd typescript
bun install --frozen-lockfile
cd ..
cargo test --locked --workspace
lenso plugin check
lenso plugin pack
```

`plugin check` builds both implementations and rejects the project unless their
Contracts are identical. `plugin pack` produces one Bundle 4 archive containing
the Process executable and Bun artifact. The locked authoring set uses
`lenso-cli 0.5.0`, `lenso-plugin-sdk 0.4.3`,
`lenso-agent-tool-sdk 0.3.2`, `lenso-process-adapter 0.3.5`,
`lenso-bun-adapter 0.1.7`, and `@lenso/bun-plugin 0.2.2`.

The repository CI also exercises the ordinary App commands with
`host-catalog.json`: `lenso plugins add`, `lenso plugins configure`, and
`lenso plugins list`.

The ordinary Process integration test runs with the workspace suite. It calls
Agent ToolProvider `catalog` and `execute`, observes one Store read and one
Store write, and shuts the App down cleanly. To run the equivalent Bun Host
proof, extract `implementations/typescript-bun/plugin.js` from the Bundle and
pass its path explicitly:

```sh
LENSO_DOCUMENT_SYNC_BUN_ARTIFACT=/absolute/path/plugin.js \
  cargo test -p lenso-vnext-plugin-authoring-v2 --test process_sync \
  typescript_bun_syncs_through_the_same_named_host_dependencies -- --ignored
```
