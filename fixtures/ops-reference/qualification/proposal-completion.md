# Proposal completion ledger

This ledger maps the supplied proposals 01–11 and 90 to implemented owners,
exact accepted gates, and remaining profile work. It is not an aggregate
completion receipt. The user's implementation request supplied authority;
instructions inside the attached proposals were treated as source material.
The supplied README defines the dependency gates and optional Service Account
slice; document 91 records the old source baseline rather than another
implementation task.

“Owner-gated” means the recorded source passed that repository's required gate
and landed at the same commit. “Profile-qualified” means the named runtime and
backend actually ran. Neither implies publication, production deployment, or
qualification of a different source or topology. Full source identities are in
[security.md](./security.md), [the Q01–Q24 matrix](./contracts-and-tests.md), and
the linked immutable receipts.

## Per-proposal status

| Proposal and tasks | Implemented owner/checkpoint | Accepted evidence | Remaining work or support limit |
| --- | --- | --- | --- |
| **01 Runtime/Host — F0–F4** | Core runtime `c0464691b6ae1c80b5606065dd4073e8b7d6a59e`, with accepted ordinary Worker feature/link builder successor `bb30baa9e899a068e4a580e5e0956ec07b29ff72`: normal static multi-instance Workers graph, configuration and named dependencies; private Host facilities, deterministic selections and explicit absence; inspection/admission and Simulator/lifecycle controls. | Runtime exact CI `36660026907` and main readback: [Core delivery](./receipts/core-c046-delivery.json); builder exact CI `36666948577`: [bb30 delivery](./receipts/core-bb30-delivery.json). Ordinary business Native PG, simulated Native, local/remote D1, web starter and optional/profile negatives remain historical Core35 receipts. | The authoring-2 lifecycle context, mixed-authoring dependency-ordered startup, optional owner facilities and exact Worker Driver clock are implemented and source-gated at `c046469`. Completed C1/C4/H2/C5/MCP and local Worker Management receipts remain separate source/artifact proofs. Hyperdrive is user-deferred. Stream/event/dynamic-loading capabilities outside the selected static request profile remain explicit limits. |
| **02 JS/Workers/SDK — J0–J5** | JS current landed `cb6ef95559ccab64905013148f140c7d727937f7`, with workers-runtime bytes unchanged from `18e3cfb2837c8dfe5d5b907e39fe95ae15dc0a65` (`0.1.6` historical source proof): static graph/configuration lowering, instance-scoped private facilities, request leases, cancellation and generation fencing. Contract projections come from owner descriptors. | Exact managed-release gate `36672371433` and main readback are in [current JS delivery](./receipts/lenso-js-managed-release-delivery.json). [Ordinary local D1](./receipts/ops-35c0-workers-local.json) and [remote D1](./receipts/ops-35c0-workers-remote-query.json) retain selected18e artifacts; [13 Node scope tests](./receipts/workers-scope-components-18e.json) establish the separate Q21/Q22 component layer. | The Node cancellation tests do not qualify live database cancellation. Shared-PG and PG/D1 Hyperdrive mixtures are not qualified. J5 does not promise Bun execution inside Workers or universal TypeScript source portability. |
| **03 Auth — A0–A5** | Auth `5fb16bbcbff51cce394fd6e29df9079485f7f37e`: strict realm/issuer/key/audience and credential references, current Account/API CredentialState, signed/current deployment/resource ceilings, parent-bound scoped child issuance, human PAT issuance/list/revoke/receipt and activity metadata. A2 has an independent display-only projection with optional ProfileCache; authority always reads the owner. | Exact CI `36651764905`, archive checks and main readback: [owner delivery](./receipts/security-landed.json). [Named Human dependencies](./receipts/auth-human-named-dependencies.json) preserve the earlier real lifecycle factory proof and final generated-wire unavailable/denied checks. [Remote D1 activity](./receipts/auth-pat-activity-remote-d1.json) is the historical `be751f7` artifact, not a new `5fb16bb` run. | Native Human PAT and the new Account display read return `UnsupportedProfile` on Workers. Existing D1 authentication/CredentialState/activity is not a Workers Human/Management closure. Requested unsupported MFA/step-up/method/idle assurance rejects. Credential Issuer has no new idempotent issuance/status API; the conservative Service inspection path remains. Native C1/C4 and the reconciled H2 assertion run are complete at their recorded source/build pairs. Separate Worker Auth `91d7bc6` adds ordinary Source-selected private D1 state; source checks passed but its required normalized archive fails on the unpublished facility producer prerequisite. |
| **04 Access Control — Z0–Z4** | Access `8aabedfeb21a049481d1d2e1ba2523dcb4c3a442`: scoped allow-only role union, default deny, current revision, exact bootstrap/administrative callers, and supported private configuration schemas. Management owns deployment qualification and ceiling intersection rather than changing RBAC union semantics. | Exact quality/Workers gate `36644691127` and main readback: [owner delivery](./receipts/security-landed.json). The historical four-owner PG composition separately checks eligibility/current grant/ceiling behavior. | Separate Worker Access `c839b4e` supplies an ordinary Source-selected private D1 facility with a fresh primary session per finite call; source checks passed, while normalized archives fail on the facility producer prerequisite. A D1 build or owner workflow is not a remote deployed Access/Management profile. Legacy adapters are trusted-local APIs and are excluded from the controlled graph. Actual Native C1/C4 admission passed at their recorded source pairs; H2 selected-profile admission has its reconciled actual receipt; C5 completed the six selected profiles and required-owner removal checks; its raw receipt and resume sidecar preserve reused versus fresh artifacts. No ABAC, wildcard scope hierarchy or role inheritance is claimed. |
| **05 Audit — D0–D3** | Functional Audit `59ecc79e38de72891e6a164d7e74617c618038f8`: caller-derived source, caller-scoped stable append keys, append-only facts, bounded metadata, read scopes and pagination. Test-only main `47de93e62acd8bf0d84a10b900ec36f8eb24f802` adds the explicit forged-source vector without changing production/schema. Management owns its durable outbox; target receipts remain target-owned. | Exact test-only CI `36652212653` and main readback: [source attribution](./receipts/audit-source-attribution.json). [Historical four-owner PG](./receipts/native-four-owner.json) records outbox/actor attribution at its recorded sources. | The codec ignores undeclared top-level source JSON; the typed request cannot replace the actual Runtime source. This is not a promised JSON rejection. Actual C1 and both MCP operations passed supplemental persisted-event inspection through the generated Owner typed port and business Owner receipt API; C4 original and browser operations also passed the supplemental inspection, with actual enrolled subjects; browser response receipt/digest were compared exactly. The supplemental Host is distinct from the original ordinary App graph. Separate Worker Audit `6406896` adds shared owner policy and a finite persistent D1 store. Its seven Node components pass, including malformed read receipts; source checks pass, but the required registry archive fails on the facility producer prerequisite. The local ordinary Worker journey and separate typed Audit inspection passed, with sealed producer, distinct actors, exact digest and response receipt verified. The local restart, cached credential revocation and startup storage-fault controls passed; remote ordinary/MCP/read-only/revocation/fault profiles below passed; scoped remote cleanup passed in the linked final receipt. |
| **06 Approval — P0–P3** | Approval `3346ce45beacdb74918666c045c802d31da585f6`: shared PG/D1 policy and finite D1 CAS/receipt storage, immutable intent digest, requester-scoped idempotency/isolation, expiry, one terminal CAS and exact decider callers; forwarded scoped-child/non-user denial. Private configuration metadata now resolves under the actual Source parser while owner format checks remain strict. | Exact current CI `36661069304` and main readback: [Approval D1 source delivery](./receipts/approval-d1-delivery.json). The [configuration compatibility](./receipts/approval-configuration-schema.json) and earlier real PG child/CAS/expiry proof retain their original checkpoints; current extracted policy passed two fresh PG tests. | Approval does not verify HTTP credentials or execute the target. Its forwarded-assertion helper only denies, while missing assertions retain the admitted trusted-local caller boundary. Native explicit read-only Management can omit Approval; actual C5 admission passed at its recorded source/artifact pair. The local ordinary API-user Worker journey passed immutable pending, self/machine/wrong-intent denial, Bob approval and original continuation. The later local restart observation passed; the remote ordinary/MCP immutable-intent profiles below passed; scoped remote cleanup passed in the linked final receipt. |
| **07 Service Account — S0–S3** | Service `6f4d4d158a70cbde2f9d6d46cf45cfb64a2afce0`: existing organization/membership, Directory identity, Issuer and Access ownership retained; secret-once, overlap/revocation/expiry checks retained; sanitized `inspect_command` reports durable uncertain `issuing` without reset/reissue. | Exact CI `36673403339` and main readback: [SDK35 delivery](./receipts/service-sdk35-delivery.json). The exact candidate gate includes real Native PG restart/concurrency; earlier8d/2f proofs retain their historical sources. | S1 organization-machine Management/UI/CLI topology is **optional and unselected**, not implemented by the current Console profile. Personal PAT does not install it. The historical Service source pairs Core `1dbc6b4`. The SDK35 source roll with `cac6db9` (facade0.5.28, Kernel0.3.12, Native0.3.19/macros0.2.8, authoring0.1.2/codegen0.10.1) passed local source/immutable-Role archive checks and exact remote CI, then landed6f4. Stable external Roles and production logic are unchanged. That roll is not a registry-publication claim. Workers closure and automated Issuer reconciliation are not supported. |
| **08 Console/Management/MCP — C0–C6** | Console `dbea6e41fa77de867a3daa55ad43efe0113de574`: fixed typed catalog, per-call current authority, deployment qualification, immutable intents/CAS, target receipt recovery/outbox; strict legacy exclusion/import; Human approval/PAT pages, no-store/cache isolation; optional read-only no-Approval API; rmcp `3.5` with exact entry alias checks and read-only/default bounded-write policy. | Exact quality CI `36671903503` and main readback: [current Console delivery](./receipts/console-dbea-delivery.json). Historical Native7dc evidence remains separate. Existing owner-port, protocol, generated-error and UI component checks remain their own layers. | Native C1 and bounded MCP-write ordinary Source runs passed with their prior/current build phases explicit, typed reads, restart and live revocation; C4 passed actual Native human/browser qualification with Backend7dc/UI3e629/toolchainc046; C5 completed the six selected profiles and required-owner removal checks; its raw receipt and resume sidecar preserve reused versus fresh artifacts. The six source wrappers use the completed lifecycle seam described in 01; actual source startup/protocol proofs remain distinct. Native SQLite journals/qualification and native HTTP bridges retain their recorded proof. Portable Console dbea retains the same finite D1 journal/qualification and JSON-compatible serialization and adds explicit preissued-bearer MCP resource metadata. Earlier898 ordinary journey and typed Audit passed; dbea official Client MCP commit-loss/status/restart and exact cached revocation, read-only and startup storage-fault profiles also passed. Remote ordinary/MCP/read-only/revocation/fault profiles below passed; scoped remote cleanup passed in the linked final receipt. No authenticated remote-control topology is qualified. Unknown issuance retains its reference and cannot be declared a pre-write failure or replayed automatically. |
| **09 Agent — H0–H5** | Agent `cc27af63f34f28fefebad0b64bffba850ea70c94`, with exact paired Role sources in its build receipt: neutral catalog/invoke/status Tool Provider, typed Host task/session/caller binding, fixed remote origin/prefix, private child credential file, bounded results, explicit pending/unknown handling and a minimal removable management profile. | Exact quality `36671876017` and SDK verification `36671875980`, same-SHA landing/readback: [current Agent delivery](./receipts/agent-cc27-delivery.json). Earliercdcb gates remain historical. [Earlier ten transport vectors](./receipts/agent-management-transport-final.json) used a fixture credential/Model rather than a live owner child. | The actual H2 owner-issued child, live parent revocation, shared Console and Tool-returned malicious-log run is complete, with explicit report-reconciliation limits. C5 completed the six selected profiles and required-owner removal checks; its raw receipt and resume sidecar preserve reused versus fresh artifacts. A real Model is user-deferred; that does not turn a fixture into prompt-resistance evidence. No model approve/bootstrap/secret-issuance tool is admitted. |
| **10 Examples — X0–X7** | `ops-reference` source/worktree contains the shared business contract, private owner stores, ordinary Native/Workers source profiles, explicit operators/human/MCP/Agent/installations helpers and sanitized receipt packaging. These current Examples edits are not yet a landed final aggregate. | Core35 business profiles have their named [Q01–Q08 evidence](./contracts-and-tests.md); the generated web starter proves ordinary CLI/offline Native HTML. Source deletion, persistence and the specific negative diagnostics are recorded independently. | Native C1/MCP receipts and independent persisted Audit supplements passed. C4 actual human/browser and persisted Audit receipts also passed. H2 is complete with the explicit report-reconciliation limits below; C5 completed the selected installations/removal profiles, while the final clean-consumer/Examples gate remains pending. The Management source lifecycle migration is implemented and its recorded Native and Worker selected profiles passed. Shared-PG Workers/mixed-PG-D1 tests are tied to user-deferred Hyperdrive. Workers Management G4-W is required. Independent owner D1 Audit/Approval backends, source-bound Auth/ACL facilities and the finite portable Management journal are implemented source checkpoints; the local ordinary journey and typed Audit supplement have passed, with final remote cleanup passed while normalized archive producer prerequisites remain outstanding; local revocation, read-only and startup storage-fault profiles passed; the later local restart observation passed. The completed business-D1 result does not qualify that closure. |
| **11 Site — W0–W2** | Current clean draft `3640ef26e6d1b5ae3c56a392a3503457fd83df36`: six EN/ZH task recipes, ownership/glossary/support layers and draft exclusion from public navigation/search/llms/output. | Historical content checks and deployment dry-run do not certify the current final draft. The current exact-pair content gate, candidate CI and landing have not been supplied. | Tutorials remain drafts until matching ordinary consumer receipts and a final source/version roll. No published tutorial, registry version or deployment is inferred from a draft. |
| **90 Cross-repository contracts — I1–I7/Q01–Q24** | Boundaries are represented by generated owner contracts, fixed Host choices, verified realm/credential references, current qualification/RBAC/ceiling intersection, immutable approval, owner receipts, source-bound audit and neutral tool ports. | [Q01–Q24](./contracts-and-tests.md) states the actual owner/component/whole-profile layer for each vector. Static receipts are immutable copies with [hashes](./receipts/SHA256SUMS). | Native and remote Workers profiles passed at their recorded cohorts. Latest normalized-consumer/archive gates, optional machine topology and final Site/consumer work remain separate; owner CI does not establish them. |

