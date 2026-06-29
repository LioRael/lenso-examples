# Lenso Examples

Runnable examples for Lenso module authors.

This repository uses published packages for released contracts where available:

- `lenso`
- `@lenso/remote-module-kit` for the gRPC legacy transport example

The V8 proofs currently have two temporary sibling-repository dependencies
until the matching contracts are published:

- TypeScript service examples resolve `@lenso/service-kit` and the V8
  `@lenso/remote-module-kit` build from `../lenso-runtime-console`.
- `examples/rust-service` points at the sibling `../lenso` crate.

For local V8 verification, clone the matching V8 branches next to each other:

```sh
git clone --branch chore/open-source-hygiene https://github.com/LioRael/lenso-examples.git
git clone --branch chore/open-source-hygiene https://github.com/LioRael/lenso-runtime-console.git
git clone --branch chore/open-source-hygiene https://github.com/LioRael/lenso.git
pnpm --dir lenso-runtime-console install
pnpm --dir lenso-runtime-console --filter @lenso/remote-module-kit build
pnpm --dir lenso-runtime-console --filter @lenso/service-kit build
cd lenso-examples
pnpm install
```

## Quick Start

Install dependencies and run the full example smoke:

```sh
pnpm install
pnpm smoke
```

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

The example README includes the matching `lenso service check`, install, diff,
upgrade preview, rollback preview, and deployment export commands.

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

V11 examples keep `lenso.module.v1` module contracts next to
`lenso.module-release.v1` release artifacts so module install remains the
business-capability entrypoint and service install remains the provider/process
entrypoint.
The V8 proof path uses service operation metadata and checks across both TS
services and the Rust Axum provider, including safe HTTP probes and runtime
function declarations.

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

To run the example through a real host API and call its remote HTTP route via
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

### gRPC Notes Legacy Transport

`examples/grpc-notes` is the native gRPC compatibility example for the older
remote-module transport. It exposes a module manifest over gRPC, schema-admin
`notes`, `GET /notes` through the host proxy, and `grpc-notes.summarize.v1`
through the runtime function lane. The V5 HTTP service manifest path is shown
by the HTTP examples above.

Start it from the repository root:

```sh
pnpm start:grpc-notes
```

Verify the gRPC protocol path:

```sh
pnpm smoke:grpc-notes
```

Install it into a local host with the checked-in manifest and a gRPC base URL:

```sh
lenso module catalog add ../lenso-examples/examples/grpc-notes/lenso.module.json --base-url grpc://127.0.0.1:50051 --summary "Native gRPC notes module"
lenso module add ../lenso-examples/examples/grpc-notes/lenso.module.json --base-url grpc://127.0.0.1:50051
lenso console package apply-plan
```

### Support Ticket Service

`examples/support-ticket` is the agent-ready service demo. It turns a concrete
business prompt into an independently running service that provides the
`support-ticket` module with tickets data, HTTP routes, an admin action, a
runtime escalation function, and Console-visible metadata:

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

With the service running, emit a V9 service package artifact from its manifest
URL:

```sh
pnpm service-package:support-ticket
```

Then install the package artifact:

```sh
lenso service install dist/lenso-service/support-suite-provider/lenso.service-package.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
```

Install the business module through the generated V10 module release artifact:

```sh
lenso module release inspect dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json
lenso module release check dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
lenso module install dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
```

Or add that release to the local catalog and install by module name:

```sh
lenso module catalog add dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
lenso module install support-ticket
```

Run the full service host path:

```sh
pnpm host-api-smoke:support-ticket
```

That starts the service, installs it into a temporary host, exercises the
host-owned HTTP proxy and runtime path for its module, and verifies Runtime
Story evidence.
For the manual walkthrough, see
[docs/support-ticket-service-module-run.md](docs/support-ticket-service-module-run.md).

## Repositories

- Backend framework: https://github.com/LioRael/lenso
- Runtime Console and service kit: https://github.com/LioRael/lenso-runtime-console
