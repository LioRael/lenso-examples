# Lenso Examples

This repository also owns the vNext example Capabilities and executable Apps
extracted from `LioRael/lenso` at monorepo commit
`67d21499548d07e92c2f6529d7c8345e58c067d9` under ADR 0064. Their imported
subtrees retain relevant Git history and consume released core, runtime,
protocol, Auth, and authoring packages through versioned dependencies.

Runnable examples for Lenso module authors.

This repository uses published packages for released contracts where available,
including `lenso` and the framework-owned `@lenso/service-kit`.

Standard TypeScript Service examples consume the published Service Kit. Pinned
integration-set runs override it with the exact Framework source selected by
the integration set. The Rust V8 proofs still use sibling path dependencies:
`examples/rust-service` and `examples/support-system` point at `../lenso`
crates.

For local V8 verification, clone the matching V8 branches next to each other:

```sh
git clone https://github.com/LioRael/lenso-examples.git
git clone https://github.com/LioRael/lenso.git
cd lenso-examples
pnpm install
```

## Quick Start

Install dependencies and run the standard Provider example smokes:

```sh
pnpm install
pnpm smoke
```

The communicating Autonomous Services proof is opt-in because it also requires
the System Sandbox CLI and the sibling `lenso` checkout; use
`pnpm smoke:support-system` for that proof.

## Support Desk application acceptance

The product-level acceptance starts from one exact `lenso.app.json`, runs the
Support Ticket Provider and Local Control Adapter with `lenso system dev`,
connects the signed System through Console, and exercises the reviewed
`console_ui_esm` Support Ticket and Story surfaces in a real browser.

From this repository root, the materialization command proven by the runner is:

```sh
lenso app compose ./support-desk \
  --blueprint support-desk \
  --pack ./fixtures/acceptance/support-desk/capability \
  --implementation support-api=linked \
  --implementation notification-worker=linked \
  --implementation lenso/platform-story=linked \
  --apply
```

The resulting document must exactly match
`fixtures/acceptance/support-desk/lenso.app.json`. The explicit `--apply` flag
only atomically materializes that composition; it is not a separate product
lifecycle or a deployment operation.

Run the complete proof with sibling Framework, CLI, and Console checkouts:

```sh
pnpm install --frozen-lockfile
pnpm acceptance:support-desk
```

The runner accepts optional `LENSO_FRAMEWORK_ROOT`, `LENSO_CLI_ROOT`,
`LENSO_CONSOLE_ROOT`, and `LENSO_CLI_BIN` overrides. Otherwise it resolves
`../lenso`, `../lenso-cli`, and `../lenso-console`. It starts a disposable local
PostgreSQL instance when `LENSO_ACCEPTANCE_DATABASE_URL` is absent; an explicit
URL must name an acceptance database. Node, pnpm, Cargo, `initdb`, `postgres`,
`pg_isready`, and a headless Chrome are the local prerequisites. Cargo remains
required even with `LENSO_CLI_BIN`, because
the runner uses the Console repository's Cargo-backed migration and service
entrypoints.

The proof uses only the public CLI and authenticated HTTP boundaries. It never
seeds or queries Console tables directly, never sends Adapter URLs, bearer
tokens, or signing keys for managed authorities to the browser, and never
publishes or deploys an artifact. The browser signs in through the public
Console password flow and receives only its ordinary Console session; no
compile-time development bearer is injected. Set
`LENSO_ACCEPTANCE_KEEP_TEMP=1` only when local screenshots and process evidence
should be retained for inspection.

The regular `pnpm check` keeps the fast composition/contract and Provider gates
in CI. The cross-repository process and browser proof remains the explicit
`pnpm acceptance:support-desk` command.

## Blank Host Starter

Use the standalone CLI when you want a blank Rust host before installing
services:

```sh
cargo install lenso-cli
lenso host init ../my-lenso-host
cd ../my-lenso-host
cp .env.example .env
lenso serve
```

The starter serves `GET /v1/app/status`, `GET`/`POST /v1/app/items`,
`/openapi.json`, and the admin APIs.
Keep this repository for runnable module examples; the host starter template is
owned by the standalone `lenso-cli` repository.

## Examples

### vNext target-owned Web UI

