# Optional management source roots

These roots wrap the neutral Management service with ordinary Lenso providers
and named owner ports. The business target uses the generated OpsState client;
security calls use Auth, Access Control, Approval and Audit clients. Private Host
facilities supply the verifier, prepared qualification/journal stores and the
Host-created clock. Setup belongs to the explicit operator and private bootstrap
selection, never runtime DDL or the public HTTP routes.

The plain reference App disables these roots. The qualification scripts select
an API/PAT profile or a separate Account/browser profile, disable the plain
business write endpoint, and record their actual selected providers. Follow the
fixture-level guide for configuration and supported receipts.

## Read-only management

The explicit `read_only = true` management configuration exposes only
`state.read`. Select `approval: null` for its named dependency and omit the
Approval owner from that App. Auth credential state, operator qualification,
Access Control and Audit remain bound. The authority also rejects write entries
if a caller bypasses the catalog. Human approval cannot run in this profile.

The default write configuration requires a bound Approval owner during startup.
It cannot become active merely because its dependency is optional in the shared
source descriptor. The installation verifier qualifies both the real read-only
App and rejection of a write profile with the same missing owner.

## Scoped independent Agent connection

The HTTP root has two optional Host configuration strings. They hold complete
JSON objects with known fields, bounded to 8 KiB; callers cannot supply policy
objects in HTTP request bodies.

- `scoped_issuance_json` belongs to the Account/browser profile. It contains
  `deployment`, `task_id`, `agent_session_id`, `delegate_caller`, `entries`,
  `audience` and `max_ttl_seconds`. Each entry contains `permission`, `entry_id`,
  `scope_kind` and `scope_id`. The authority catalog must still admit every entry.
  Audience values are restricted to Management catalog/invoke/status and TTL is
  at most fifteen minutes. The optional named `delegation` dependency binds the
  selected Account owner.
- `delegated_session_json` belongs to the explicitly selected
  `credential_scheme = "delegated_session"` target. It contains `realm`,
  `issuer`, `public_key`, `max_assertion_ttl_seconds`, `task_id`,
  `agent_session_id` and `delegate_caller`. Realm is `operators`; issuer and key
  are the explicit Account owner trust profile.

A current browser session may POST `/auth/delegations/scoped` with only
`idempotency_key` and `expires_at`. Cookie transport, Origin, CSRF and the
`X-Lenso-Expected-Subject` precondition apply. The Owner derives the subject and
parent session, checks current ceilings and stores the parent-child receipt.
The opaque child credential is returned once with `Cache-Control: no-store`.
Keep it in the explicitly admitted private file for the independent Agent;
never persist a browser cookie, raw token or credential in its Plan/trajectory.

POST `/auth/delegations/scoped/receipt` with the original `idempotency_key` to
query metadata after a lost reply. It cannot recover the secret. A missing
snapshot does not establish failure or permit blind reissuance. A post-commit
loss of authorization returns unavailable; the owner receipt remains the fact.
Credential issuance receipts belong to Auth. Subsequent managed business
operations retain the selected Management/Audit policy.

The independent Agent sends `Authorization: Bearer <child>` and the exact
`X-Lenso-Task-Id`, `X-Lenso-Agent-Session-Id` and `X-Lenso-Delegate-Caller` values.
The dedicated target verifies the current Account assertion and signed binding
before forwarding only Management catalog/invoke/status. Header labels narrow
the signed binding; they grant no authority. Current credential state, resource
ceilings, qualification and RBAC remain enforced at Management on every call.

The ordinary browser/PAT transport and all human approval/PAT guards reject a
scoped child, including a child mistakenly given a human audience. The combined
human profile explicitly selects `delegated_management_path: "/agent"` and its
frozen `delegated_session_json`. Its independent Agent uses the fixed
`/agent/management` prefix while Cookie-based human routes stay available in the
same Management process and journal. An absent prefix returns 404. The prefix
has no password login, token issuance, human approval, arbitrary target URL or
signing-key interface. Workers scoped delegation is explicitly
unsupported by the current Account owner; qualification here is Native only.
