# Plugin authoring delivery acceptance

This fixture is the product-level proof for the first Plugin authoring v2
delivery. It keeps product behavior here and points to the owning repositories
for transport, lifecycle, selection, and cleanup mechanics.

## Released inputs

The standalone Cargo and Bun lockfiles are the executable authority. The
selected public packages are:

| Owner | Package | Version |
| --- | --- | --- |
| Core | `lenso-app-plan` | `0.4.1` |
| Runtime | `lenso-kernel` | `0.3.4` |
| Runtime | `lenso-native-adapter` | `0.3.9` |
| Runtime | `lenso-process-adapter` | `0.3.5` |
| Runtime | `lenso-plugin-sdk` | `0.4.3` |
| Runtime | `lenso-runtime-codec` | `0.3.2` |
| Bun | `lenso-bun-adapter` | `0.1.8` |
| Bun | `@lenso/bun-plugin` | `0.2.2` |
| Agent | `lenso-agent-tool-sdk` | `0.3.2` |
| Agent | `lenso-capability-agent-tool-provider` | `0.2.2` |
| CLI | `lenso-cli` | `0.5.1` |
| CLI | `@lenso/cli` | `0.16.1` |

## Reproducible path

Run from this directory. `HOST_TARGET` is `aarch64-apple-darwin` for the
current Rust Process artifact; use `javascript-bun` to select the portable Bun
artifact.

```sh
bun install --cwd typescript --frozen-lockfile
cargo test --locked --workspace
npx --yes @lenso/cli@0.16.1 plugin check --repo-root . --json
npx --yes @lenso/cli@0.16.1 plugin pack --repo-root . --output document-sync.lenso-plugin --json

HOST_TARGET=aarch64-apple-darwin
npx --yes @lenso/cli@0.16.1 app build \
  --source acceptance/host.ts --target "$HOST_TARGET" --out /tmp/document-sync-host
npx --yes @lenso/cli@0.16.1 plugins add document-sync.lenso-plugin --root /tmp/document-sync-host
npx --yes @lenso/cli@0.16.1 app show --root /tmp/document-sync-host --json
npx --yes @lenso/cli@0.16.1 plugins remove example.document-sync --root /tmp/document-sync-host
npx --yes @lenso/cli@0.16.1 app check --root /tmp/document-sync-host
```

Extract `implementations/typescript-bun/plugin.js` from the produced Bundle and
run the ignored Bun test as documented in the README. Both runtime tests use
the same `identity-v1` configuration, generated Capability contracts, named
bindings, and Rust Native Store factories. Kernel shutdown is part of every
successful case and therefore exercises the selected implementation's stop
path.

## Evidence map

- **PROOF-01:** `plugin check` and `plugin pack` create one Bundle 4 containing
  the Rust Process and Bun artifacts. The Host build/add/show/remove/check path
  independently verifies the incoming archive. `process_sync.rs` invokes each
  selected implementation and shuts down its real child.
- **PROOF-02:** `process_sync.rs` spies on reads and writes for distinct Store
  accounts, swaps the two named bindings without changing Plugin code, and
  binds both names to one exact Store instance. Saved optional absence and new
  candidate preservation are owned by the CLI selection transaction delivered
  in [lenso-cli#324](https://github.com/LioRael/lenso-cli/pull/324).
- **PROOF-03:** `process_sync.rs` runs the linked Agent ToolProvider `catalog`
  and `execute` endpoints through a selected `DocumentSyncClient`; the Store
  spies prove nested dispatch and the write. `updated` is the normal tool result and
  `already_running` remains a domain guard inside both implementations. Host
  admission is the external concurrency authority.
- **PROOF-04:** complete-object construction, failed and late constructors,
  cancellation after committed work, ignored cancellation, failed stop, and
  cleanup ownership are exercised by the released Runtime cohort at
  [`dc586a5`](https://github.com/LioRael/lenso-runtime-rust/tree/dc586a5).
  Bun callback/control progress while another dependency call is blocked is
  covered by [lenso-bun-adapter#29](https://github.com/LioRael/lenso-bun-adapter/pull/29).
- **PROOF-05:** `process_sync.rs` rejects an altered artifact digest offline and
  rejects an unsupported Process profile before readiness or Store access.
  Bundle compatibility and lifecycle-only providers are covered by the Runtime
  release above. The lockfiles and released-input table constrain this fixture
  to public artifacts and actual supported targets.

The proof intentionally does not create another storage framework or claim a
language-by-runtime matrix. Store behavior is private fixture code; Capability
contracts, Host choices, and execution adapters remain separate owners.