[`fixtures/vnext-web-ui`](fixtures/vnext-web-ui) is the runnable Rust proof for
a target-owned Web Shell, UI Contribution, and Browser Adapter. Its Axum/Tower
ingress stays behind the Browser Adapter and projects only an explicitly bound
Capability through a generated browser client.

```sh
cargo test --locked -p lenso-vnext-web-ui --test web_ui
```

See the [fixture README](fixtures/vnext-web-ui/README.md) for ownership,
middleware scope, and the Bun-backed browser-client conformance command.

### Communicating Support Services

`examples/support-system` extracts the `support-ticket` and `support-sla`
Modules into separate Autonomous Services. The clusterless System Sandbox
starts API, Worker, and Migration Workloads with isolated Service Stores. A
generated HTTP client calls support-ticket, which calls support-sla directly
through the generated gRPC client without a Host or Provider in the Data Plane.
The same support workflow publishes `support.ticket-opened.v1` transactionally,
authenticates its Service Principal and delegated tenant context at support-sla,
and commits the SLA business effect through a durable Inbox.

The current product proof for Support Desk is the application acceptance
described above:

```sh
pnpm acceptance:support-desk
```

It owns the current Compose → Run locally → signed Connect → Status lifecycle.
The historical `acceptance:m1` through `acceptance:m6`, support-system fixtures,
and direct Provider smokes remain compatibility and regression inputs; they are
not alternate public lifecycles or authoritative product proofs. For targeted
compatibility checks, run:

```sh
pnpm smoke:support-system:contract
pnpm smoke:support-system
pnpm smoke:support-ticket
```

See [`examples/support-system/README.md`](examples/support-system/README.md)
for the identities, evidence, and local CLI override.

### Rust Manifest

`examples/rust-manifest` is a minimal Rust package using the published
`lenso` facade. It declares a schema-admin manifest, runs manifest lints, and
prints the manifest JSON:

```sh
pnpm rust-manifest
```

### Rust Service Provider

`examples/rust-service` is a standalone Axum service provider. It exposes the
`rust-audit-log` module through a service manifest, status endpoint, module
manifest endpoint, and a direct HTTP route:

```sh
pnpm start:rust-service
```

Install its manifest into a local Lenso host:

```sh
lenso service install http://127.0.0.1:4130/lenso/service/v1/manifest
```

Print the manifest without starting the server:

```sh
pnpm rust-service:check
```

With the service running, emit a service package plus module release artifacts
from its manifest URL:

```sh
pnpm service-package:rust-service
```

Then install the package artifact:

```sh
lenso service install dist/lenso-service/rust-audit-service/lenso.service-package.json \
  --base-url http://127.0.0.1:4130/lenso/service/v1
```

The example README includes the matching `lenso service verify`, install,
upgrade-plan, rollback preview, and deployment export commands.

The Rust and TypeScript examples intentionally expose the same service contract
shape: a service process provides one or more independently installed modules,
while the Host owns auth, runtime queues, retries, outbox, and observability.
V12 adds `lenso.workspace.json` at the repo root so the examples can also be
treated as one local service workspace:

```sh
lenso service workspace list --workspace-file lenso.workspace.json
lenso service dev --workspace-file ../lenso-examples/lenso.workspace.json
```

Use the second command from a generated host repo when you want the host and
example services to start together. After the services are running,
`workspace check` verifies each example service directory, manifest, and status
endpoint:

```sh
lenso service workspace check --workspace-file lenso.workspace.json
```

From a generated host repo, install a workspace provider by name:

```sh
lenso service install support-suite-provider --workspace-file ../lenso-examples/lenso.workspace.json
```

Or export the whole example workspace into the host service-start state shape:

```sh
lenso service workspace export \
  --workspace-file ../lenso-examples/lenso.workspace.json \
  --output .lenso/module-services.json
```

The current application-model path intentionally has one lifecycle:

1. Compose the exact `lenso.app.json`.
2. Run it locally with `lenso system dev`.
3. Connect it through signed Console enrollment.
4. Read System, Service, Surface, Story, and Workload status in Console.

`pnpm acceptance:support-desk` is the executable reference for that lifecycle.
`lenso.system.json` and the older system-state, release, and runbook fixtures
remain compatibility-test inputs; they are not public product commands or
Console deployment controls.

### Hello Action Service

