# Communicating Support Services

This fixture preserves the existing `support-ticket`, `support-sla`,
`support-http`, and `support-grpc` identities while extracting the Modules into
two Autonomous Services:

- `support-ticket-service` owns the `support-ticket` Module, its HTTP Service
  Contract, and an isolated Store.
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
