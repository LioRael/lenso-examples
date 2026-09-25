# Knowledge base App

This is the runnable Lenso knowledge base reference App. It starts from an
ordinary source project: the App owns one linked Rust business Plugin and one
Bun Plugin under `project/app/`, while Engine derives the Host Catalog,
bindings, and immutable Plan.

The complete path includes:

- a normal Vite/React browser page at `/` with bearer login, dynamic excerpt
  policy, note creation, durable processing, and file upload;
- a real PostgreSQL-backed API Token Auth Plugin with two-user isolation;
- PostgreSQL note, result, attachment, and per-user settings persistence;
- a typed Bun-to-Rust Jobs Capability path that persists the excerpt before it
  completes the job lease, making redelivery idempotent; and
- source-deleted restart verification for both the business records and durable
  job state.

The excerpt is deterministic and is not represented as a model result. The App
captures the current settings revision when it accepts a note, and stale
compare-and-set updates are rejected without changing the active value.

The linked Rust Plugin also has a separate, optional Host binding for a
versioned attachment-size ceiling. It preserves the existing 1 MiB default
and never replaces the per-user PostgreSQL excerpt settings. A bound Host
source must provide one validated `max_attachment_bytes` value from 1 through
1,048,576; an unavailable bound source rejects new uploads. Each accepted
upload records the request-pinned policy revision as a decimal string in its
response and attachment row. The database operator adds this nullable column
without replacing old attachment records.

The generated native Host recognizes this private binding only because the
selected KnowledgeBase package declares `host-bindings =
["attachment-policy@1"]`. It does not infer the interface from the Plugin ID.
The optional `lenso app start --business-snapshot-policy` file is Host-owned,
not an App-owned `plugins/` option or Plugin environment variable. For a
prepared App distribution, a file source can be selected with an absolute
policy path (substitute a real absolute snapshot path):

```json
{
  "schema": "lenso.host-business-snapshot-policy.v1",
  "object": {
    "plugin_id": "lenso.reference.knowledge-base",
    "instance_key": "default",
    "object_key": "attachment-policy"
  },
  "source": {
    "kind": "file",
    "reference": "operator-attachment-policy",
    "path": "/absolute/path/attachment-policy.json"
  },
  "poll_interval_millis": 1000,
  "max_stale_millis": 30000
}
```

The source file is a separate versioned document, atomically replaced by its
operator:

```json
{
  "schema": "lenso.business-snapshot.v1",
  "revision": 1,
  "object": {
    "plugin_id": "lenso.reference.knowledge-base",
    "instance_key": "default",
    "object_key": "attachment-policy"
  },
  "value": { "max_attachment_bytes": 262144 }
}
```

Run `lenso app start --from dist --business-snapshot-policy
/absolute/path/host-policy.json --check` to require a valid initial source
before readiness, then omit `--check` for continuous polling. Source loss,
wrong object or conflicting revision, and expiry make new uploads unavailable
until a fresh authorized snapshot is accepted. The Host authorization uses the
Plugin-owned `attachment_policy_schema()` and exact selected linked `default`
Instance.
The standard verifier still exercises the unchanged default. To opt in to a
disposable database-backed HTTP check of this Host binding, add
`--verify-business-snapshot` to the verifier command below. It creates a
private Host-owned file outside the App's Plugin Root, starts the exact built
distribution with that policy, and checks revision 1, a revision 2 update,
source loss and same-revision conflict failing closed, and revision 3 recovery.
Three accepted uploads must persist their pinned policy revisions in the
PostgreSQL attachment rows while the per-user settings remain unchanged. The
probe uses only a fresh disposable database; it is not a configuration-center
provider or a production source adapter.

The React source under `project/frontend/` consumes generated types and the
browser runtime from an independently packed `@lenso/web-client` candidate.
The checked-in static assets keep the App build self-contained. To validate a
frontend rebuild in a disposable consumer, supply that package and its
independently recorded SHA-256 to the verifier. A frozen Bun install alone
does not establish the identity of a local `file:` tarball:

