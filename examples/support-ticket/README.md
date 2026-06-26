# Support Ticket Service Module

This example is the agent-ready proof point for Lenso: a business module with
routes, schema-admin data, actions, a runtime function, and Console-visible
metadata.

It exposes:

- a manifest at `/lenso/module/v1/manifest`;
- service status at `/lenso/module/v1/status`;
- `GET /tickets/{id}` and `POST /tickets`;
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
lenso module install http://127.0.0.1:4110/lenso/module/v1/manifest
lenso module service list
lenso module service export --module support-ticket --format compose
lenso module doctor support-ticket --json
```

Restart the API and worker after install. Open `/console` and check Modules:

- service lifecycle shows installed / configured / ready;
- service status, compatibility, deployment, and health history are visible in
  Operations;
- the `tickets` data surface and `assign_ticket` action are available;
- Remote Calls and Runtime Story show host-owned operation evidence after the
  module is used through the host.

If doctor reports `restart_pending`, restart the host. If it reports
`manifest_unreachable` or `service_not_ready`, start `pnpm start:support-ticket`
again and recheck the manifest URL.