`examples/hello-action` is a starter service provider. It exposes:

- a service manifest at `/lenso/service/v1/manifest`;
- service status at `/lenso/service/v1/status`;
- the `hello-action` module below `/lenso/service/v1/modules/hello-action`;
- two HTTP routes, `GET /hello/{name}` and `POST /greetings`;
- two runtime functions, `hello-action.say-hello.v1` and
  `hello-action.record-greeting.v1`;
- one declarative admin surface with a `seed_greeting` action and a fallback
  `greetings` schema-admin entity.

Start the module from the repository root:

```sh
pnpm start:hello-action
```

Or work inside the example package directly:

```sh
cd examples/hello-action
pnpm dev
pnpm smoke
```

Change the module by editing:

- `src/module.ts` for the manifest, handlers, and seed data;
- `src/server.ts` for local startup behavior;
- `src/smoke.ts` for executable expectations as the module grows;
- `catalog-entry.json` for optional discovery metadata.

The server prints a manifest URL like:

```text
http://127.0.0.1:4100/lenso/service/v1/manifest
```

Use that URL with a local Lenso host checkout:

```sh
lenso service install http://127.0.0.1:4100/lenso/service/v1/manifest
```

The example does not ship a Runtime Console package, so there is no frontend
package install step for this module.

The server reads `PORT` from the shell environment. The optional discovery
record lives at `examples/hello-action/catalog-entry.json` and matches the
default `PORT=4100` documented in `examples/hello-action/.env.example`.

To verify the host-side install path without mutating a real Lenso checkout,
run the integration smoke from this repository root:

```sh
pnpm host-smoke
```

It starts the service examples, creates temporary host repos, runs the real
`lenso module catalog add` and `lenso service install` commands, and checks the
generated `.lenso/module-catalog.json`, `.env`, and install receipts.

To run the example through a real Host API and call its Service HTTP route via
`/modules/hello-action/http/greetings`, follow
[docs/hello-action-host-run.md](docs/hello-action-host-run.md).

### Account Profile Service

`examples/account-profile` keeps product profile data outside the first-party
auth anchor. The service provider is `account-profile-service`; it provides the
`account-profile` module with an `auth` dependency, profile records,
organizations, memberships, HTTP routes, an admin action, and schema-admin
pages.

Start it from the repository root:

```sh
pnpm start:account-profile
```

Smoke the module directly:

```sh
pnpm smoke:account-profile
```

Install its manifest into a local Lenso host:

```sh
lenso service install http://127.0.0.1:4120/lenso/service/v1/manifest
```

### Support Ticket Service

`examples/support-ticket` is the agent-ready service demo. It turns a concrete
business prompt into an independently running service that provides the
`support-ticket` module with tickets data, HTTP routes, an admin action, a
runtime escalation function, its exact Business API contract, and its
Module-owned `console_ui_esm` Surface:

```text
Build a support ticket module for a Lenso app.
```

Run it from the repository root:

```sh
pnpm start:support-ticket
```

Smoke the module directly:

```sh
pnpm smoke:support-ticket
```

Build the receipt-bound Console artifact from the same repository and bind it
to an exact Module Release digest:

```sh
LENSO_SUPPORT_TICKET_MODULE_RELEASE_DIGEST='sha256:<64-hex>' \
  pnpm build:console-artifact:support-ticket
```

The supported product proof is `pnpm acceptance:support-desk`. It composes the
exact App, runs it locally, performs signed Console enrollment and connection,
then exercises the generated client and real browser. Console observes runtime
status; it does not install, deploy, upgrade, or roll back the Provider.

For lower-level service/Host compatibility, run:

```sh
pnpm host-api-smoke:support-ticket
```

That compatibility smoke starts the service, installs it into a temporary Host,
exercises the host-owned HTTP proxy and runtime path for its module, installs
the first-party `audit-log` module by name, and verifies both support-ticket and
Audit Events Data Surfaces. The smoke checks co-install visibility;
support-ticket records do not create audit rows unless a module calls the
audit-log writer API. It is not an alternative application lifecycle.
For the manual walkthrough, see
[docs/support-ticket-service-module-run.md](docs/support-ticket-service-module-run.md).

## Repositories

- Backend framework: https://github.com/LioRael/lenso
- Console Service: https://github.com/LioRael/lenso-console
