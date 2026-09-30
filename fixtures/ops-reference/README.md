# Operations reference App

An ordinary source App routes one HTTP Endpoint to two configured Instances of
one state Plugin. The CLI discovers Plugin Roots, generates OpsState projections,
resolves named dependencies and builds the Host. There is no App-owned resolver
or handwritten Plan. Management is an explicitly selected optional assembly;
the business verifiers run without it.

## Prepare exact candidates

The published DX is `@lenso/cli@0.17.4`, whose native executable reports
`lenso 0.6.4`; the Rust facade is `lenso = 0.5.28`. These are distinct package
versions. New scoped Host facilities and linked Rust Workers graphs require
**source candidates**, selected in [candidate-inputs.json](candidate-inputs.json).
Workers runtime `0.1.6` is a candidate, not a public registry install target.
The selected source CLI reports `lenso 0.6.6` and uses the `lenso 0.5.28`
SDK. The native CLI and updated authoring path remain source candidates;
`lenso 0.5.28` is published. Management qualification explicitly selects the
already reachable owner Cargo Plugins through their Plugin Root files. The
generated Host retains each exact Git package identity and records these
selections in `.lenso/root-linked-sources.json`.

From this directory, with Python 3.12+, Git, Cargo and npm:

```sh
python3 prepare.py --cache candidate-tools
```

This builds the exact Core commit and packs the exact JS runtime commit in a
fresh directory. It uses standard Cargo configuration and does not upgrade the
global CLI. `candidate-tools/tools.json` records the executable path and digest,
plus the runtime path and archive digest. It also stages the npm launcher and
its exact TypeScript parser for the current platform. Use its native `cli` path
below; TypeScript Host authoring uses the `npm_cli` path. The local launcher
stage does not prove npm publication. No sibling
checkout is required; starting a built Native distribution needs only the
native executable.

## Generate the ordinary Web starter

The published CLI can generate a small HTML and greeting App before adopting the
state dependency and its explicit Host facilities:

```sh
npx --yes @lenso/cli@0.17.4 app create starter --web --no-install
npx --yes @lenso/cli@0.17.4 app build --root starter --out starter-dist
npx --yes @lenso/cli@0.17.4 app check --root starter-dist
npx --yes @lenso/cli@0.17.4 app start --from starter-dist
```

The starter has no selected facility factories. The operations fixture below is
the subsequent example with two state Instances and explicit resource choices.
The automated starter verifier uses the candidate CLI's ordinary creation and
build commands, deletes its source, serves HTML from the offline Native artifact
and checks clean shutdown. Before publication, select `--source-cohort` to apply
the recorded Core Git selection to the generated Rust Plugin Roots explicitly:

```sh
python3 verify_starter.py --cli /absolute/path/to/lenso --source-cohort --receipt starter-receipt.json
```

The receipt labels this as an explicit source consumer. Omitting the flag checks
registry dependencies and requires the generated SDK versions to be published.

## Run Native with simulated state

```sh
/absolute/path/to/lenso app build --root project --out dist
/absolute/path/to/lenso app check --root dist
/absolute/path/to/lenso app explain --root dist --host-facilities profiles/simulated-native.json --json
/absolute/path/to/lenso app start --from dist --host-facilities profiles/simulated-native.json
```

Read `/state/primary` and `/state/secondary` at the printed URL: their initial
values are 11 and 29, labels differ and revisions start at zero. A POST to
`/state/primary` accepts:

```json
{"value":47,"expected_revision":0,"idempotency_key":"write-1"}
```

The response contains revision one and a domain receipt. Repeating the exact
command returns that receipt. A different intent under the same key or a stale
revision returns 409. Invalid input returns 400; an unknown Instance returns 404.
`/receipts/primary/write-1` queries the committed result. Named selections in
`project/plugins/.dependencies.json` do not depend on registration order.
All profiles restrict integers to JavaScript's exact integer range.

The optional `cache` Port is saved as explicit `none`. `/cache` reports that
selection. Adding another configured state provider does not silently select
it, and `check`, `show` and `explain` preserve the saved choices:

```sh
python3 verify_optional_cache.py --cli /absolute/path/to/lenso --receipt optional-cache-receipt.json
cargo test --locked --manifest-path project/app/state/Cargo.toml --lib
```

The owner tests use the actual Kernel and TestSimulator. Their explicit test
World retains state and receipts across a new Host generation, checks competing
CAS writes and rejects retired handles. This test storage does not establish
SQL or platform persistence.

```sh
python3 verify.py --cli /absolute/path/to/lenso --receipt native-receipt.json
```

The verifier copies only the business fixture into a clean directory, deletes
generated contract projections, then rebuilds through the ordinary CLI. It
removes copied sources and toolchains from the startup PATH, checks shared HTTP
vectors and graceful shutdown, and retains source/build receipts. Simulated
Native state lasts for one Instance and resets when that Instance is recreated.
Simulated Workers state lasts for one request event. Neither proves persistence.

