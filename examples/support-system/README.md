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

Run the M3 durable-process and federated-evidence proof through the same public
Sandbox seam:

```sh
LENSO_CLI_BIN=../lenso-cli/target/debug/lenso pnpm acceptance:m3
```

The command first preserves the complete M2 direct-call, Event Contract,
identity, tenant, Deadline, and Call Policy proof. It then withholds System
Plane state while the support SLA Durable Workflow runs child work, recovers
across participant restarts, advances controlled time to its timeout, and
reverses both completed support-ticket effects exactly once in declared order.
One v1 instance remains pinned while a new v2 instance starts; an incompatible
worker and unsafe migration are rejected without mutating durable state.

Story aggregation and Runtime Console are absent during execution. After the
workflow completes, the authenticated Service-local feeds are collected into
the same `lenso.federated-runtime-story.v1`, late evidence enriches the existing
Story, and an intentionally unavailable source remains visible as an
`unreachable` Segment gap. The support SLA Reliability Contract is evaluated
against real Workflow Store pressure and remains report-only. The final gate
runs the independent Host-managed Provider smoke and requires no Kubernetes,
external workflow engine, or production authority.

Run the M4 Safe Module Extraction proof through the same support-system seam:

```sh
LENSO_CLI_BIN=../lenso-cli/target/debug/lenso pnpm acceptance:m4
```

The command retains the M3 and Provider proofs, then emits
`lenso.m4-safe-module-extraction-acceptance.v1` with blocked and corrected
readiness, deterministic plan/scaffold evidence, interrupted/resumed backfill,
reconciliation, linked/candidate behavior comparison, quiescence, failed
provisional rollback, exact approval pins, Autonomous authority history, stale
evidence rejection, post-commit rollback blocking, and cleanup status.

## M6 GA support and recovery shell

Inspect the public candidate/published modes and their non-production effects:

```sh
pnpm acceptance:m6 -- --describe
```

Preflight an exact staged package set against the versioned GA Support
Manifest without running product scenarios:

```sh
pnpm acceptance:m6 -- --preflight --mode candidate \
  --support-manifest ./lenso.ga-support-manifest.v1.json \
  --trusted-manifest-digest "$LENSO_M6_TRUSTED_MANIFEST_DIGEST" \
  --packages ./m6-candidate-packages.json
```

The trusted digest must come from the reviewed acceptance environment, not from
the supplied manifest file. Candidate package entries point to immutable
absolute `artifactPath` files outside the framework workspaces. Exactly one
staged CLI entry declares `candidateTracer: true`; that copied CLI must consume
and report every exact package digest from the isolated starter.

Candidate mode accepts only exact staged artifacts with immutable digests and
accepted shadow receipts. Published mode accepts only public registry artifacts
with accepted release receipts. Both reject path/workspace dependencies,
mutable versions, synthetic final versions, missing receipts, and combinations
absent from the Support Manifest. Candidate evidence always records
`gaEligible: false`.

The explicitly approved Environment Verification lane produces the recovery
scenario evidence consumed by the shell:

```sh
LENSO_NATS_TEST_INFRASTRUCTURE_APPROVED=true \
LENSO_SPIFFE_TEST_INFRASTRUCTURE_APPROVED=true \
LENSO_KUBERNETES_TEST_INFRASTRUCTURE_APPROVED=true \
DATABASE_URL=postgres://... \
SPIFFE_ENDPOINT_SOCKET=unix:///... \
pnpm acceptance:m6:environment -- --output ./m6-environment-evidence.json
```

The complete candidate gate additionally consumes `--scenario-evidence` and
`--ga-evidence`, which bind delivery recovery, backup/restore, disaster
recovery, performance, the 3–20 Service envelope, and security review to the
exact Support Manifest. Follow
[`M6_EVOLUTION_TUTORIAL.md`](./M6_EVOLUTION_TUTORIAL.md) for the public
fresh-starter product path.

It runs the real JetStream, SPIFFE/SPIRE, PostgreSQL-backed recovery, and
disposable Kubernetes/Operator proofs. Successful named proof commands are
bound directly into the scenario evidence written by `--output`; callers do
not need to predict command-output digests. An optional `--scenario-evidence`
input is still accepted for independently collected evidence and is verified
against this run. The artifact records only stable principals, digests, adapter
versions, outcomes, effects, and cleanup—never credentials, tokens,
certificates, private keys, or production topology.

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
