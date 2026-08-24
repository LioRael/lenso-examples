# AGENTS.md

Guidance for coding agents working in this repository.

The `crates/lenso-capability-*` and `fixtures/vnext-*` trees own example and
product-shaped vNext behavior. Keep Kernel mechanics and host runtime
implementations in their owning repositories; use versioned dependencies.

## Agent skills

### Issue tracker

Issues and PRDs are tracked in the central `LioRael/lenso` GitHub repository. See `docs/agents/issue-tracker.md`.

### Triage labels

Triage uses the five canonical labels in the central tracker. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses a single-context layout. See `docs/agents/domain.md`.

### Capability projections

Native Capability crates own only their generated Rust projection. A fixture
that executes TypeScript owns its projection under that fixture instead of
vendoring it beside the native crate.