```sh
LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso \
  --web-client-package /absolute/path/to/lenso-web-client.tgz \
  --web-client-sha256 "$EXPECTED_WEB_CLIENT_SHA256" \
  --framework-source /absolute/path/to/lenso-rust-checkout \
  --tool-provider-source /absolute/path/to/lenso-capability-agent-tool-provider \
  --auth-source /absolute/path/to/lenso-auth-plugin/crates/lenso-auth-api-token-plugin \
  --jobs-source /absolute/path/to/lenso-jobs-plugin/crates/lenso-jobs-plugin \
  --secrets-source /absolute/path/to/lenso-secrets-plugin/crates/lenso-secrets-env-plugin
```

The following source-development commands require an already prepared App root
with Auth, Jobs, and Secrets adopted and configured, an excerpt Plugin install,
and a disposable development database. A clean checkout does not contain those
provider inputs or configuration. The acceptance verifier below performs that
setup in a temporary copy; it does not prepare this checkout for `app dev`.
For an already prepared App, first place the exact packed `@lenso/web-client`
candidate used by `bun.lock` at its local dependency path, then install and
generate from this fixture directory:

```sh
mkdir -p project/frontend/vendor
cp /absolute/path/to/lenso-web-client.tgz project/frontend/vendor/lenso-web-client.tgz
(cd project/frontend && bun install --frozen-lockfile && bun run generate)
lenso app dev --root project
```

The tarball is intentionally untracked and must match the lockfile; the
frontend cannot install from a clean checkout without it. To update the
checked-in production page after setup, run
`(cd project/frontend && bun run build)` explicitly. The verifier's optional
frontend build does not modify these checked-in files.

`project/frontend/lenso.dev.toml` then opts this App into a loopback Vite page
when `app dev` runs. Engine writes the current Host URL to
`project/.lenso/dev-backend-url`; the dev page's `/__lenso/backend` endpoint and
its proxy read that file on every request, so a successful Host rebuild can
change ports without restarting Vite. The proxy admits only the public paths in
`openapi.json`: notes, job processing and status, settings, and attachments.
Host administration and internal routes are not proxied. If the URL file is
missing or invalid, these endpoints return 503 instead of using a stale Host.
`bun run build` still writes the static production page to
`project/app/notes-web/public`; the dev server does not replace that build or
add a runtime dependency to the packaged App.

## Acceptance

Use a disposable PostgreSQL database. The verifier creates App-owned schemas
and intentionally leaves them available for inspection; it does not delete or
reset the supplied database.

```sh
LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso \
  --framework-source /absolute/path/to/lenso-rust-checkout \
  --tool-provider-source /absolute/path/to/lenso-capability-agent-tool-provider \
  --auth-source /absolute/path/to/lenso-auth-plugin/crates/lenso-auth-api-token-plugin \
  --jobs-source /absolute/path/to/lenso-jobs-plugin/crates/lenso-jobs-plugin \
  --secrets-source /absolute/path/to/lenso-secrets-plugin/crates/lenso-secrets-env-plugin
```

The Rust business Plugin pins the current candidate `lenso` 0.5.27, HTTP
Endpoint 0.3.4, and Agent Tool Provider 0.3.0 cohort. These versions are not
all published. The two explicit source arguments are development inputs for
that cohort: the verifier checks their package identities and exact direct
versions, then puts Cargo path overrides in its disposable consumer's private
`CARGO_HOME`. It never adds an absolute checkout path to this fixture's
manifest. The checked-in `Cargo.lock` was resolved against these local
candidate versions but may need a different dependency graph at a newer
checkout. Source mode refreshes the temporary business App lock offline under
the candidate overrides before its `--locked` operator build. Auth and Jobs
operator builds keep the caller's original Cargo home, so those overrides
cannot change their independent locks. The temporary Auth operator is built
once with `--locked` outside the source tree, then its exact binary digest is
checked before token issuance. A subsequent Host build may rewrite the copied
Auth lock; it cannot silently cause a second Cargo resolution for credentials.
The checked-in fixture and original provider checkout locks are not rewritten.
These locks are not proof that the same versions are available from crates.io.
Source mode trusts the selected checkouts and reuses the caller's Cargo registry
cache; it is not an isolated third-party build.

