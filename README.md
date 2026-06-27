# Lenso Examples

Runnable examples for Lenso module authors.

This repository uses published packages instead of sibling workspace paths:

- `lenso`
- `@lenso/service-kit`
- `@lenso/remote-module-kit` for the gRPC legacy transport example

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

Install its manifest into a local Lenso host:

```sh
lenso service install http://127.0.0.1:4110/lenso/service/v1/manifest
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
