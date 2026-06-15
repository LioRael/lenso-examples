# Lenso Examples

Runnable examples for Lenso module authors and API consumers.

This repository uses published packages instead of sibling workspace paths:

- `lenso`
- `@lenso/remote-module-kit`
- `@lenso/ts-sdk`

## Quick Start

Install dependencies and run the full example smoke:

```sh
pnpm install
pnpm smoke
```

## Examples

### Rust Manifest

`examples/rust-manifest` is a minimal Rust package using the published
`lenso` facade. It declares a schema-admin manifest, runs manifest lints, and
prints the manifest JSON:

```sh
pnpm rust-manifest
```

### Hello Action Remote Module

`examples/hello-action` is a starter remote module package. It exposes:

- a manifest at `/lenso/module/v1/manifest`;
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

- `src/module.mjs` for the manifest, handlers, and seed data;
- `src/server.mjs` for local startup behavior;
- `src/smoke.mjs` for executable expectations as the module grows;
- `catalog-entry.json` for optional discovery metadata.

The server prints a manifest URL like:

```text
http://127.0.0.1:4100/lenso/module/v1/manifest
```

Use that URL with a local Lenso host checkout:

```sh
lenso module add http://127.0.0.1:4100/lenso/module/v1/manifest
lenso console-package apply-plan
pnpm install
```

The example does not ship a Runtime Console package, so `apply-plan` is still
safe to run and should leave no frontend package to install for this module.

The server reads `PORT` from the shell environment. The optional discovery
record lives at `examples/hello-action/catalog-entry.json` and matches the
default `PORT=4100` documented in `examples/hello-action/.env.example`.

To verify the host-side install path without mutating a real Lenso checkout,
run the integration smoke from this repository root:

```sh
pnpm host-smoke
```

It starts `hello-action`, creates a temporary host repo, runs the real
`lenso module catalog add` and `lenso module add` commands, and checks the
generated `.lenso/module-catalog.json`, `.env`, and console package install
plan. By default it uses a sibling `../lenso-runtime-console` checkout; set
`LENSO_RUNTIME_CONSOLE_DIR=/path/to/lenso-runtime-console` to use another one.

To run the example through a real host API and call its remote HTTP route via
`/modules/hello-action/http/greetings`, follow
[docs/hello-action-host-run.md](docs/hello-action-host-run.md).

## Repositories

- Backend framework: https://github.com/LioRael/lenso
- Runtime Console and remote module kit: https://github.com/LioRael/lenso-runtime-console