In default mode, the verifier copies all candidate sources into a temporary consumer directory,
adopts them through `lenso app add`, runs each explicit schema operator, issues
two short-lived test credentials, builds the distribution, deletes every source
copy, clears `PATH`, and exercises the real HTTP listener. It checks unauthenticated
rejection, stale configuration rejection, cross-user denial, upload, processing,
Jobs disable/enable and full Plugin Root removal with preserved business
records, settings, and user isolation, plus a Host restart. While Jobs is
disabled or removed, the same TypeScript Plugin follows its declared optional
dependency and completes deterministic excerpts inline. Removing Jobs moves
its Plugin Root to recoverable local trash; it does not erase the PostgreSQL
knowledge records or uninstall the immutable Host distribution. A separate
versioned upgrade requires two exact signed Releases and is not implied by
this removal check.

The Jobs check uses a five-second readiness deadline for the exact newly
created job to reach `succeeded` with one attempt. A queue claim can initially
find no due job when the Host and disposable PostgreSQL clocks differ, or
process older work first; neither outcome is treated as success for the new
note.

### Local workerd `/settings` persistence slice

`verify_workers_settings.py` is one cross-target HTTP corpus for the same
business settings source and the same disposable PostgreSQL rows. Run the
Native verifier with `--workers-settings-handoff /private/path/handoff.json`.
At the handoff it has issued two high-entropy credentials through the real
Auth Plugin operator and is holding the Native App open. The handoff is a
mode-0600 file containing the two tokens; do not print it, copy it into the
Workers distribution, or commit it. The verifier removes its own handoff
automatically on timeout or shutdown, and resumes only after the acceptance
operator removes that exact file.

The selected Workers App must be built by the specialized Rust builder from
the exact locally verified `lenso.reference.knowledge-settings` Bundle and
the `@lenso/workers-runtime` 0.1.5 candidate. It is a separate Plugin/Instance
from the full Native knowledge-base Plugin; only `/settings` is portable.
Build and run the local workerd App, its dedicated settings bridge, and a
disposable PostgreSQL instance in one task-owned Linux network namespace.
Publish the PostgreSQL and workerd listeners to **host 127.0.0.1 only** so
Native and this corpus can reach them; leave the bridge bound to the same
namespace's **127.0.0.1 only** and do not publish or proxy it. Set the
Worker's explicit `KNOWLEDGE_SETTINGS_BRIDGE_ORIGIN` to that bridge's printed
`http://127.0.0.1:<port>`. Configure both Native and bridge for that one
disposable PostgreSQL database. No production Workers Auth/DB Capability is
implied by this local sidecar.

If ArcBox's ordinary host port publisher accepts but resets connections, keep
all three processes in the same **internal** task-owned Docker network and use
`verify_workers_settings_tunnel.py` only for PostgreSQL (5432) and workerd
(8787). Run one tunnel per port, with the exact namespace-anchor container:

```sh
python3 verify_workers_settings_tunnel.py \
  --container lenso-kb-settings-ns-925a --inside-port 5432
python3 verify_workers_settings_tunnel.py \
  --container lenso-kb-settings-ns-925a --inside-port 8787
```

Each prints its ephemeral host `127.0.0.1` listener. It refuses non-internal
or multiply connected networks and never forwards the private bridge. The
Native App uses the PostgreSQL listener; this corpus uses the workerd
listener. Record those listener ports and the container's observed ID in the
receipt. Do not substitute a public or `0.0.0.0` proxy.

