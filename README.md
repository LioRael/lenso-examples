# Lenso vNext Examples

Executable, product-shaped examples for the current Lenso architecture. This
repository contains Capability contracts and Plugin compositions; Kernel,
Driver, Adapter, Runner, and Host mechanics remain in their owning
repositories.

The source was extracted from `LioRael/lenso` at monorepo commit
`67d21499548d07e92c2f6529d7c8345e58c067d9` under ADR 0064. Imported subtrees
retain their relevant Git history and consume released Lenso packages through
versioned dependencies.

## Current examples

`crates/lenso-capability-*` contains runtime-neutral Capability descriptors and
generated Rust bindings for:

- greeting, secure greeting, secrets, and durable counters;
- story query and story events;
- Web Shell and UI Contribution;
- game sessions;
- agent orchestration, model, tool, memory, and progress boundaries.

`fixtures/vnext-*` contains executable Plugins and Apps that prove those
contracts:

- `vnext-native-greeter`: a minimal native Provider;
- `vnext-stateful-plugin`: Plugin-owned durable state, setup, recovery, and
  upgrade;
- `vnext-story-plugin`: typed Story query/event behavior with durable state;
- `vnext-web-ui`: target-owned Web Shell, Browser Adapter, and UI Contribution
  composition;
- `vnext-agent-harness`: replaceable model/tool providers, durable memory, and
  progress events;
- `vnext-plugin-authoring-v2`: one Plugin Contract with Rust Process and
  TypeScript Bun implementations, typed configuration, and two named
  dependencies;
- `vnext-plugin-authoring-v2/agent`: an Agent-owned Tool Provider that consumes
  the same document-sync Capability through the Rust product SDK;
- `vnext-game-session`: bounded real-time session behavior and protocol
  conformance.

The Bun Adapter conformance suite remains in
[`LioRael/lenso-bun-adapter`](https://github.com/LioRael/lenso-bun-adapter).
The author-facing cross-language example lives here and consumes only published
packages.

## Validation

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo test --locked --workspace
cargo check --locked --manifest-path fixtures/vnext-plugin-authoring-v2/Cargo.toml \
  --workspace --all-targets
cargo test --locked --manifest-path fixtures/vnext-plugin-authoring-v2/Cargo.toml \
  --workspace
cargo test --locked -p lenso-vnext-web-ui --test web_ui \
  generated_browser_client_invokes_the_allowlisted_app_capability \
  -- --ignored --test-threads=1
```

These checks are also the complete CI surface. Historical Service Kit, Node
Provider, launchpad, Support Desk, and milestone acceptance examples have been
removed rather than retained as alternate public lifecycles.
