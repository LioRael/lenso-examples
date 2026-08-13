# Organization invitation email product acceptance

This fixture is the reviewed input to
`pnpm acceptance:organization-invitation`. It proves the first transactional
Notification workflow through three separate gates:

1. `:contract` validates exact package prerequisites, shared Event schemas,
   Module and Service ownership, Surface operations, and allowed state paths.
2. The runtime gate starts the real Host, worker, PostgreSQL stores, and Email
   Provider with deterministic fake transport, then uses the public
   organization invitation-delivery HTTP API.
3. The browser gate reconciles the Notification `console_ui_esm` artifact and
   Surface API Grant into the real Console, then verifies rendered delivery,
   attempt, receipt, retry, Story correlation, denial, responsive, theme,
   keyboard, focus, scroll, and reduced-motion evidence.

The Notification Surface belongs to `lenso/notification`; the downstream Email
Provider supplies transport behavior only. SMTP credentials, Provider bearer,
invitation token, full recipient address, unrestricted Provider URL, and
plaintext render snapshots are forbidden from browser and persisted evidence.

The runner must refuse to start until `lenso@0.3.45` and
`@lenso/service-kit@0.6.1` are available through the explicitly selected
integration set. Local source overrides are allowed only when their exact
commit and content digest are recorded in the resulting evidence.
