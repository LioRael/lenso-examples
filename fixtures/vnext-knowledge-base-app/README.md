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

The verifier copies all candidate sources into a temporary consumer directory,
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
LENSO_REFERENCE_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso --package-only \
  --linked-snapshot "$SIGNED_SNAPSHOT" --trust "$CATALOG_TRUST" \
  --auth-version "$AUTH_VERSION" --auth-crate "$AUTH_CRATE" \
  --jobs-version "$JOBS_VERSION" --jobs-crate "$JOBS_CRATE" \
  --secrets-version "$SECRETS_VERSION" --secrets-crate "$SECRETS_CRATE" \
  --auth-operator "$AUTH_OPERATOR" --auth-operator-sha256 "$AUTH_OPERATOR_SHA256" \
  --jobs-operator "$JOBS_OPERATOR" --jobs-operator-sha256 "$JOBS_OPERATOR_SHA256"
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

The Auth and Jobs schema operators cannot be obtained as executable tools from
the signed-catalog `.crate` admission API. Pass separately built operator executables
and their 64-character SHA-256 digests. The verifier checks their executable
bits and digests before work begins, copies them into the disposable consumer,
and verifies the copies again before invocation. These digests pin the supplied
operator bytes but do not establish their provenance or bind them
cryptographically to the catalog releases. Build and review the operators from
the approved release cohort, then record that provenance separately; do not
call this an all-signed-binary supply chain proof. The knowledge-base operator
is built from the App-owned source fixture.

Run the narrow argument and preflight checks without PostgreSQL using
`python3 -m unittest discover -s . -p 'test_verify_*.py'`. No exact published
Auth/Jobs/Secrets package cohort, matching operator binaries, or second
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
