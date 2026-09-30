# Private Workers Owner operator

This supplementary test Worker performs explicit Owner setup and fixture-user
credential operations. It is separate from the ordinary source-authored business
Host. It does not qualify that Host or publish a setup Capability.

Build with the repository's normal Cargo configuration:

```sh
cargo check --target wasm32-unknown-unknown
cargo test --locked
python3 build.py
python3 stage_assets.py --sources /private/tmp/selected-owner-sources.json
```

`stage_assets.py` requires each selected checkout to be clean at its exact SHA.
Its input maps `auth`, `access`, `audit`, `approval`, `management`, and `runtime`
to `{ "root": "<checkout>", "sha": "<full SHA>" }`. It copies only tracked
Owner adapters and the selected Workers runtime modules, recording byte hashes.
The generated Wasm includes the constructors and reset function required by the
Workers runtime. No toolchain override or source-path Cargo patch is introduced.

`build.py` records the operator manifest, normal Cargo lock, Rust sources,
Worker entry point and build/staging scripts, plus every generated package file.
It rejects source changes during compilation and requires the typed Audit
reader's source annotation to match the Audit Owner in the normal Cargo lock.
Preserve this complete build
directory for a newer source-selected remote preparation; a directory containing
only `pkg`, `owner-assets` and `worker.mjs` is insufficient for that path.

`prepare_remote.py prepare` without build-selection flags retains the historical
`b840e9c7` Wasm check. Select a newer build explicitly with both flags:

```sh
python3 prepare_remote.py prepare \
  --private-root /private/tmp/new-operator-private \
  --template-private-root /private/tmp/reviewed-template-private \
  --runtime /private/tmp/new-operator-runtime \
  --facts /private/tmp/new-deployment-facts.json \
  --source-runtime /private/tmp/checked-operator-build \
  --operator-build-receipt /private/tmp/checked-operator-build/pkg/build-receipt.json \
  --source-selections ../../candidate-inputs.json
```

The supplied receipt must be byte-identical to the build directory's receipt.
Its source hashes must match both that directory and the current reviewed
operator source. The selected Core revision must match the normal operator lock
and manifest patches. Five Owner revisions come from the current Worker core
manifest and normal locks: Auth, Access and Audit are compiled operator owners;
Approval and Management are selected JavaScript adapters, checked against the
normal Management lock. All five must match `owner-assets/sources.json` and the
actual staged bytes; the runtime must match the selected JavaScript revision.
Missing or partial flags, changed locks, sources, generated files or assets stop
preparation before private keys or runtime directories are created. There is no
bare expected-hash override. The private preparation receipt records the verified
build and asset receipt hashes. This preparation performs no deployment,
resource creation or Owner call, and does not promote historical qualification.

The private Worker selects five D1 bindings: `AUTH_DB`, `ACCESS_CONTROL_DB`,
`AUDIT_DB`, `APPROVAL_DB`, and `MANAGEMENT_DB`. It also requires three private
string bindings: `OPS_TEST_OPERATOR_CAPABILITY`, `OPS_AUTH_CONFIGURATION_JSON`,
and `OPS_AUTH_SECRETS_JSON`. The last is exactly the signing-reference and
pepper-reference string map admitted by the Auth Owner configuration. Native
file Secrets do not serve as a Worker fallback.

The only path is `POST /_qualification/owners`. It requires the test capability
in a Bearer header, refuses Origin, Cookie, and query parameters, and returns
`Cache-Control: no-store`. It never logs requests, responses, or exceptions.
Requests are finite `{ action_id, owner, operation, parameters? }` envelopes.
Access operations additionally receive a fixture credential privately.

| Owner | Operations | Boundary |
| --- | --- | --- |
| Auth | `setup`, `verify`, `public_key`, `issue`, `metadata`, `revoke_token`, `revoke_session`, `attenuate` | Existing migration and `ApiTokenAuthOperator` APIs; default Actor kind is `user`, with the existing `service-account` kind admitted only for fixture denial vectors. |
| Access | `setup`, `verify`, `bootstrap`, `revoke_role`, `list_roles` | Existing schema plan and actual typed Kernel dispatch. Bootstrap authenticates a real Owner-issued fixture user before protected grants. |
| Audit | `setup`, `verify`, `inspect_operation`, `verify_operation` | Existing finite Owner adapter exports; a separate deployment-scoped Kernel typed reader verifies persisted phases, actors, digest, and opaque receipt against the actual ordinary-App response. No append fixture or synthetic events. |
| Approval | `setup`, `verify`, `inspect_operation` | Existing finite Owner adapter exports. Read-only diagnostic shape/status inspection; no decision fixture. |
| Management | `setup`, `verify`, `grant`, `revoke` | Management-owned schema and qualification exports. No copied Auth identities or RBAC tables. |

Setup precedes issuance. The ordinary business Host starts only after every
Owner reports readiness; runtime handlers do not run migrations or grants.
Auth `public_key` derives the verifier through the Owner's public helper. The
runtime issuer, assertion public key, and exact Management CredentialState caller
must match the ordinary selected profile. Audit writers and Approval caller lists
must name the actual Management Plugin instance from that Host's catalog.

Auth issuance parameters are `subject`, `deployment`, `permissions`,
`resource_scopes: [{kind,id}]`, exact operation `audience`, and RFC3339
`expires_at`. The operator validates the SDK ceiling and stores Owner facts. It
does not invent a signed actor assertion. Access bootstrap parameters are
`subject` and `scopes: [{kind,id,roles:[{id,permissions,subjects}]}]`.
The subject must equal the independently authenticated fixture user. All input
shapes are checked before the first bootstrap write.

The optional `actor_kind` issue parameter accepts only `user` and
`service-account`. A subject label alone never establishes a machine vector.

The client uses a task-owned directory with mode `0700`. Its regular input and
credential files must have mode `0600`. `operator-profile.json` contains a fixed
URL ending in `/_qualification/owners` and the local `capability_file` name.
An action file may replace the wire credential with `credential_file` and an
issuance must select a new local `secret_output` name.

```sh
python3 client.py --private-root /private/tmp/task-owned-operator --action /private/tmp/task-owned-operator/issue-alice.json
```

For each mutation the client creates `<action_id>.started.json` exclusively and
fsyncs it before dispatch. It makes one Node fetch; redirects, retries, and
transport fallback are forbidden. Known success writes a redacted
`<action_id>.completed.json`; issuance saves its raw token once in the private
output file. A completed repeat returns local metadata without dispatch. An
interrupted or unknown action retains its marker and cannot be replayed, even
after a Worker or client restart. The isolate's bounded action fence is an
additional check, not durable storage.

`ApiTokenAuthOperator::issue` has no durable secret-replay receipt. If its reply
is lost, stop and inspect `metadata` under a new read action. Do not issue again
under a different key to conceal uncertainty. No response containing the raw
token is written to a receipt, browser storage, terminal, or model context.

The D1 setup and component receipts are distinct from standard registry archive
verification. Source-selected facilities require the coordinated Core SDK source
cohort. The standard archive check remains a release prerequisite until those
producer package versions are separately released. No registry publication is
authorized by this test operator.
