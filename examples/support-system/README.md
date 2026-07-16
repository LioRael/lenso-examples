# Communicating Support Services

This fixture preserves the existing `support-ticket`, `support-sla`,
`support-http`, and `support-grpc` identities while extracting the Modules into
two Autonomous Services:

- `support-ticket-service` owns the `support-ticket` Module, its HTTP Service
  Contract, the `support.ticket-opened.v1` Event Contract, and an isolated
  Store.
- `support-sla-service` owns the `support-sla` Module, its gRPC Service Contract,
  and an isolated Store.

Both Services declare API, Worker, and Migration Workloads. `lenso system dev`
starts them in dependency order, assigns development-only Workload Identity,
publishes their local endpoints, waits for health, and cleans up its owned
processes and Stores.

## Run the proof

The fixture currently consumes the direct client APIs from the sibling
`../lenso` checkout. Build the support binaries and validate the System inputs:

```sh
pnpm smoke:support-system:contract
pnpm smoke:support-system
```

Run the authoritative M1 developer-preview acceptance workflow from the
repository root:

```sh
pnpm acceptance:m1
```

This one command proves the generated-client business call and local Story
Segment evidence, repeats the call after the Sandbox endpoint-state source is
withheld, verifies Deadline, crash, partial-unavailability, attempt-count, and
no-unsafe-retry machine results, checks deterministic cleanup, and finally runs
the independent Host-managed Provider smoke. Runtime Console, Host, Provider,
Kubernetes, service mesh, external broker, and production identity processes
are absent from the Autonomous Data Plane proof.

Run the combined M2 reliability and identity proof with a CLI containing M2
cyclic Service bootstrap support:

```sh
LENSO_CLI_BIN=../lenso-cli/target/debug/lenso pnpm acceptance:m2
```

This extends the same public Sandbox seam with a transactional support event,
exactly-once business effects over at-least-once delivery, local Service
Principal/delegation/tenant verification, dead-letter replay, restart and
transport interruption recovery, the complete deterministic Call Policy
matrix, plane-independent Service-local evidence, cleanup, and the unchanged
Provider smoke. Event production and consumption run inside the two started
Service Worker processes, while the Call Policy matrix reaches the live SLA
Service. The command starts an ephemeral Postgres instance when `DATABASE_URL`
is not already supplied.

Real production integrations are deliberately separate and require explicit
authorized infrastructure:

```sh
LENSO_NATS_TEST_INFRASTRUCTURE_APPROVED=true \
LENSO_SPIFFE_TEST_INFRASTRUCTURE_APPROVED=true \
DATABASE_URL=postgres://... \
SPIFFE_ENDPOINT_SOCKET=unix:///... \
pnpm acceptance:m2:production
```

That command runs the NATS JetStream transport conformance, reuses the same
support Module handler and Event Contract against JetStream, and runs the
SPIFFE/SPIRE identity proof. It refuses to infer production authority from
repository access or local execution.

To use a source-built CLI containing the System Sandbox:

```sh
LENSO_CLI_BIN=../lenso-cli/target/debug/lenso pnpm smoke:support-system
```

The smoke performs this Data Plane path:

```text
generated HTTP client
  -> support-ticket-service / support-http
  -> generated gRPC client
  -> support-sla-service / support-grpc
```

No request is routed through the declared compatibility Host or Provider. The
final JSON output exposes Service References, resolved endpoints, Deadline,
Idempotency Key, HTTP and gRPC call decisions, calls before and after the
System Plane source is withheld, one-attempt unsafe-operation suppression,
Workload Identity, and Store isolation. Each Service also writes
business-operation evidence inside its sandbox-owned Store before cleanup.

The original Host-managed Provider proof is intentionally independent:

```sh
pnpm smoke:support-ticket
```
