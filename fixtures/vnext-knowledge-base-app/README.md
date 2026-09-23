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

The React source under `frontend/` consumes generated types and the browser
runtime from an independently packed `@lenso/web-client` candidate. The
checked-in static assets keep the App build self-contained. To regenerate them:

```sh
python3 verify.py \
  --cli /absolute/path/to/lenso \
  --web-client-package /absolute/path/to/lenso-web-client.tgz \
  --auth-source /absolute/path/to/lenso-auth-plugin/crates/lenso-auth-api-token-plugin \
  --jobs-source /absolute/path/to/lenso-jobs-plugin/crates/lenso-jobs-plugin \
  --secrets-source /absolute/path/to/lenso-secrets-plugin/crates/lenso-secrets-env-plugin
```

## Acceptance

Use a disposable PostgreSQL database. The verifier creates App-owned schemas
and intentionally leaves them available for inspection; it does not delete or
reset the supplied database.

```sh
LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso \
  --auth-source /absolute/path/to/lenso-auth-plugin/crates/lenso-auth-api-token-plugin \
  --jobs-source /absolute/path/to/lenso-jobs-plugin/crates/lenso-jobs-plugin \
  --secrets-source /absolute/path/to/lenso-secrets-plugin/crates/lenso-secrets-env-plugin
```

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
versioned upgrade still requires an exact consumable second Release and is
not implied by this removal check.

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
runtime-removal scenario. A separate Release remains necessary to prove a
versioned upgrade.

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
versioned Release is bundled with this fixture. Until those inputs exist and
the full verifier succeeds, the package-only, public-registry, and upgrade
acceptance gates remain unverified.

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
the verifier writes a mode-`0600` JSON file containing the temporary URL and
test credential, then keeps the source-deleted Host running for up to ten
minutes. Delete the handoff file after browser automation completes; the
verifier will then perform its restart checks and shut down normally.

No App-authored Host, Plan, Runtime Profile, binding document, database URL, or
credential value is checked in. The default acceptance uses local candidate
sources; this README does not present Auth, Jobs, or Secrets as
registry-installed defaults. Public publication and deployment are separate,
unauthorized steps.
