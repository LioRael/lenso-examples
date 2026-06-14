# Hello Action Host Run

Run this guide when you want to verify the `hello-action` example through a
real Lenso host API instead of calling the module server directly.

## Prerequisites

Keep these repositories checked out as siblings:

```text
framework/
  lenso/
  lenso-examples/
  lenso-runtime-console/
```

The host commands below assume the backend repository's local defaults:

- API on `http://127.0.0.1:3000`;
- `APP_ENV=local`, so development bearer tokens are accepted;
- the `lenso` CLI is available through the sibling Runtime Console checkout.

If `lenso` is already on your `PATH`, you can use `lenso ...` instead of
`pnpm --dir ../lenso-runtime-console exec lenso ...`.

## Start The Remote Module

From `lenso-examples`:

```sh
pnpm install
pnpm start:hello-action
```

The module prints:

```text
Hello Action manifest: http://127.0.0.1:4100/lenso/module/v1/manifest
```

Keep this process running. The example stores greetings in memory, so restarting
the module clears records created during this guide.

## Install Into The Host

From the sibling `lenso` backend checkout:

```sh
test -f .env || cp .env.example .env
pnpm --dir ../lenso-runtime-console exec lenso module add \
  http://127.0.0.1:4100/lenso/module/v1/manifest
```

`hello-action` does not publish a Runtime Console package, so no frontend
dependency install is required for this module. If the API is already running,
restart it after `module add`; `REMOTE_MODULES` is loaded on startup.

Start the host services:

```sh
just db-up
just migrate
just api
```

Start `just worker` and `just console-api` in separate shells if you also want
background runtime processing and the Runtime Console.

## Call Through The Host Proxy

Create a greeting through the host-owned remote HTTP proxy:

```sh
curl -sS -X POST http://127.0.0.1:3000/modules/hello-action/http/greetings \
  -H 'authorization: Bearer dev-service:admin:hello-action:greetings:write' \
  -H 'content-type: application/json' \
  -H 'x-correlation-id: corr_hello_action_host_run' \
  -d '{
    "recipient": "host-proxy-user",
    "message": "Hello through the Lenso host proxy.",
    "sent_at": "2026-06-14T03:00:00Z"
  }' | jq .
```

If `jq` is not installed, drop the `| jq .` suffix.

Expected shape:

```json
{
  "status": "forwarded",
  "module_name": "hello-action",
  "method": "POST",
  "declared_path": "/greetings",
  "remote_path": "/greetings",
  "capability": "hello-action:greetings:write",
  "path_params": {},
  "data": {
    "record": {
      "id": "greeting_2",
      "recipient": "host-proxy-user",
      "message": "Hello through the Lenso host proxy.",
      "sent_at": "2026-06-14T03:00:00Z"
    }
  }
}
```

Read the same module state back through schema-admin:

```sh
curl -sS 'http://127.0.0.1:3000/admin/data/hello-action/greetings?limit=10' \
  -H 'authorization: Bearer dev-service:admin' | jq '.data'
```

The response should include both the seed record and `host-proxy-user`.

## Inspect Operations

Remote proxy calls are host-owned operational data. Query the call by the same
correlation id:

```sh
curl -sS \
  'http://127.0.0.1:3000/admin/runtime/remote-proxy-calls?correlation_id=corr_hello_action_host_run&limit=10' \
  -H 'authorization: Bearer dev-service:admin' | jq '.data[0]'
```

In Runtime Console, the same call appears in Remote Calls and in the Runtime
Story for `corr_hello_action_host_run`.

## Troubleshooting

- `401`: the request is missing an `authorization` header.
- `403`: the service token is missing `hello-action:greetings:write`.
- `404`: the API was started before `REMOTE_MODULES` was updated, or the module
  process is not running.
- `400 validation_failed`: the proxy write request is missing
  `content-type: application/json` or has invalid JSON.