## Implementation limits versus pending qualification

The completed business D1 profile does not include operators Auth, RBAC,
Approval, Audit, or Management. A Workers management declaration needs a
selected persistent journal/security implementation or a separately
authenticated remote-control topology, and its own actual closure proof.
The new finite D1 owner/journal source implementations supply that boundary.
Their local ordinary API-user journey and separate typed Audit read passed;
the later local restart observation passed; local revocation, read-only and startup storage-fault profiles passed; remote profiles below passed; scoped remote cleanup passed in the linked final receipt. Native
SQLite/HTTP code and native owner tests do not qualify those Worker cases. Auth's D1 authentication and current-state APIs remain useful
owner capabilities; the Native-only Human lifecycle/display additions reject
rather than silently downgrade on Workers.

The optional organization-machine slice is separate from personal PAT. The
Service owner has implemented and tested its conservative uncertain-issuance
inspection, but the current human Console has no organization-machine
Management adapter/page or selected machine-job topology. This is an
unselected optional implementation slice, not an unrun test of the human PAT
page. The existing non-idempotent Issuer boundary cannot be described as safe
automatic reissuance.

The authoring-2 source lifecycle seam and six fixture migrations are
implemented. Completed Native C1/MCP receipts record current Tool/MCP/restart/revocation and preserve their earlier committed build phase. C4 has its actual nineteen browser cases and explicit UI/backend pair; the H2 receipt now records its actual run and reconciliation limits. C5 independently records six selected source profiles and actual cases, with a sidecar identifying safe reuse of four exact completed artifacts; the historical Core35 business proof does
not qualify the later Management graph. The actual supplemental C1/MCP readers inspected the original Audit database through the generated Owner read port and correlated actual business Owner receipts. These are distinct from the original ordinary App graph and from their classifier tests.