If ArcBox cannot exec into the namespace anchor, `--container` may instead
select its task-owned `lenso-kb-settings-pg-925a` container. The tunnel then
uses that image's `nc`, after verifying that PostgreSQL shares the exact
internal anchor namespace and has no second network. The allowed ports stay
5432 and 8787; the bridge remains inaccessible from the host.

After the Native handoff appears, create the short-lived Host policy from
the Auth-issued credentials without exposing raw tokens to the Guest:

```sh
python3 verify_workers_settings.py prepare-policy \
  --handoff /private/path/handoff.json \
  --policy /private/path/bridge-policy.json
```

Bind-mount that mode-0600 policy **only** into the bridge process and set
`LENSO_KNOWLEDGE_BRIDGE_AUTH_POLICY` to its in-container path. Give the bridge
the same disposable PostgreSQL URL through `LENSO_KNOWLEDGE_DATABASE_URL`.
The policy has a 30-minute default expiry and maps only `user-a` and `user-b`
to SHA-256 token digests; source Auth issues the opaque token with 32 bytes of
OS randomness. Do not use low-entropy credentials with an unsalted digest.

With `workers-build.json` from that same workerd distribution, run the two
phases. `--workerd-generation` must be the observed start identity of the
task-owned workerd process/container, not an invented label; record both
values in the final receipt. Restart **only** that workerd process/container
between phases, leaving the bridge, Native App, and PostgreSQL intact:

```sh
python3 verify_workers_settings.py before-restart \
  --handoff /private/path/handoff.json \
  --workers-url http://127.0.0.1:WORKER_PORT \
  --workers-build /absolute/path/to/dist-workers/workers-build.json \
  --state /private/path/settings-state.json \
  --workerd-generation OBSERVED_FIRST_START
python3 verify_workers_settings.py after-restart \
  --handoff /private/path/handoff.json \
  --workers-url http://127.0.0.1:WORKER_PORT \
  --workers-build /absolute/path/to/dist-workers/workers-build.json \
  --state /private/path/settings-state.json \
  --workerd-generation OBSERVED_SECOND_START
```

The state file is created mode 0600 without credentials. The corpus verifies
two-user isolation, Native write → workerd read/CAS → Native readback, stale
and malformed requests, optional/no-key CAS, replay and payload conflict,
cross-target same-key concurrency, query-path parity, and settings plus
idempotency persistence after the workerd restart. It rehashes the Bundle,
Component, Jco core, and pinned runtime modules against the build receipt;
the operator must additionally record the actual workerd command, version,
listener, namespace, selected SHA/Plan, and process start evidence. Remove
the handoff to release the Native verifier only after both phases; then
remove the local policy and stop only the task-owned processes. A passed
corpus is local-workerd target evidence, not full-App or production support.

### Exact package inputs for external providers

The default command above retains source-checkout adoption. To test the separate
package-consumer gate, use `--package-only` instead of all three `--*-source`
arguments. Supply one signed linked Cargo snapshot and its independent public
trust file, plus the exact `.crate` archives and versions for
`lenso.auth.api-token`, `lenso.jobs`, and `lenso.secrets.env`:

```sh
CARGO_HOME=/scratch/cargo-home LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso --package-only \
  --linked-snapshot "$SIGNED_SNAPSHOT" --trust "$CATALOG_TRUST" \
  --auth-version "$AUTH_VERSION" --auth-crate "$AUTH_CRATE" \
  --jobs-version "$JOBS_VERSION" --jobs-crate "$JOBS_CRATE" \
  --secrets-version "$SECRETS_VERSION" --secrets-crate "$SECRETS_CRATE"
```

