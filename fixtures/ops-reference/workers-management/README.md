# Workers management source assembly

This source wrapper selects the same Management core and generated security
ports as the Native reference App. It adds no business storage or permission
engine. The ordinary build must combine the existing OpsState@2 source owner
with the `app/management/{core,http,secrets}` tree here and the explicitly
selected Worker security owners. Keep the plain Ops Web endpoint disabled so
all business writes pass through Management.

The initial Worker profile accepts API credentials issued by the API-token
owner with `actor_kind=user`. It supports catalog/read, an immutable pending
write, a different qualified human's typed approval, original-operation commit,
status and receipt/audit recovery. Human PAT issuance, browser password sessions
and scoped Account delegation retain their separately qualified Native profile.

The Root chooses exact source revisions in Cargo and named dependency providers:

| Consumer | Dependency | Bound owner |
| --- | --- | --- |
| Management | `state` | `example.ops-state/primary` |
| Management | `credential_state` | `lenso.auth.api-token/default` |
| Management | `access` | the API-key-trusting Access owner |
| Management | `approval` | selected D1 Approval; None only in explicit read-only |
| Management | `audit` | selected D1 Audit |
| HTTP | `auth` | `lenso.auth.api-token/default` |
| HTTP | `management`, `human` | `example.ops-management/default` |

Host facilities select only the named private bindings:

- Management `authority`: `MANAGEMENT_DB`, `{profile:"workers-d1"}`; the exact
  reachable `lenso-management-core` package owns `src/workers/journal.mjs`.
  `workers-clock=true` supplies the same event Driver's deadline domain.
- HTTP optional `mcp`: its event-scoped closed SDK module is owned by the exact
  reachable `lenso-management-http` package at `src/workers/mcp.mjs`.
  Configuration selects `read-only` or explicit `approved-writes`; the selected
  `MCP_PROFILE` string is public configuration and grants no database access.
- Secrets `secrets`: string binding `OPS_AUTH_SECRETS_JSON`; non-secret
  configuration supplies exact API caller plus signing and pepper references.
  The private string contains exactly those two bounded values. No secret value
  belongs in the grant file, Plugin config, generated plan or qualification log.

Explicit owner setup prepares each D1 database and grants exact Management
qualification before deployment. Runtime constructors only inspect prepared
versions. The Management event budget must be at most 30 seconds; durable claims
use absolute wall-clock expiry rather than persisting a relative Driver time.

Source files are under construction until exact cohort manifests, an ordinary
CLI build and real security graph receipts exist. Component D1 and protocol
checks alone do not qualify this source assembly or remote deployment. Remote
Hyperdrive remains outside the requested acceptance for this run.
