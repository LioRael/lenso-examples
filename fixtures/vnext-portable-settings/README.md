# Portable endpoint → Settings Store

This is a **source fixture proof**, not a completed generic
`lenso app build --target workers` pipeline, deployable App, Auth integration,
or full Knowledge Base port.
It contains two business Plugins and one infrastructure Plugin:

```text
HTTP ingress → settings HTTP endpoint → SettingsStore@1
                                           |
                              private PostgreSQL persistence
```

Both Native and Workers run the same Rust endpoint, generated Capability
Client/Provider, storage validation, Host Catalog, Plugin Root snapshot, and
public `resolve_plugin_root` lowering. The resulting immutable Plan has three
distinct Instances and two bindings. No handwritten derived Plan or second
resolver exists. Removing the storage Instance makes resolution fail.

Workers links these Plugins into **one wasm-bindgen Host artifact**. Artifact
count is not Plugin count. `NativePluginRegistry` is the source-linked
Execution Adapter inside this Host; `WorkersDriver` provides its event loop.
This does not qualify native packages generally for Workers target admission.

## Ownership and contract

| Owner | Source | Responsibility |
| --- | --- | --- |
| SettingsStore role | `crates/settings-contract/` | `example.settings-store@1` version `1.0.0`; single Descriptor/Schema owner and generated Rust projection |
| HTTP endpoint Plugin | `plugins/settings-http/` | `#[endpoint]` handlers and typed `Port`; no database/bridge access |
| Storage Plugin | `plugins/settings-store/` | Value validation, CAS, durable idempotency receipts, atomic PostgreSQL transactions |
| Host | `src/composition.rs`, `src/bin/native.rs`, `src/workers.rs`, `worker.mjs` | Availability, public composition lowering, Runtime Driver, private resource injection, listener/event startup and shutdown |
| Local persistence service | `src/bin/bridge.rs` | Reuses the storage Plugin's exact Rust PostgreSQL implementation; fixed read/change operations only |

The first version has no previous compatibility baseline. The role is portable,
but cross-lane transfer is disabled: both business Instances inhabit one
source-linked execution lane. Generated Rust projection freshness is checked
by the contract package's `build.rs`; do not edit its `src/generated.rs`.

`read(principal)` returns `{revision,value}`, initially `{0,""}`.
`change(principal,expected_revision,idempotency_key,value)` returns the committed
snapshot. Principal and key are 1–64 ASCII alphanumeric/hyphen bytes; value is
1–256 UTF-8 bytes without control characters. Revisions are bounded integers.
Successful retries return the original receipt, including after later writes.
Changing any input for an existing key returns `idempotency_conflict`; a stale
revision returns `conflict`. Invalid values return `invalid`. Infrastructure
failure remains a Runtime Failure, not a fabricated Domain Error or memory
fallback. An owner-row lock serializes CAS and receipt lookup/insertion in the
same transaction, including concurrent first writes.

`x-local-test-principal` is deliberately **untrusted test data used as a key**.
It is not authentication or authorization, and key separation is not authenticated
tenant isolation. Source-linked code and the local bridge are trusted.
Production authorization and resource provisioning are outside this fixture.

No historical KB code is copied or imported. That code's SQLx/Tokio/Auth ownership
is inappropriate for this bounded product-neutral Wasm slice. The new storage
Plugin owns its independent string setting, tables, and receipts; there is no
second authority over the historical KB's facts.

## Private infrastructure

Native injects direct PostgreSQL into the storage Plugin. Workers injects a
Host-owned JS closure into that same Plugin, after the endpoint's Plan-bound
typed call. The closure fixes an explicit loopback origin, accepts only
`read`/`change`, refuses redirects, bounds bodies, and attaches its authorization
header. Credentials never enter Wasm, portable values, or the endpoint.
There is no public fetch/database Capability. The loopback service runs the
same Rust storage transaction implementation used by Native.

Workers bounds inbound body reads to 4096 bytes and three seconds, cancels
unfinished reads on timeout or client cancellation, and distinguishes 408
timeouts from 413 size violations. The persistence response shares the
outbound fetch's three-second deadline rather than acquiring a fresh budget.

The Native entrypoint installs the existing `WebIngressFactory`, binds
`127.0.0.1:0`, and prints its HTTP origin only after Kernel startup succeeds.
No fixture-owned HTTP server or router sits in front of the business Plugins.
SIGTERM/Ctrl-C performs bounded Kernel shutdown, including managed ingress work.
Workers accepts real HTTP in local workerd and creates/shuts down an App per
event. The durable harness sends actual HTTP to both targets, recreates the
Native process and workerd, and verifies persisted state and original receipts.

## Generic builder handoff

The Host assembly seam remains `composition::plan()` plus
`composition::registry(Rc<dyn storage::Persistence>, WebIngressEventFactory)`.
Each business Plugin is now a separate source package with Cargo metadata
identity and `#[lenso::plugin]`. The HTTP package declares its Store requirement
as `Port<SettingsStoreClient>` and its routes as `#[endpoint]` handlers.
The Store uses generated `#[lenso::provides]` Capability endpoints. There are no
hand-maintained Plugin Descriptors, route tables, dispatch implementations, or
dependency-connection lifecycle methods.

The Host calls `NativePluginDefinition::link()` explicitly for constructor-free
Wasm targets, uses the generated linked catalog/factories, and installs a
`ConfiguredPluginFactory` override only to attach the Host-owned private
`Rc<dyn Persistence>`. The existing Web Ingress factory is the third Plugin.
`root()` still supplies a fixture-owned in-memory Plugin Root snapshot to the
public resolver; no derived Plan or bindings are authored here.