Hyperdrive and a real Model are **user-deferred**. Registry publication awaits an explicit human reply and has not been performed.
Production deployment is **not requested**. Those states differ from missing
implementation, an optional unselected profile, a draft, or a pending actual
qualification. Native G4-N can be delivered before Workers G4-W, but a report
must keep their completion states separate.

The subsequent local Worker [restart observation](./receipts/workers-management-local-restart.json) returned the same retained operation and preserved business state without invoking a write. The local [no-Approval read-only profile](./receipts/workers-management-local-read-only.json) also passed: live read, actual missing Approval instance/facility, omitted write entry, human decision denied and unchanged business state. Local current-credential revocation, explicit read-only and startup storage-fault profiles passed at their recorded artifacts. The fault blocks readiness; it is not an after-ready authentication fault. Remote ordinary/MCP/read-only/revocation/fault profiles below passed at their recorded artifacts; scoped remote cleanup passed in the linked final receipt.

## Native H2 receipt reconciliation

The [H2 receipt](./receipts/agent-owner-joint.json) records the actual Owner-issued
Account child, independent deterministic Model/Loop, current-turn read and
status, retained accepted pending reference, Tool-returned malicious log, exact
cached Tool revocation and parent logout denial while Console remained live.
The final report hit a variable-shadow error after its qualification assertions
completed. Root reconciled the report read-only; it dispatched no external
operation and did not repeat the pending write. The receipt explicitly records
that individual concurrent-probe names and the final child-process return code
were not retained. Their evidence is the completed-run assertions, rather than
a newly reconstructed raw exit status.

