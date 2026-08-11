# Support Desk product acceptance fixture

This directory is the reviewed input to `pnpm acceptance:support-desk`.

- `capability/` contributes the service-backed `support/tickets` Module and the
  linked `lenso/platform-story` Module to the public Support Desk blueprint.
- `lenso.app.json` is the exact App Composition produced by the command in the
  repository README. The acceptance refuses any structural JSON drift.
- The runner initializes a fresh public Service workspace, registers the real
  Support Ticket Provider, and starts it only through `lenso system dev`.

The fixture contains no environment, deployment, release, database, Adapter
credential, or signing-key state. Enrollment keys and Adapter/Provider Core
bearer tokens are ephemeral server-side acceptance inputs and never cross into
browser requests. The representative operator uses a normal Console login and
session.
