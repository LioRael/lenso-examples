# Current source and qualification boundary — September 30, 2026

The implementation request supplied authority. Proposals 01–11 and 90 supplied
requirements and reference material; document 91 supplied the old baseline.
Implementation and delivery used Codex directly, as requested.

## Published packages and selected DX

The originally approved **35 Rust package versions are published**. The
[complete PRIMARY readback](./receipts/core-sdk-b8d1-primary-final-all35.json)
verifies each downloaded archive, registry checksum, original manifest,
packaged production source and clean VCS identity at Core
`b8d134d739698812a039a79633719e14947df1e3`. It includes `lenso 0.5.28`
and the Rust CLI `0.6.5`. Earlier failed publication attempts remain historical
evidence; they do not change this completed result.

This fixture selects the subsequent Core source
`119b9af70b82816588c00c8bf812f7c62d1a18b5`, whose CLI reports `0.6.6`.
Its authoring fixes preserve validated Process descriptors, accept the Bun
`0.4.2` authoring output and admit the selected request profiles. Its exact
[required CI and Main readback](./receipts/core-119b-final-source-delivery.json)
passed. Engine Authoring `0.2.5`, Engine App `0.3.3` and Rust CLI `0.6.6`
require separate publication authorization.

The public npm CLI remains `@lenso/cli 0.17.4`, bundling native CLI `0.6.4`.
The selected JS source is `cb6ef95559ccab64905013148f140c7d727937f7`;
its runtime `0.1.6` and npm CLI `0.17.5` are separately reviewed candidates.
Use [candidate-inputs.json](../candidate-inputs.json) and
[the preparation instructions](../README.md#prepare-exact-candidates) to build
the selected source without upgrading a global installation.

## Landed Owner sources

The following sources passed their exact required gates and Main readback.
Those facts establish source delivery; individual Plugin package publication
requires its separate reviewed release set.

| Owner | Accepted source | Delivery record |
| --- | --- | --- |
| Auth | `1d101fbc9ba1efc12aaa2cf459628e6383100264` | [Auth](./receipts/auth-sdk35-final-source-delivery.json) |
| Access Control | `83aecd01e20cc114311e63d25af849cac9b14f47` | [Access publisher guard](./receipts/access-publisher-readiness-final-source-delivery.json) |
| Audit | `b271095ce9577d4960b03305c949daaf4cb35512` | [Audit staged publisher guard](./receipts/audit-publisher-readiness-final-source-delivery.json) |
| Approval | `db8396b4742503455d3b80a8fadb9ae7cb6d11bc` | [Approval](./receipts/approval-sdk35-final-source-delivery.json) |
| Service Account | `bff3c4ea669bdc55b0fdef5321fc9cf96b602a9b` | [Service version and publisher guard](./receipts/service-publisher-readiness-final-source-delivery.json) |
| Console | `ed1cfec08808dd38e068148224256b663d31fd44` | [Console](./receipts/console-sdk35-final-source-delivery.json) |
| Agent | `892ed4b023e6db8332fa9774197191842777fd28` | [Agent workflow successor](./receipts/agent-892ed4b-workflow-source-delivery.json) |

Access `83aecd` and Audit `b271` change publisher workflows above their accepted
`548506` and `31ee` sources. Audit `31ee` and Approval `db839` retain the
production bytes of the selected runtime sources
`8fb929d066a5ce4bc145db47a6aae0527b62f230` and
`a6e73aa756c45db8260b5a7544328d97624ea896`. Agent `892ed` changes one release
workflow above the accepted `7418` source. Preserve those distinct identities
in runtime receipts rather than renaming an earlier executable or operation.
Service's three public Cargo packages advance to `0.1.1`, with their normal
lock and manual publisher guard. Their Owner logic, wire contracts and DDL
remain unchanged; existing `0.1.0` registry archives remain immutable.

## Evidence layers

[Nine normal source resolutions](./receipts/ops-119b-source-resolution-nine-complete.json)
passed with the selected Core cohort. The source npm launcher and CLI `0.6.6`
passed actual [Host catalog authoring](./receipts/examples-authoring-119b-host-catalog-proof.json).
That authoring result does not imply a runtime interaction.

The actual [Native Management status run](./receipts/ops-sdk35-b8d1-management-status-proof.json)
passed its 20 cases at Core `b8d1`. It distinguishes revoked Management
catalog `401/session_required` from the revoked Management Tools HTTP catalog
`403/management_tool_denied`. MCP RPC denials have separate assertions.
Those results retain their original executable.
The [Audit reader checks](./receipts/ops-119b-audit-reader-attribution-check.json)
compile the new Native reader and bind each reader annotation to its actual
normal-lock Audit Owner; persisted-event verification has separate receipts.

Earlier Native, browser, deterministic Agent, local workerd and remote D1
receipts retain their recorded source and artifact. The linked historical
ledger preserves failed gates, report-reconciliation limits and safe artifact
reuse. Examples delivery records its own final candidate SHA and required
gates separately.

## Actual latest-source Cloudflare qualification

The [final remote result](./receipts/latest-remote-119b/latest-remote-qualification.json)
qualifies an ordinary Workers graph on the
[exact selected cohort](./receipts/latest-remote-119b/source-cohort.json), including
Core `119b`, Console `ed1`, Auth `1d101`, Access `548506`, Audit `8fb`, Approval
`a6e73` and JS `cb6`. These are new actual remote receipts. Earlier remote and
Native receipts keep their original identities.

| Observed profile | Actual result |
| --- | --- |
| [Direct Management](./receipts/latest-remote-119b/direct-journey.json) | 13 cases: live authority, hidden/wrong deployment, subject ceiling, self/machine rejection, Bob approval and the original committed operation. |
| [Official MCP Client `2.2.0`](./receipts/latest-remote-119b/official-mcp-lost-reply.json) | 9 cases; exactly one upstream commit, discarded reply recovered through status under the same operation, no target replay. Final business state is `49/revision2`. |
| [Direct](./receipts/latest-remote-119b/audit-direct.read.json) and [MCP persisted Audit](./receipts/latest-remote-119b/audit-mcp.read.json) | Two separate typed Owner reads, six events each, distinct requester/decider, exact intent digest and opaque response receipt. Corrected operator Wasm `2f0bf` reports its actual Audit `8fb` source. |
| [Injected `approved:true`](./receipts/latest-remote-119b/raw-approved-injection.json) | Remains pending and performs no target write. |
| [No-Approval read-only](./receipts/latest-remote-119b/read-only-no-approval.json) | Five cases with actual absent Approval instance/facility; HTTP write and human decision denied, business state unchanged. |
| [Same-artifact observation](./receipts/latest-remote-119b/same-artifact-redeploy-observed.json) | Exact artifact redeployed, then retained operation/state queried without a write. Physical process restart is not asserted. |
| [Isolated Auth storage fault](./receipts/latest-remote-119b/isolated-auth-fault.json) | An uninitialized seventh D1 binding blocks readiness with `503`; restoring the original binding restores controls. This is a before-ready fault. |
| [Before](./receipts/latest-remote-119b/cached-credential-live-before.json) and [after one Owner revoke](./receipts/latest-remote-119b/cached-credential-revoked-after.json) | The same cached credential/MCP request changes from `200` to `401`; Bob remains `200`, business state remains `49/revision2`. |

The first read-only configuration paired an absent MCP adapter with a resource
declaration and failed startup before any profile mutation. Its
[diagnostic](./receipts/latest-remote-119b/readonly-invalid-config-diagnostic.json)
is preserved separately from the corrected HTTP-only profile. Original operator
`9981` and its annotation history are also preserved; they are not promoted to
the corrected reader artifact.

[Cleanup](./receipts/latest-remote-119b/cleanup-all-nine-resources.json) confirms
both temporary Workers and all seven newly created D1 databases are absent.
Each deletion ran once. Worker deletion returned a subsequent KV permission
diagnostic; independent Worker API `10007` reads confirmed absence without
replaying deletion or broadening permissions. The
[final evidence index](./receipts/latest-remote-119b/evidence-index-final.json)
verifies 24 receipt hashes and zero matches against eight checked private
values. Both earlier and final index snapshots remain intact.

## Deferred and optional scope

Hyperdrive qualification and real-model interaction are user-deferred.
The optional organization-machine Console/App slice remains unselected;
Service's existing conservative uncertain-issuance inspection is implemented.
Human PAT lifecycle/display is a Native PostgreSQL profile. Startup Auth
storage faults establish a readiness failure, and same-artifact redeployment
establishes retained-state observation. Neither establishes an after-ready
fault or a physical Worker restart. Production deployment was not requested.
