# Support Ticket Remote Module

This example is the agent-ready proof point for Lenso: a business module with
routes, schema-admin data, actions, a runtime function, and Console-visible
metadata.

It exposes:

- a manifest at `/lenso/module/v1/manifest`;
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
```

Open `/console` after restarting the host. The `tickets` data surface and
`assign_ticket` action should be available through the module admin views.