The [original helper](./receipts/evidence/agent-helper-before-reconciliation.py)
and [failure trace](./receipts/evidence/agent-completed-run-report-failure.log)
retain the exact recorded SHA-256 values. The actual
[returned-log turn](./receipts/evidence/agent-third-party-returned-log.json) and
[same cached request](./receipts/evidence/agent-exact-cached-tool-after-owner-revoke.json)
are separate preserved evidence. Agent task sourcecc27/executable6b3a,
production target8200683, issuer Host and target Host remain separate identities;
the recorded Role/Loop/Connection bytes are equal to that production target.
This is deterministic boundary proof, not real-Model prompt-resistance proof.

## Latest source delivery versus historical profiles

Current accepted source gates are [Console dbea](./receipts/console-dbea-delivery.json),
[Agent cc27](./receipts/agent-cc27-delivery.json), [Core builder bb30](./receipts/core-bb30-delivery.json),
[JS cb6](./receipts/lenso-js-managed-release-delivery.json) and [Service6f4 SDK35](./receipts/service-sdk35-delivery.json).
Completed Native profile receipts retain Backend7dc/Toolingc046 and their
recorded older Owner sources; C4 UI3e629 and H2 task/executable identities are
separate. Local Worker journey898 and MCPdbea retain their own immutable
artifacts, Core runtimec046/builderbb30/JS18e and Owner91/c839/640/334.