## Verify real Native PostgreSQL

Provide an authorized **test** PostgreSQL URI in a private local file:

```sh
python3 verify_pg.py --cli /absolute/path/to/lenso \
  --database-url-file /private/path/test-postgres-uri \
  --receipt native-pg-receipt.json
```

`LENSO_OPS_TEST_DATABASE_URL` is also supported for CI. The verifier creates two
owned temporary schemas, runs [operator SQL](storage/native-pg/schema.sql) before
startup, and cleans up those schemas afterward. Startup only opens and checks
the selected resources; it never creates tables. The URI stays out of command
arguments and receipts. CAS uses a row lock; state and the idempotency receipt
commit in one transaction. After source deletion, a second Host checks prior
state and receipts. Concurrent commands at one revision yield one 200 and one
409; replay does not increment the revision again.

## Build the same business graph for Workers

Install the `wasm32-unknown-unknown` target and `wasm-bindgen 0.2.127`. Use the
packed runtime directory from `candidate-tools/tools.json`:

```sh
/absolute/path/to/lenso app build --root project --target workers \
  --wasm-bindgen /absolute/path/to/wasm-bindgen \
  --workers-runtime /absolute/path/to/candidate-tools/workers-runtime/package \
  --workers-facilities profiles/simulated-workers.json \
  --workers-host-limits profiles/workers-host-limits.json \
  --out dist-workers
```

The receipt retains source/plan/Wasm/binding/entry digests, exact owner grants,
runtime modules and explicit request budgets. This is a static request graph
with one HTTP Endpoint. Dynamic installation, streams, events and TypeScript
Workers Plugin authoring remain outside this profile. With local workerd running
that artifact, verify simulated routing without claiming event persistence:

```sh
python3 verify.py --cli /absolute/path/to/lenso --url http://127.0.0.1:8787 \
  --artifact dist-workers --static --receipt workers-routing-receipt.json
```

## Select D1 explicitly

Copy the project, change each state Instance's profile to `workers-d1`, and
provide this Host facility file:

```json
{
  "schema":"lenso.host-facilities.v1",
  "instances":{
    "example.ops-state/primary":{
      "state":{"binding":"OPS_PRIMARY","configuration":{"profile":"workers-d1"}}
    },
    "example.ops-state/secondary":{
      "state":{"binding":"OPS_SECONDARY","configuration":{"profile":"workers-d1"}}
    }
  }
}
```

The private owner factory receives only its selected binding, configuration and
event scope. Run [D1 operator SQL](storage/d1/schema.sql) separately in both owned
test databases, then insert the primary state (label `primary-state`, value 11,
revision 0) and secondary state (`secondary-state`, 29, 0). Configure those D1
bindings in the built artifact's Wrangler file. Build with that facility file
and `profiles/workers-host-limits.json`: its 10-second event budget includes
readiness and database operations. Use Wrangler `4.143.1`, compatibility date
`2026-09-26`. Resource creation/setup is separate from runtime startup.

```sh
python3 verify.py --cli /absolute/path/to/lenso --url http://127.0.0.1:8787 \
  --artifact dist-d1 --profile workers-d1 --receipt d1-local-receipt.json
LENSO_REFERENCE_HTTP_TRANSPORT=node python3 verify.py \
  --cli /absolute/path/to/lenso --url https://your-test-worker.example \
  --artifact dist-d1 --profile workers-d1 --infrastructure cloudflare \
  --receipt d1-remote-receipt.json
```

Local workerd uses D1 emulation; remote receipts identify real Cloudflare D1
separately. Restart workerd or redeploy the same artifact without resetting its
databases, then add `--after-restart` to check prior state and the original
receipt. D1's conditional receipt INSERT and state UPDATE share one batch
transaction. If a dispatched write loses its response, query the same key before
making another decision. Unknown writes are never replayed automatically.

Hyperdrive/Workers PostgreSQL and a shared PG+D1 Worker remain unqualified;
Hyperdrive acceptance was explicitly deferred. Missing or wrong selections
fail rather than switching backend. Changing a binding is not data migration.
Native PG and D1 proofs do not establish the Auth, approval, Console, Agent or
MCP dependency closure on Workers.

## Select Native management

Management adds independently removable HTTP, Console, Agent and MCP projections
to the same state Plugin. The reference selects exact Console, Agent, Auth,
Access Control, Business Approval and Audit Log source revisions. Public
Capability contracts connect them; Console does not read an owner's tables or
grant authority. The operator prepares disposable schemas, key material,
qualifications and local journals before startup. Runtime startup only opens
those selected resources.

```sh
python3 verify_management.py --cli /absolute/path/to/lenso \
  --database-url-file /private/path/test-postgres-uri \
  --audit-url-file /private/path/test-audit-postgres-uri \
  --receipt management-receipt.json
```

