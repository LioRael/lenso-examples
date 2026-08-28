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
- `vnext-game-session`: bounded real-time session behavior and protocol
  conformance.

The Bun Plugin SDK and its executable generated-Provider golden path live in
[`LioRael/lenso-bun-adapter`](https://github.com/LioRael/lenso-bun-adapter),
next to the Execution Adapter conformance suite. A standalone Bun example can
join this repository after the SDK and provider codegen releases are publicly
installable; this repository does not pin unpublished or local-only packages.

## Validation

```sh
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo test --locked --workspace
cargo test --locked -p lenso-vnext-web-ui --test web_ui \
  generated_browser_client_invokes_the_allowlisted_app_capability \
  -- --ignored --test-threads=1
```

These checks are also the complete CI surface. Historical Service Kit, Node
Provider, launchpad, Support Desk, and milestone acceptance examples have been
removed rather than retained as alternate public lifecycles.