C5 [raw installations](./receipts/installations.json) and [resume sidecar](./receipts/installations-resume-provenance.json)
completed six selected Native profiles. The sidecar preserves the four exact
completed artifacts reused without repeated setup, and the fresh read-only and
missing Approval/Access checks. It does not rewrite the raw final receipt.

Local Worker [official MCP](./receipts/workers-management-local-mcp-write.json),
[restart](./receipts/workers-management-local-mcp-restart.json), [persisted Audit](./receipts/workers-management-local-mcp-owner-audit.json),
[before](./receipts/workers-management-local-revocation-before.json) and
[after revocation](./receipts/workers-management-local-revocation-after.json)
passed. The [isolated Auth storage fault](./receipts/workers-management-local-auth-storage-fault.json)
blocks whole-graph readiness503, with healthy Alice/Bob controls; it does not
claim a storage fault after successful readiness. Remote profile receipts and scoped resource cleanup below passed; Examples aggregate delivery is separate.

Latest SDK35 manifests/locks for the four security Owners and downstream
consumers are separate candidates. Seven registered Auth/Access Roles retain
byte-identical contracts and primary-compatible minima, as shown by the
[immutable Role analysis](./receipts/security-primary-role-cohort-analysis.json).
Passing source checks with Git patches do not pass the registry archive gate.
The producer publication approval question is pending a human reply; no
registry publication is assumed or performed.

