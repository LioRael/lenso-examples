# Hello Action Remote Module

This is a starter remote module package for Lenso. It uses the published
`@lenso/remote-module-kit` and `@lenso/ts-sdk` packages, so it can run without
a sibling framework checkout.

It exposes:

- a manifest at `/lenso/module/v1/manifest`;
- two HTTP routes, `GET /hello/{name}` and `POST /greetings`;
- two runtime functions, `hello-action.say-hello.v1` and
  `hello-action.record-greeting.v1`;
- one declarative admin surface with a `seed_greeting` action and a fallback
  `greetings` schema-admin entity.

## Local Development

Install dependencies from the repository root:

```sh
pnpm install
```

Run the module server:

```sh
pnpm dev
```

The server reads `PORT` from the shell environment. The default
`.env.example` value is:

```text
PORT=4100
```

The server prints a manifest URL:

```text
http://127.0.0.1:4100/lenso/module/v1/manifest
```

Run the non-interactive smoke:

```sh
pnpm smoke
```

Change the starter by editing:

- `src/module.mjs` for manifest declarations, handlers, and seed data;
- `src/server.mjs` for local startup behavior;
- `src/smoke.mjs` for executable expectations;
- `catalog-entry.json` for optional discovery metadata.

`POST /greetings`, `hello-action.record-greeting.v1`, and the
`seed_greeting` admin action all append to the same in-memory greeting list.
The fallback schema-admin `greetings` entity then shows the new records, which
makes the example useful for testing write paths without adding a database.

## Install Into A Lenso Host

From a local Lenso host checkout, install the running module:

```sh
lenso module add http://127.0.0.1:4100/lenso/module/v1/manifest
lenso console-package apply-plan
pnpm install
```

This example does not publish a Runtime Console package, so the generated
install plan should not request frontend dependencies. It still exercises the
same manifest install path as a fuller third-party module.

The optional catalog record is `catalog-entry.json`; it mirrors the local
server's default manifest URL for discovery flows.

## Host Install Smoke

Run the host-side integration smoke from this package:

```sh
pnpm host-smoke
```

It starts this module, creates a temporary host repo, runs the real `lenso`
CLI, and verifies:

- `.lenso/module-catalog.json` from `lenso module catalog add`;
- `.env` from `lenso module add`;
- `.lenso/console-package-install-plan.json` with zero console packages.

By default the smoke uses a sibling `../lenso-runtime-console` checkout. Set
`LENSO_RUNTIME_CONSOLE_DIR=/path/to/lenso-runtime-console` to use another
checkout. Set `LENSO_KEEP_HOST_SMOKE=1` to keep the temporary host repo for
inspection.

## Host Proxy Run

For the full local host path, including `just api` and a `curl` request to
`/modules/hello-action/http/greetings`, see
[../../docs/hello-action-host-run.md](../../docs/hello-action-host-run.md).
