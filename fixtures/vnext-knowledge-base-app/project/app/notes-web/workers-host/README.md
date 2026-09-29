# Local knowledge-settings Workers Host

These Plugin-owned sources implement the private
`lenso:knowledge-settings-local@1.0.0/plugin` Host integration. They are
explicitly trusted Host code, not Bundle-signed code, a security sandbox, or
production Workers qualification. The Engine independently verifies the
selected Bundle, Plan, Component and separately pinned generic runtime.

The bridge is preserved byte-for-byte from `lenso-js` commit `b2b3122`:
`sha256:dfbbf31eea1db4c1cc17496316b0d9529fe45661abcd2dda80dcd00077a3711e`.
It imports `component-admission.mjs` from the generic runtime; this Plugin
does not vendor that library. Only the worker entrypoint's generated artifact
import changed from `knowledge-settings-artifact.mjs` to `artifact.mjs`.

## Prepare and approve

Use `prepare_integration.py` beside these sources with explicit paths to the
complete manifest and Component extracted from an existing, verified Bundle:

```sh
python3 prepare_integration.py \
  --manifest /absolute/path/to/extracted/manifest.json \
  --component /absolute/path/to/extracted/plugin.component.wasm \
  --output /absolute/path/to/new-integration
```

The manifest must be the complete document exported by the Bundle tooling,
not a source manifest or a manually reconstructed document. Whitespace and
object key order do not affect its digest: the helper sorts object keys and
serializes compact JSON before hashing. Duplicate keys, floating-point values,
and non-finite numbers are rejected. Minimal hand-authored manifests with
omitted defaults are not supported; the helper never invents defaults.
The helper checks the Plugin identity and Component header, hashes the
normalized manifest, exact Component bytes, and three fixed sources,
then publishes a new directory containing only the profile and declared files.
It neither resolves implementations nor verifies Bundle signatures or runs
Plugin code. The Engine's final Bundle/Plan admission is authoritative; a
mismatched manifest or Component digest fails there. No credentials are read.

Review the source and printed profile digest before granting Host approval:

```sh
lenso app build --target workers --workers-runtime /explicit/runtime \
  --jco /explicit/jco \
  --workers-integration /absolute/path/to/new-integration/integration.json \
  --trust-workers-integration sha256:PRINTED_PROFILE_DIGEST
```

The runtime version is pinned separately to `0.1.5`. The integration profile
binds the `lenso.reference.knowledge-settings/default` Instance to those
manifest, Component and source bytes. Preparation and approval do not deploy.

## Local acceptance

Start the task-owned PostgreSQL database, then run the settings bridge and
workerd in the **same Linux network namespace** so workerd's Fetch to
`127.0.0.1` reaches that bridge. Do not expose the bridge on `0.0.0.0` or add
a host-network proxy. Configure the bridge with a short-lived, mode-0600
Host-owned policy mapping SHA-256 digests of two high-entropy opaque Auth
Plugin tokens to their Native user IDs. Neither raw tokens nor the policy
file belong in the integration or distribution.

Pass the exact loopback origin as `KNOWLEDGE_SETTINGS_BRIDGE_ORIGIN`. The
adapter admits only the KB `/settings` read/CAS protocol, rejects redirects,
uses a bounded deadline without retries, and never passes Bearer credentials
to the Guest. Its four-export admission and Guest business validation remain
unchanged. It provides no generic database import, implicit network fallback,
production Workers Auth, Jobs, Secrets, or PostgreSQL Capability.

The generic `workers-build.json` receipt records the integration separately
from the generic runtime. Its `profile_file` identifies the byte-for-byte
`workers-integration.json` copy; the verifier checks its profile digest and
metadata against the receipt as well as the declared output file hashes.
Run the fixture's Native → local workerd → Native PostgreSQL corpus before
claiming cross-target persistence.

## Bridge regression tests

```sh
python3 test_bridge.py --lenso-js /explicit/historical/lenso-js-checkout
```

Use a trusted checkout containing the `0.1.5` runtime (historical commit
`b2b3122`). The runner stages this bridge and its unchanged historical tests
with only the generic admission/request modules in a temporary directory,
then runs Node's test runner. It does not fetch, install, or deploy anything.
