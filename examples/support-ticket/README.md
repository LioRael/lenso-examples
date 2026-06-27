# Support Ticket Service Suite

This example is the agent-ready service-suite proof for Lenso:
`support-ticket` is the business module, and `support-suite-provider` is the
provider service process that also exposes `support-notification` and
`support-knowledge-base`.

It exposes:

- a service manifest at `/lenso/service/v1/manifest`;
- service status at `/lenso/service/v1/status`;
- `support-ticket` under `/lenso/service/v1/modules/support-ticket`;
- `support-notification` under `/lenso/service/v1/modules/support-notification`;
- `support-knowledge-base` under `/lenso/service/v1/modules/support-knowledge-base`;
- ticket HTTP routes, `support-ticket.escalate-ticket.v1`, the `tickets`
  admin surface, and `assign_ticket`;
- `support-notification.send-ticket-update.v1`;
- `GET /articles/{id}` for knowledge-base articles.

Run it from the repository root:

```sh
pnpm --filter @lenso/example-support-ticket start
```

Run the smoke:

```sh
pnpm --filter @lenso/example-support-ticket smoke
```

Install and check the suite from a local Lenso host:

```sh
lenso module install support-ticket
lenso service check support-suite-provider
lenso service doctor support-ticket --json
```

Restart the API and worker after install. Open `/console` and check Modules:

- `support-ticket` is the installed business module;
- `support-suite-provider` is the configured service provider;
- `support-notification` and `support-knowledge-base` appear as sibling modules
  from the same provider;
- service status, compatibility, deployment, and health history are visible in
  Operations;
- the `tickets` data surface and `assign_ticket` action are available.

The host still owns Runtime Story, Remote Calls, queue, outbox, and retry
evidence after the modules are used through the host.

If check or doctor reports `restart_pending`, restart the host. If it reports
`manifest_unreachable` or `service_not_ready`, start the provider again and
recheck:

```sh
pnpm --filter @lenso/example-support-ticket start
lenso service check support-suite-provider
```