For the repeated Host and adopted Auth/Jobs operator builds in this verifier,
`LENSO_REFERENCE_CARGO_TARGET_DIR` may name one absolute, task-owned,
non-symlink directory. This reuses Cargo build artifacts across the operators
and the main, unadopted, and upgraded distributions while keeping the sandbox
`CARGO_HOME`, offline source, package locks, and each Host source/receipt
separate. Without the variable, adopted operators retain separate
`operator-targets/<provider>` directories. Inspect ownership and running
processes before later cleaning a reused target; a build cache is not
publication evidence.

To exercise a controlled Secrets version switch, include the second Release
in that same signed snapshot:

```sh
CARGO_HOME=/scratch/cargo-home LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso --package-only \
  --linked-snapshot "$SIGNED_SNAPSHOT" --trust "$CATALOG_TRUST" \
  --auth-version "$AUTH_VERSION" --auth-crate "$AUTH_CRATE" \
  --jobs-version "$JOBS_VERSION" --jobs-crate "$JOBS_CRATE" \
  --secrets-version "$SECRETS_VERSION" --secrets-crate "$SECRETS_CRATE" \
  --secrets-upgrade-version "$NEXT_SECRETS_VERSION" \
  --secrets-upgrade-crate "$NEXT_SECRETS_CRATE"
```

The verifier first runs the App with the selected Secrets version. It then
rejects a mismatched archive without changing that selection, uses signed
`lenso app add --replace` to select the second version, and builds a new Host.
`app check` and `app show` must pass for the new distribution. After a
controlled stop and restart, existing notes, settings, and Jobs history must
remain available, cross-user denial must still hold, and a new Job must
complete. The old source and App-owned Secrets configuration remain intact.
This tests a planned restart, not live request draining, a Jobs Plugin upgrade,
database migration, or automatic rollback. Neither Release is supplied by
this repository; a local test catalog does not establish official listing.

Each version must be an exact Cargo version, not `latest` or a range. The
verifier rejects source-checkout arguments in this mode, copies no sibling
Plugin repository, and calls `lenso app add PLUGIN_ID@VERSION` with the signed
snapshot, trust file, and matching local archive. The CLI verifies the catalog
signature, exact selected release, target, package and Plugin identity, and
archive digest before vendoring source into the disposable App. It does not
download the archive or prove its crates.io provenance. The build still
requires a compatible published dependency closure and may execute package
build scripts; review untrusted package source and build in an isolated
environment. The App's own Rust and TypeScript business Plugin source remains
part of this fixture, so `--package-only` describes its external provider
inputs, not a binary-only App build.

The source-mode candidate arguments are rejected in `--package-only`. The
verifier regenerates only the disposable business App copy's `Cargo.lock`
offline from the sandbox's configured packaged Cargo source, then builds its
operator with `--locked --offline`; it reports that generated lock's SHA-256.
The checked-in source-candidate lock is unchanged. This local step does not
prove that the exact framework and Agent Tool Provider versions are published
or visible on crates.io; public registry availability needs separate readback.

The verifier does not accept detached Auth or Jobs operator executables. After
`app add` verifies each exact archive and vendors its source, the verifier checks
that the adopted lock names the selected Plugin, version, and input `.crate`
digest, and that the complete
vendored source still matches its recorded source digest. It requires the
Auth `examples/api-token-operator.rs` and Jobs `examples/jobs-operator.rs`
inside those adopted packages, builds them offline from the vendored manifests,
checks the lock and source digest again, and copies the resulting executables
to the disposable consumer before invocation. Missing packaged examples or
offline dependencies fail the gate. The knowledge-base operator remains built
from the App-owned fixture source. Full App assembly waits until the Auth
operator has produced the public key used by its configuration. After
`app build`, the verifier also runs
`app check` and `app show` on the built distribution and requires all three
external provider Instances to be present before deleting the source tree.
In a separate disposable copy of the complete App, it verifies the adopted
Jobs lock against the exact `.crate`, selects the optional Jobs dependency as
absent, restores only that probe's generated, unmodified Jobs Instance intent,
and runs `lenso app unadopt lenso.jobs@VERSION`. It checks that the selected
source and intent move to recoverable trash while Auth and Secrets remain
selected. The prior Host authority validates the explicit optional-dependency
selection, then is removed before a fresh `app build`. The verifier runs
`app check`/`app show` on that new distribution and processes a real note
through the no-Jobs path. This does not alter the main
runtime-removal scenario. The optional Secrets upgrade probe requires a
separate exact Release; Jobs removal alone does not prove an upgrade.

