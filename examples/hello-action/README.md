# Hello Action Service

This starter service provider uses `@lenso/service-kit`. The service is named
`hello-service`; it provides the `hello-action` module.

It exposes:

- a service manifest at `/lenso/service/v1/manifest`;
- service status at `/lenso/service/v1/status`;
- the `hello-action` module below `/lenso/service/v1/modules/hello-action`;
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

Run the service:

```sh
pnpm dev
```

The server reads `PORT` from the shell environment. The default `.env.example`
value is:

```text
PORT=4100
```

The server prints:

```text
http://127.0.0.1:4100/lenso/service/v1/manifest
http://127.0.0.1:4100/lenso/service/v1/status
```

Run the non-interactive smoke:

```sh
pnpm smoke
```

## Install Into A Lenso Host

From a local Lenso host checkout, install the running service:

```sh
lenso service install http://127.0.0.1:4100/lenso/service/v1/manifest
```

This example does not publish a Runtime Console package, so the generated
install receipt should not request frontend dependencies.

The optional catalog record is `catalog-entry.json`; it mirrors the local
server's default service manifest URL for discovery flows.

## Host Install Smoke

Run the host-side integration smoke from this package:

```sh
pnpm host-smoke
```

It starts this service, creates a temporary host repo, runs the real `lenso`
CLI, and verifies:

- `.lenso/module-catalog.json` from `lenso module catalog add`;
- `.env` from `lenso service install`;
- `.lenso/module-installs.json` records `hello-action` as a module provided by
  `hello-service`;
- `.lenso/console-package-install-plan.json` with zero console packages.

Set `LENSO_KEEP_HOST_SMOKE=1` to keep the temporary host repo for inspection.

## Host Proxy Run

For the full local host path, including `just api` and a `curl` request to
`/modules/hello-action/http/greetings`, see
[../../docs/hello-action-host-run.md](../../docs/hello-action-host-run.md).
