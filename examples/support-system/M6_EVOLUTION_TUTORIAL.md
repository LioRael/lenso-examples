# From Fresh Lenso Host to Autonomous Service System

This M6 tutorial uses public packages and the exact component combination in
the released GA Support Manifest. It demonstrates Lenso as an agent-ready
modular app framework; it is not an enterprise service mesh.

## 1. Start outside framework workspaces

Install the exact published `@lenso/cli` version named by
`lenso.ga-support-manifest.v1.json`, then create a fresh host in an empty
directory:

```sh
lenso host init support-app
cd support-app
cp .env.example .env
lenso serve
```

Verify `/console`, the independent Provider compatibility path, and the M0–M5
acceptance guarantees before M6-only work.

## 2. Build a linked business Module

Create `support-ticket` through the public module command. Declare its
`ModuleManifest`, routes, Store ownership, events, Workflow, admin, and Console
surfaces. Verify linked behavior and generated Contracts. Provider v1 remains
the supported Host-managed compatibility path.

## 3. Evaluate extraction

Use `$lenso-module-extraction` or the public CLI to create the readiness report,
stale-safe Extraction Plan, and dry-run. Resolve ownership, cross-Module SQL,
Consumer, transaction, and data-volume findings. Scaffold API, Worker, and
Migration Workloads and one isolated Service Store from the exact plan.

Expand, backfill, reconcile, compare linked/autonomous behavior, quiesce linked
work, and prepare rollback. Stop at the authority-transfer Approval Boundary.
Only explicit approval bound to the current plan digest may establish
Autonomous authority.

## 4. Exercise Service behavior

Generate direct HTTP or gRPC clients from exact Service Contracts. Preserve
Deadline, idempotency, Story Context, Service Principal, delegated actor,
tenant, causation, region, and Call Policy. Publish reliable Events through the
Service Outbox and consume through the Inbox.

Start a version-pinned Durable Workflow with retries, timers, child Workflow,
and declared compensation. Inspect local Story Segment evidence and the
federated Runtime Story without making Console part of the Data Plane.

## 5. Deliver, fail, recover, and evolve

Create an immutable Service Release, verify trust and policy, stage through the
supported deployment adapter, observe canary evidence, and exercise rollback.
Run the delivery recovery, backup/restore, active-passive DR, three-Service
performance, 10/20 Service envelope, security, and Contract lifecycle gates.
Every destructive, production, Retirement, trust, restore, and authority action
stops at its named Approval Boundary.

## 6. Candidate proof

Run from clean caches with exact staged artifacts:

```sh
pnpm acceptance:m6 -- --mode candidate \
  --support-manifest ./lenso.ga-support-manifest.v1.json \
  --packages ./m6-candidate-packages.json \
  --scenario-evidence ./m6-failure-evidence.json \
  --ga-evidence ./m6-ga-evidence.json
```

Candidate mode always records `gaEligible: false`. It performs no production
publication, release-mode change, Contract Retirement, disaster authority
change, or destructive cleanup. Published-mode GA proof uses immutable public
packages only and is a later gate.