The signed directory binds the operator's own source to the selected `.crate`;
it does not sign the resulting machine code or its transitive dependencies, or
prove a reproducible build. The operator build may execute package build
scripts and must run inside a single-owner, network-disabled container with
read-only original release inputs and toolchain, a scratch-only writable disposable
consumer, an explicit
allowlisted environment, and CPU, memory, process, file-size, disk, and time
limits. Set `CARGO_HOME` explicitly to a sandbox-local, credential-free offline
cache; the verifier gives Cargo a scratch-local `HOME` and does not pass the
database URL or signing secrets to the operator build. `--offline` alone is not
a sandbox. The `operator_receipts` measurement records each selected crate,
source, Cargo.lock, and operator binary digest; the lock and binary digests are
observations, not catalog signatures. Do not execute this verifier
against an unreviewed release on the host. The checks cannot defeat a malicious
concurrent writer outside that container.

Inside the same sandbox, run the narrow argument and preflight checks without PostgreSQL using
`python3 -m unittest discover -s . -p 'test_verify_*.py'`. No exact published
Auth/Jobs/Secrets package cohort, verified crate-derived operators, or second
versioned Release is bundled with this fixture. These tests alone do not
establish package-only, public-registry, or upgrade acceptance.

After a successful run, `verify.py` prints one `MEASUREMENT` JSON line. Its
phases separate consumer preparation, candidate Plugin operator setup, App
authoring, optional frontend authoring, and the final `lenso app build`. Each
phase records elapsed seconds and the change in logical regular-file bytes
inside the disposable consumer directory; `distribution_bytes` is the built
distribution's logical size. The OS, machine architecture, and CLI/Cargo/Bun
versions identify the measurement environment. These are single-run local
observations with ambient package caches, not controlled cold-build or warm
edit-to-ready numbers. The supplied CLI is already built, so framework
maintainer build time is outside this measurement; external package caches and
PostgreSQL storage are outside the disk count. Host recompilation count and
cross-machine performance remain unmeasured. In default mode, Auth, Jobs, and
Secrets are source candidates, so those figures do not establish the separate
package-only consumer or publication gates. The `provider_input_mode`
measurement field distinguishes the modes.

For an interactive browser pass, add
`--browser-handoff /tmp/lenso-browser-handoff.json`. After the scripted checks,
the verifier exclusively creates a mode-`0600` JSON file containing the
temporary URL and test credential, then keeps the source-deleted Host running
for up to ten minutes. The parent directory must exist and cannot be
non-sticky writable by other users; an existing file or symlink at the target
path is refused. Delete the handoff file after browser automation completes;
the verifier then reads the browser's final excerpt policy, checks later inline
processing against that value, performs its restart checks, and shuts down
normally. On timeout or `KeyboardInterrupt`, it attempts to remove the still-present file
after checking its inode. On POSIX, a handoff running on the main thread also
temporarily catches `SIGTERM` so normal cleanup can run, then restores the
previous handler. These are best-effort safeguards: deletion is not atomic with
the inode check against a process running as the same user, and `SIGKILL`, a
process crash, or power loss can leave the file behind. If it remains, stop
the verifier and manually remove it only after confirming that the path still
names the handoff file; do not delete a replacement path. Do not reuse the
temporary test credential.

No App-authored Host, Plan, Runtime Profile, binding document, database URL, or
credential value is checked in. The default acceptance uses local candidate
sources; this README does not present Auth, Jobs, or Secrets as
registry-installed defaults. Public publication and deployment are separate,
unauthorized steps.