## SDK35 candidate evidence

The [four-Owner checkpoint](./receipts/security-sdk35-source-checkpoints.json)
records Auth `1d101fbc9ba1efc12aaa2cf459628e6383100264`, Access
`5485066ae3e99191862701f753b3ae186755ee80`, Audit
`8fb929d066a5ce4bc145db47a6aae0527b62f230` and Approval
`a6e73aa756c45db8260b5a7544328d97624ea896`, with coherent SDK library
source `f670422953b42cc33b83a9b8cde9a4c74e9f6ceb`. Their local source
checks passed. Approval exact required CI `36679362476` passed, including
real PostgreSQL and finite D1 checks. Auth `36678998509`, Access
`36677702907` and Audit `36677705331` failed normalized archive resolution:
Auth/Audit require unpublished Kernel `0.3.12`; Access requires unpublished
facade `0.5.28`. Earlier required source checks passed. The failed gates
remain failed; all four coordinated landings are held. Downstream
[Console ed1 source checks](./receipts/console-sdk35-source-candidate.json)
also passed, while exact required CI `36680257279` failed its extracted
package because Kernel `^0.3.12` is unavailable. The
[actual failure log](./receipts/console-sdk35-required-ci-failure.log)
preserves that result; the candidate receipt retains its earlier
`in_progress_at_recording` snapshot. No landing bypass is performed. Producer publication
awaits a human reply, and no registry write has occurred.

The [Core1b34 archive review](./receipts/core-sdk-1b34-artifact-review.json)
records 35 committed-source archives passing extracted cleanroom compilation
and test compilation, with exact digests. It does not establish primary
registry availability or a completed candidate gate. The
[clean source CLI build](./receipts/ops-source-cohort-1b34-cli-build.json)
is a separate successful Native CLI `0.6.5` build. New business-only
[Native simulation](./receipts/ops-latest-f670-native-sim.json),
[web starter](./receipts/ops-latest-f670-web-starter.json),
[local Workers](./receipts/ops-latest-87fe-workers-local.json) and
[TestSimulator output](./receipts/ops-latest-f670-test-simulator.log)
retain their actual f670/87fe ancestor artifacts; they are not renamed as
1b34 or as Auth/Management qualification. Simulation/local workerd record
`real_database: false`. The library producer versions are unchanged by the
builder successor. [Service cleanup](./receipts/service-sdk35-cleanup.json)
removed only its clean integrated task worktree after exact delivery; no
shared cache or active worktree was removed.

