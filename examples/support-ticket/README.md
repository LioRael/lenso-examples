# Support Ticket Service

This example is the agent-ready proof point for Lenso: an independently running
service that provides the `support-ticket` module to a host.

It exposes:

- a service manifest at `/lenso/service/v1/manifest`;
- service status at `/lenso/service/v1/status`;
- a `support-ticket` module under `/lenso/service/v1/modules/support-ticket`;
- `GET /tickets/{id}` and `POST /tickets` on the module path;
- `support-ticket.escalate-ticket.v1`;
- one declarative admin surface for `tickets`;
- an `assign_ticket` admin action.

Run it from the repository root:

```sh
pnpm start:support-ticket
```

Run the smoke:

```sh
pnpm smoke:support-ticket
```

Install into a local Lenso host:

```sh
lenso service install http://127.0.0.1:4110/lenso/service/v1/manifest
lenso service list
lenso service export --module support-service --format compose
lenso service doctor support-ticket --json
```

Restart the API and worker after install. Open `/console` and check Modules:

- service lifecycle shows support-service installed / configured / ready;
- service status, compatibility, deployment, and health history are visible in
  Operations;
- the `tickets` data surface and `assign_ticket` action are available;
- Remote Calls and Runtime Story show host-owned operation evidence after the
  module is used through the host.

If doctor reports `restart_pending`, restart the host. If it reports
`manifest_unreachable` or `service_not_ready`, start `pnpm start:support-ticket`
again and recheck the manifest URL.