**The generic Engine Workers builder is not implemented.** Separate Cargo
package identities and generated linked factory/catalog discovery are proven.
Filesystem source App discovery, CLI package validation, an immutable Host
Catalog artifact, and a filesystem Plugin Root loader are not proven here. Generic
source build integration still needs to discover the packages, attach the
selected provider's private resource from explicit Host Environment
infrastructure, and generate Native/Workers entrypoint/bundling assembly.
It must preserve the two business Instances and typed binding; it must not
replace them with a single endpoint proxy or branch on these fixture IDs.
`src/bin/native.rs` and `src/workers.rs` demonstrate target assembly. Native
uses the existing ingress listener; the Workers event envelope and
`worker.mjs` loopback configuration are fixture-specific.
The bridge transport is an intentionally local PostgreSQL resource, not a
general-purpose Workers database integration or production security boundary.

HTTP body decoding uses the SDK's `Json` extractor: malformed/unknown body
fields return 400 and non-JSON media types return 415. Domain validation and
conflict status mappings remain unchanged. `Headers` extracts the local-test
principal without making an Auth claim. Workers forwards the URI to real
ingress rather than dispatching `/settings` in JavaScript.

## Reproduce

Prerequisites: Python 3.11+, Cargo with `wasm32-unknown-unknown`, PostgreSQL
`initdb`/`pg_ctl`, Node, npm, `wasm-bindgen` **0.2.127**, and
`lenso-contract-codegen` **0.10.0**.

Set `LENSO_FRAMEWORK_ROOT` to a clean Rust checkout at
`6c88aeda06c5da7aed9318863fe2eba189b6c236` (including the final lifecycle fixes).
That SHA remains the recorded default proof input. To explicitly replay another
candidate, set `LENSO_FRAMEWORK_REVISION` to its full lowercase 40-character SHA.
`verify.py` requires that exact HEAD and no tracked changes or nonignored
untracked files, both before and after verification, and prints the actual SHA
on success. An override selects
an input to test; matching dependency versions alone do not establish compatibility.
The verifier copies this workspace to a temporary directory and adds candidate `[patch.crates-io]` entries there
only. No local paths are stored in the fixture manifest.
The Cargo-generated workspace lock is retained; ordinary checks/builds use
`--locked`. `python3 verify.py lock` is the explicit lock regeneration operation
against this exact framework source, not an implicit update during verification.

```sh
cd fixtures/vnext-portable-settings
export LENSO_FRAMEWORK_ROOT=/path/to/lenso
python3 verify.py check
python3 verify.py build
```

The Workers clock is an explicit historical **source** dependency, not a claim
that package `0.1.5` is published. Extract it from a checkout containing this
commit without switching that checkout:

```sh
mkdir -p .runtime
git -C /path/to/lenso-js archive \
  b2b3122f0061a649de29d1fa6d7c2fb30076f3a7 \
  packages/lenso-workers-runtime | tar -x -C .runtime
export LENSO_WORKERS_RUNTIME_ROOT="$PWD/.runtime/packages/lenso-workers-runtime"
npm ci --ignore-scripts --no-audit --no-fund
WRANGLER_SEND_METRICS=false python3 test_durable.py --workers
```

Miniflare is locked to `4.20260701.0` and runs real workerd. Only the runtime's
unchanged clock module is linked, not its generic Host/build pipeline.
The harness compiles/tests before starting PostgreSQL, creates its own temporary
cluster bound exclusively to `127.0.0.1`, generates a fresh bridge token in
process memory, and stops/reaps its processes in `finally` blocks. It ignores
ambient database URLs. No user database, Cloudflare login, deploy, or publication
is involved. If a local Cargo wrapper reports a Unix socket path-length error,
run with a short `TMPDIR` such as `/tmp`.

Proof covers derived bindings/removal, Kernel startup before any database,
read/CAS/replay/conflict/invalid input over actual Native and Workers HTTP,
concurrent identical writes through two Native HTTP Hosts,
cross-target reads and writes, separate data keys, workerd recreation, original
receipt replay after a later write, bridge rejection without authorization, and
both targets' fail-closed behavior after stopping the owned database.

The source-authoring local proof used Cargo 1.98.1, wasm-bindgen 0.2.127, Node 26.5.0,
PostgreSQL 18.6, Miniflare 4.20260701.0, and its workerd 1.20260701.1.
It passed workspace Rust tests, native and Wasm builds, generated-projection freshness,
the durable corpus above, Rust formatting, JS syntax, and Python compilation.
The explicit framework source input was clean
`6c88aeda06c5da7aed9318863fe2eba189b6c236`; this is local source evidence,
not candidate CI. Both business packages use generated source authoring in real
workerd, including malformed-body/media-type rejection and ingress 404s.
The final replay against clean `3e56a3f34ef2f71f9b7a5d1e141e1d4c8d8de184`
also exercised an unfinished upload returning 408 in real workerd. Reader tests
cover cancellation, oversized/invalid bodies, and an unresponsive cancellation
hook. An incremental contract check rejected a generated-only edit and passed
again after restoration; the projection itself was not changed.
PostgreSQL operations drive their connection in the same owned future with a
two-second bound; completion/cancellation drops both, with no detached driver task.
The Store's generated lifecycle rejects a missing Host resource before Ready;
this does not connect to PostgreSQL or treat database availability as readiness.
An attached but unavailable backend still fails closed on each request.
All owned PostgreSQL, bridge, and workerd processes were stopped/reaped; only
ignored build artifacts, extracted clock source, and fixture-local npm tools
remain.

Regenerate the contract with `python3 verify.py generate`. Dependencies are
versioned; candidate Rust patch resolution is intentionally local source testing,
not publication proof. No changes are needed in the existing KB fixture or
repository-level `node_modules`.