This builds an ordinary source App, runs explicit private bootstrap, then
removes bootstrap and builds again. After deleting source, it verifies live
credentials, qualified subjects, current ACL ceilings, immutable approval
intents, distinct human decisions, single business commit, conflict detection,
audit attribution, Agent reads and real HTTP MCP initialize/list/call. Restart
retains the operation. Revoking the credential blocks a previously initialized
MCP session as well as HTTP and Agent calls. MCP is read only in this profile;
neither model projection exposes human approval or personal-token issuance.

Select the single approval-required MCP write entry explicitly to verify an
external client requesting a write, querying its pending operation, receiving a
distinct human decision and continuing the original request once:

```sh
python3 verify_management.py --cli /absolute/path/to/lenso --mcp-write \
  --database-url-file /private/path/test-postgres-uri \
  --audit-url-file /private/path/test-audit-postgres-uri \
  --receipt mcp-write-receipt.json
```

Each invocation initializes a new owned test namespace. A failed initial
compilation can resume with the original `--directory`, `--source` and
`--resume-build` before bootstrap starts; it cannot repeat an enrollment or
bootstrap whose outcome is uncertain.

## Use real Account sessions and personal tokens

```sh
python3 verify_human.py --cli /absolute/path/to/lenso \
  --database-url-file /private/path/test-postgres-uri \
  --audit-url-file /private/path/test-audit-postgres-uri \
  --receipt human-receipt.json
```

The private enrollment Plugin uses actual Password registration and the
returned Account subjects, revokes the initial registration sessions, and
enrolls those subjects through Access Control and the qualification operator.
It does not seed sessions or approval facts with SQL. The verifier checks login,
HttpOnly/Secure session cookies, CSRF and Origin rejection, qualification,
distinct Account approval, bounded personal-token issuance, one-time secret,
metadata-only reconciliation, cross-subject denial, live token revocation and
logout. The session and committed operation survive Host restart.

For a rendered Console proof, supply `--public-origin http://localhost:53231`
and `--browser-ready-file /private/path/browser-ready.json`. When that redacted
file appears, it identifies the running Native URL and owned temporary
directory. Start the selected Console checkout with:

```sh
VITE_CONSOLE_DEV_MODE=api VITE_CONSOLE_MODE=api \
  LENSO_CONSOLE_DEV_HOST=http://127.0.0.1:PRINTED_NATIVE_PORT \
  pnpm dev --host 127.0.0.1 --port 53231
```

Open `http://localhost:53231/management`. The owned directory contains private
password files for `alice@ops.test` and `bob@ops.test`; read them locally without
putting passwords or personal tokens in commands, traces or screenshots. Use
distinct browser contexts for requester and approver. Verify the pending
intent, distinct approval, commit, personal-token display/hide/list/revoke, and
absence of session or token secrets in browser storage. Write a redacted
`browser-ready.done` receipt with `status: "passed"` only after those actions
actually pass; the verifier then closes the Host and records that evidence.

These management helpers preserve their owned temporary resources for
inspection. A failed initial build can use `--directory` and `--resume-build`
before bootstrap starts. Once bootstrap or enrollment starts, reconcile its
stable receipts before any further action; do not repeat issuance or enrollment
blindly. Clean up only those owned test schemas, files and processes after
inspection. Human personal tokens currently require Native PostgreSQL; their
Workers profile fails explicitly rather than falling back.

## Run an independent task Agent with an Account-issued child

A completed human profile can explicitly select `/agent` alongside its normal
Console routes. The App issues one short-lived child through the actual Account
Owner, with fixed deployment, permissions, resources, task, Agent session and
caller. Its opaque value lives in one private regular file; the independent
Agent selects the Connection, Management Tools, Loop and deterministic fixture
Model through its own Plugin Root. It has no human approval, token, shell or
arbitrary HTTP tool.

Use `build_agent.py` to build and copy an immutable `management_task` executable with a source/build receipt from the exact Agent revision selected by
`project/app/management/tools/Cargo.toml`, then use that executable and its
`crates/lenso-agent-management-connection-plugin/examples/prepare_task.py`:

```sh
python3 build_agent.py --source /absolute/path/to/selected-agent-checkout \
  --executable /absolute/path/to/management_task \
  --receipt /absolute/path/to/agent-build.json
```

```sh
python3 verify_agent.py --directory /private/path/completed-human-profile \
  --human-receipt human-receipt.json --cli /absolute/path/to/lenso \
  --executable /absolute/path/to/management_task \
  --executable-provenance /absolute/path/to/agent-build.json \
  --prepare-task /absolute/path/to/prepare_task.py --receipt agent-receipt.json
```

The verifier observes a successful human Console request while the independent
Agent process is still live, uses the same Management process and journal,
requests a pending write at the read revision, queries it after a new Agent
process, and checks that parent logout denies the current child. It does not
approve the pending request. Real model interaction is separately deferred;
this deterministic Model proves the selected runtime and authority path.
