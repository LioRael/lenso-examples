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
Idempotency Key, HTTP and gRPC call decisions, one-attempt unsafe-operation
suppression, Workload Identity, and Store isolation. Each Service also writes
business-operation evidence inside its sandbox-owned Store before cleanup.

The original Host-managed Provider proof is intentionally independent:

```sh
pnpm smoke:support-ticket
```