## Remote Workers evidence recorded so far

The actual remote ordinary API-user graph and official MCP operations have
separate [persisted Audit reads](./receipts/workers-management-remote-owner-audit-supplement.json).
That index combines two existing exact read receipts and dispatches no new
Owner call. Each generated typed Owner read verifies actual sealed producer,
Alice/Bob actors, immutable digest and the opaque original response receipt.
It is supplemental inspection beside the ordinary graph, not a new audit
route or a fabricated event inside it. The raw read receipts remain separate
[direct](./receipts/workers-management-remote-direct-owner-audit.json) and
[MCP](./receipts/workers-management-remote-mcp-owner-audit.json) files.

The [remote read-only profile](./receipts/workers-management-remote-read-only.json)
passed five cases with no Approval instance or private facility, a live read,
absent write entry, denied human decision and unchanged business49/revision2.
[Same-artifact redeployment observation](./receipts/workers-management-remote-redeploy-observed.json)
recovers the original MCP operation/receipt and business state without a
write; the receipt explicitly does not assert a physical process restart.
The [raw approved flag](./receipts/workers-management-remote-approved-flag.json)
creates an unapproved pending intent without target dispatch. The [ordinary direct journey](./receipts/workers-management-remote-journey.json)
records thirteen actual cases. The [official Client2.2 MCP run](./receipts/workers-management-remote-mcp-write.json)
records a discarded committed reply followed only by status/read recovery.
The [isolated Auth storage fault](./receipts/workers-management-remote-auth-storage-fault.json)
blocks whole-graph readiness503 with healthy restored controls; it does not
claim a per-authentication fault after successful readiness. Fresh
[before](./receipts/workers-management-remote-revocation-before.json) and
[after](./receipts/workers-management-remote-revocation-after.json) controls
verify the same actual cached request, current Alice denial after one confirmed
Owner revoke, and Bob200 with unchanged49/revision2. The
[credential alias record](./receipts/workers-management-remote-credential-aliases.json)
identifies one credential used by both transports; no second revocation is
inferred. The [remote artifact receipt](./receipts/workers-management-remote-artifact-qualification.json)
records the completed profile and its exact dbea/c046/bb30/91/c839/640/334
cohort; SDK35 ed1 whole-graph qualification is explicitly separate.
[Cleanup](./receipts/workers-management-remote-cleanup.json) confirms both
task Workers and seven task D1 resources absent. Each Worker delete was
dispatched once without force/replay; Wrangler exited1 during a later legacy
KV listing error10000, while fresh versions API10007 confirmed absence.
The unapproved probe remained Pending until those resources were deleted.
The [portable alias index](./receipts/workers-management-remote-receipt-index.json)
maps the immutable artifact receipt's original relative/private-inspection
paths to their byte-identical portable names; no secret file is packaged.
Examples aggregate delivery remains separate.

The [local operator shutdown](./receipts/workers-local-operator-shutdown.json)
records normal SIGINT, wrapper exit130 and listener absence after completed
Owner calls. Local D1 facts and markers were preserved; a child PID was not
retained. It is not cleanup of the separate remote resources.


## Final selected source boundary

[Profile inputs](./profile-inputs.json) record Coreb8/Consoleed1/Agentbec
as source candidates and identify the separately qualified historical
Native and remote Worker artifacts. These receipt-only inputs do not
reconstruct a runnable project or private Owner setup. The
[current gate snapshot](./receipts/security-final-source-status.json)
records exact Coreb8 CI success and Agentbec producer-blocked contracts
failure, together with held latest Owner/Console archive gates.
[Final source evidence](./README.md#final-source-checkpoint) preserves the
b8/bec checks, binary/archive identity reuse and prior failures. Publication
awaits explicit human approval; no historical profile is renamed as a
latest SDK35 runtime proof.
