# Support Ticket Service Suite Run

Run this guide when you want to verify the support-ticket example as a Lenso
service suite. `support-ticket` is the business module users install, and
`support-suite-provider` is the provider service process that also provides
`support-notification` and `support-knowledge-base`.

## Prerequisites

Keep these repositories checked out as siblings:

```text
framework/
  lenso/
  lenso-examples/
  lenso-runtime-console/
```

The commands assume the local backend defaults:

- API on `http://127.0.0.1:3000`;
- `APP_ENV=local`, so development bearer tokens are accepted;
- the `lenso` CLI is installed on `PATH`.

## Start The Service

From `lenso-examples`:

```sh
pnpm install
pnpm --filter @lenso/example-support-ticket start
```

The provider serves its manifest and status at:

```text
http://127.0.0.1:4110/lenso/service/v1/manifest
http://127.0.0.1:4110/lenso/service/v1/status
```

Keep this process running. The example stores tickets in memory, so restarting
the provider resets records created during the guide.

## Install Into The Host

From the sibling `lenso` backend checkout:

```sh
test -f .env || cp .env.example .env
lenso module release inspect ../lenso-examples/dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json
lenso module catalog add ../lenso-examples/dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
lenso module install support-ticket
lenso service list
lenso service check support-suite-provider
lenso service doctor support-suite-provider --json
```

You can also install the release artifact directly:

```sh
lenso module release check ../lenso-examples/dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
lenso module install ../lenso-examples/dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
```

Run `pnpm service-package:support-ticket` from `lenso-examples` first if the
`dist/lenso-service` artifact is not present. The checked-in
`examples/support-ticket/lenso.module-release.json` is a local dev shortcut
that points at the running provider manifest.

Use `lenso service export --module support-suite-provider --format compose`
when you need a deployment fragment for the provider process.

Restart the host after installation:

```sh
just db-up
just migrate
just api
```

Run `just worker` in another shell when verifying runtime work, and run the
Runtime Console from `../lenso-runtime-console` when you want the UI.

## Operator Loop

Use these commands when the provider is installed but not behaving as
expected:

```sh
lenso service list
lenso service check support-suite-provider
lenso service status support-suite-provider support-suite-provider
lenso service start support-suite-provider support-suite-provider
lenso service stop support-suite-provider support-suite-provider
lenso service doctor support-suite-provider --json
```

`lenso service check` is the provider-level check command. In the current CLI it
shares the same diagnostic engine as `service doctor`; use the provider name
when diagnosing service state. Console still shows `support-ticket` as the
business module. The important statuses are:

| Status                  | Meaning                                                                            | Next action                                                                   |
| ----------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `ready`                 | Source is configured, loaded, manifest checks pass, and service readiness is good. | Use the host proxy and Console evidence.                                      |
| `restart_pending`       | `.env` or service lifecycle state changed after host startup.                      | Restart API and worker.                                                       |
| `configured_not_loaded` | The source exists, but the host metadata does not include the module.              | Restart first; then inspect manifest errors.                                  |
| `manifest_unreachable`  | Host cannot fetch `/lenso/service/v1/manifest`.                                    | Start the provider or fix `REMOTE_MODULES`.                                   |
| `service_not_ready`     | `.lenso/module-services.json` has a service entry whose `readyUrl` fails.          | Start the service or inspect its logs.                                        |
| `stale_state`           | A lock or pid file exists but readiness failed.                                    | Stop/restart the service; remove stale files only after checking the process. |
| `unreachable`           | The standard service status endpoint failed.                                       | Start the service or fix `/lenso/service/v1/status`.                         |

## Call Through The Host

Create a ticket through the host-owned service proxy:

```sh
curl -sS -X POST http://127.0.0.1:3000/modules/support-ticket/http/tickets \
  -H 'authorization: Bearer dev-service:admin:support_ticket.tickets.write' \
  -H 'content-type: application/json' \
  -H 'x-correlation-id: corr_support_ticket_service_module' \
  -d '{
    "title": "Service module onboarding",
    "priority": "high",
    "assignee": "triage"
  }' | jq .
```

Assign the ticket through the host-owned admin action endpoint:

```sh
curl -sS -X POST http://127.0.0.1:3000/admin/data/support-ticket/actions/assign_ticket \
  -H 'authorization: Bearer dev-service:admin:support_ticket.tickets.write' \
  -H 'content-type: application/json' \
  -H 'x-correlation-id: corr_support_ticket_assign' \
  -d '{
    "input": {
      "ticket_id": "ticket_1",
      "assignee": "support@example.com"
    }
  }' | jq .
```

## Inspect Operations

Remote Calls, Runtime Story, queue, outbox, and retry evidence are still
host-owned:

```sh
curl -sS \
  'http://127.0.0.1:3000/admin/runtime/remote-proxy-calls?correlation_id=corr_support_ticket_service_module&limit=10' \
  -H 'authorization: Bearer dev-service:admin' | jq '.data[0]'
```

Open `/console`, then check:

- Modules shows `support-ticket` as the loaded business module and
  `support-suite-provider` as the configured service provider.
- Modules also shows `support-notification` and `support-knowledge-base` as
  sibling modules from the same provider.
- Data shows the `tickets` surface and `assign_ticket` action.
- Remote Calls includes the support-ticket proxy call.
- Runtime Story for the correlation id includes the service operation.

## Automated Check

From `lenso-examples`, run the host API smoke:

```sh
pnpm host-api-smoke:support-ticket
```

It starts the provider, scaffolds a temporary host, installs the manifest,
validates provider/module separation, calls the host-owned HTTP proxy and
admin/runtime paths, and verifies Runtime Story evidence.

## Troubleshooting

| Symptom                                                                                                | Check                                                                                    | Fix                                                                                                      |
| ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `manifest_unreachable` or `manifestStatus: unreachable` in `lenso service doctor support-suite-provider --json` | The provider process is stopped or `REMOTE_MODULES` points at the wrong base URL.        | Run `pnpm --filter @lenso/example-support-ticket start`, then rerun `lenso service check support-suite-provider`. |
| `service_not_ready`                                                                                    | `.lenso/module-services.json` has a service entry, but its `readyUrl` is not responding. | Start the command shown by doctor or restart the API/worker when `autoStart` is enabled.                 |
| `stale_state`                                                                                          | A host-started service left `.lock` or `.pid` files behind.                              | Restart the API/worker; remove the stale files only if doctor still reports them.                        |
| `404` from `/modules/support-ticket/http/*`                                                            | The host did not load the module provided by the configured provider.                    | Restart the API and worker after installing or changing `REMOTE_MODULES`.                                |
| `403` from host APIs                                                                                   | The development service token lacks a `support_ticket.*` scope.                          | Use a `dev-service:admin:support_ticket.tickets.read,support_ticket.tickets.write` token for this guide. |
