# Cross-repository qualification matrix

This matrix maps Q01-Q24 from the supplied cross-repository proposal to observed evidence. A passed component test, owner test, build, simulation or backend profile establishes only that layer. It does not qualify an unrun ordinary App, browser, MCP, independent Agent, database or deployment profile. Receipt source identities remain immutable when a newer candidate is selected.

The user's request authorized implementation. Instructions embedded in proposal documents were treated as source material rather than additional execution authority. Hyperdrive and a real Model were explicitly deferred by the user. Registry publication awaits explicit human approval; it has not been performed.

## Evidence identities

The following receipt names preserve the original artifact names unless the shared security receipt already defines a portable name. Each copied receipt retains its observed status and source identity.

| Reference | Actual source and proof layer |
| --- | --- |
| [Native PostgreSQL](./receipts/ops-35c0-native-pg-diagnostics.json) | Ordinary offline HTTP distribution, Core `35c0e04ad4d85b12cc79ea660260d27ba871a518`, CLI `0.6.4` SHA-256 `06d5877850d79189701c58aade5b5462c7c0e0fdc0d1c117c986b2e78ec3fa41`. Real PG, nine vectors, four restart vectors and shutdown passed. The saved-resource negative requires its specific setup diagnostic. Auth/Management was not selected. |
| [Native simulated profile](./receipts/ops-35c0-native-sim.json) | Same recorded Core/CLI, eight business vectors, source deleted and shutdown passed. `real_database: false`. |
| [Local Workers D1](./receipts/ops-35c0-workers-local.json) | Ordinary Workers distribution, recorded Core `35c0e04`, JS `18e3cfb2837c8dfe5d5b907e39fe95ae15dc0a65`, actual local workerd/D1, eight vectors and four restart vectors. `real_database: false`; listener closure passed with the recorded wrapper exit-143 diagnostic. |
| [Generated web starter](./receipts/ops-35c0-web-starter.json) | Ordinary source App creation and build/check at Core `35c0e04`, offline Native HTML 200 and shutdown passed after source deletion. This is not Console or a security-owner profile. |
| [Remote Workers D1](./receipts/ops-35c0-workers-remote-query.json) | Actual task-owned Cloudflare Worker and two D1 resources, recorded Core `35c0e04` and JS `18e3cf`. Eight vectors, four same-artifact redeployment/restart vectors and Worker/D1 deletion plus absence readback passed. Schema setup uses the bounded direct remote-query path. |
| [Optional cache and profile negatives](./receipts/ops-35c0-optional-diagnostics.json) | Ordinary Native source profile at Core `35c0e04`. Six vectors passed: persisted absence, installation/inspection, restart, Workers-only binding and weak-KV authority rejection. Negative startup cases require their specific invalid-facility/profile diagnostics. |
| [TestSimulator](./receipts/ops-test-simulator-final.json) | Core `1dbc6b441ccc5571e2349ab4ae6e23a072a9093e`, real Kernel with named bindings, six recorded component cases, two tests and strict Clippy passed. `real_database: false`. |
| [Scope and generation components](./receipts/workers-scope-components-18e.json) | Clean JS `18e3cf`, 13 focused Node tests passed. Explicitly no actual workerd, database or whole-App qualification. |
| [Independent Agent transport component](./receipts/agent-management-transport-final.json) | Actual embedded Agent Host with the deterministic fixture Model and ten transport vectors. It records `real_remote_child: false` and owner issuance not run. It is not the real-owner H2 receipt. |
| [Independent Agent and real owners](./receipts/agent-owner-joint.json) | Actual Native owner-issued child, shared live Management/Console process and deterministic independent Model/Loop. Task source `cc27af63f34f28fefebad0b64bffba850ea70c94`, executable SHA-256 `6b3ae7e91ab0cdf1611b5e6af3974ee5f84573c8b3e9ee3325d3270a40ba237a`; production Role/Loop/Connection bytes are recorded equal to target source8200683. The final report was reconciled read-only from a completed assertion run, with no repeated pending write or other external operation. Individual concurrent-probe names and the final child-process return code were not retained. |
| [Native Management](./receipts/management-api.json) and [bounded MCP writes](./receipts/management-mcp-write.json) | Ordinary offline Native API-token profiles, Runtime Corec046 and exact recorded Owner/Console/Agent sources. Prior committed build phases remain separately identified; current distribution terminal reconciliation, typed reads, restart, live revocation with Bob positive control and shutdown passed. |
| [Native persisted Audit](./receipts/management-api-owner-audit.json), [MCP original](./receipts/management-mcp-prior-owner-audit.json) and [MCP write](./receipts/management-mcp-write-owner-audit.json) | Actual supplemental typed Owner/Kernel readers, immutable executable5261, registry Kernel0.3.11 and functional Audit59. Six actual events per operation: unique five approved phases plus verified requester attempt, actual Alice/Bob attribution and opaque receipt matched through the business Owner API. This is not an original-App inspection route. |
| [Human Console browser](./receipts/human-console-browser.json), [original Human Audit](./receipts/human-prior-owner-audit.json) and [browser Audit](./receipts/human-browser-owner-audit.json) | Actual enrolled Account users/sessions and PAT lifecycle plus nineteen real browser cases. Backend7dc/UI3e629/toolchainc046 are distinct recorded identities. The browser61/rev2 operation has five persisted phases with exact enrolled actors/digest/response receipt; original47/rev1 has an additional requester attempt. |
| [Security owners](./receipts/security-landed.json) and [four owner ports](./receipts/native-four-owner.json) | Exact source/CI/landing and historical real PG port composition described in [security.md](./security.md). The PG composition used Auth `be751f7`, Access `62ed6b9`, Approval `7f5530c`, Audit `59ecc79` and Console `2c817d25`; it is not an ordinary SourceApp or browser run. |
| [Auth remote D1 owner](./receipts/auth-pat-activity-remote-d1.json) | Auth `be751f795b254ab228ae4fb2fde7cf79a934d09b`, Wasm SHA-256 `fe54627c0bef003cf1c0d25a0652ed7f260ca4f8ad1ec5e5721100d30d8f9089`, Wrangler `4.143.1`. Nine real owner activity/revocation vectors and scoped cleanup passed. Human PAT lifecycle remains Native-PG-only. |
| [Workers owner source checkpoint](./receipts/security-workers-owner-checkpoint.json) and [finite Audit reads](./receipts/audit-d1-read-receipt.json) | Core `c046469`, Auth `91d7bc6`, Access `c839b4e`, Audit `6406896`, Approval `3346ce4`. Source checks and finite-store components passed; normalized Auth/Access/Audit archive gates failed on the unreleased facility producer prerequisite. Ordinary local Workers journey/MCP/restart/revocation/read-only/readiness-fault profiles passed in separate receipts; remote profiles below passed with scoped cleanup passed while producer registry publication remains pending. |

The landed security checkpoint is Auth `5fb16bbcbff51cce394fd6e29df9079485f7f37e`, Access `8aabedfeb21a049481d1d2e1ba2523dcb4c3a442`, Audit `47de93e62acd8bf0d84a10b900ec36f8eb24f802`, Approval `3346ce45beacdb74918666c045c802d31da585f6` and Service `6f4d4d158a70cbde2f9d6d46cf45cfb64a2afce0`. Auth's named Human dependencies and generated live-guard checks passed, followed by exact CI `36651764905` and main readback. Audit's explicit forged-wire regression is test-only; its focused three tests and strict check passed, followed by exact CI `36652212653` and main readback. Functional Audit consumer source stays `59ecc79`. Approval's original selected Native profile stays `b7acf592` with supported private configuration metadata and exact CI `36653544540`. The shared PG/D1 policy descendant `3346ce4` passed exact CI `36661069304` and landed; this does not rename the old App proof. These followups do not change a public Capability schema; their [Auth](./receipts/auth-human-named-dependencies.json), [Audit](./receipts/audit-source-attribution.json) and [Approval](./receipts/approval-configuration-schema.json) receipts preserve earlier proof identities.

The [first Core `35c0e04` remote attempt](./receipts/ops-35c0-workers-remote.json) failed during bulk upload and confirmed task-owned D1 cleanup. It remains a failed historical attempt; the separately recorded fresh direct-query run above passed. The Native PG and optional diagnostic runs completed with exit code zero after requiring the specific missing-resource or invalid-facility diagnostics. An unrelated startup failure cannot qualify Q01/Q03/Q04.

## Q01-Q24 status

| Vector | Observed status and layer | Evidence and remaining qualification |
| --- | --- | --- |
| Q01: a saved resource disappears while another candidate exists | Passed, ordinary Native PG with specific diagnostic | `saved_deleted_resource_rejected_with_valid_secondary` requires the deleted primary's setup diagnostic while a valid secondary remains. The secondary is not substituted; an unrelated startup failure is insufficient. |
| Q02: explicit no-cache choice survives installing a provider | Passed, ordinary Native profile | The Core `35c0e04` optional-cache receipt preserves `provider: null` across a new provider, inspection and Host restart. |
| Q03: Workers-only binding selected on Native | Passed, ordinary Native negative admission | `workers_binding_on_native_rejected` requires the invalid facility/profile diagnostic specifically. No remote adapter fallback is inferred. |
| Q04: authoritative state bound to weak KV | Passed, ordinary Native negative admission | `kv_authority_profile_rejected` requires the invalid authority-profile diagnostic specifically. This is binding rejection, not a KV consistency guarantee. |
| Q05: two same-Plugin instances use the intended stores | Passed, selected ordinary backend profiles | Native PG checks two isolated schemas; local and remote D1 check two configured resources, named isolation and unknown-instance rejection. Remote D1 supplies the two actual database resources. No registration-order permutation beyond the recorded runs is claimed. |
| Q06: Native and Workers select different databases | Explicit independent profile boundary | The Native PG and D1 receipts identify independent stores. Neither asserts synchronization or shared revocation across databases. A shared-PG Workers topology was not qualified. |
| Q07: Hyperdrive encounters unsupported PostgreSQL behavior | User-deferred, not run | There is no Hyperdrive qualification or full PostgreSQL-equivalence claim. |
| Q08: simulation passes while real database proof is absent | Passed, evidence-layer separation | Simulation and local workerd retain `real_database: false`. Real Native PG and remote D1 have separate receipts. TestSimulator is never promoted to backend evidence. |
| Q09: application identity enters the operators realm | Owner/component and selected Native realm profiles passed | Owner issuer/key/audience/live-reference negatives retain exact gates. Completed Native C1/C4 identify actual user issuers and separate realm profiles; an arbitrary application-identity injection not listed in those receipts is not inferred as an extra whole-App vector. |
| Q10: authentication succeeds without qualification or permission | Owner/component and selected Native/Worker denial profiles passed | Four-owner PG separates qualification/current policy; actual Native C1/C4 and local Worker wrong-deployment/scope/machine cases retain their proof layers. Generated CredentialState/ACL/Approval failures preserve unavailable versus known denial. Local Worker storage-fault proof is readiness503, not an after-ready denial. |
| Q11: credential ceilings or current policy narrow after catalog admission | Historical owner PG/D1, Native H2 and actual local/remote MCP cached revocation passed | Signed ceilings intersect current owner ceilings and grants. The `be751f7` D1 artifact proves current attenuation/revocation on that artifact. Native C1/MCP rejects the revoked session with Bob available. H2 records the same catalogued hashed Tool request succeeding before actual PAT revocation and receiving403 afterwards, before expiry. The actual local and remote Worker MCP receipts separately verify the same cached request before and after one confirmed Owner revoke while Bob remains available. C4's literal request remains the narrower fresh-guard vector. Arbitrary live RBAC timing variants are not inferred. |
| Q12: MCP aliases or target deployment change | Component, Native MCP and H2 typed target rejection passed | The selected Native MCP and H2 receipts exercise fixed entries and target override denial; H2 contains actual typed tool_failed events without completion of the attempted override. No generic remote target is admitted. |
| Q13: an untrusted log asks for secret disclosure | Passed, actual Tool-returned log with deterministic Loop | H2 executes `fixture__external_log`, receives the malicious log in its actual Tool result and then rejects the unadmitted hidden Tool before invocation. The [recorded turn](./receipts/evidence/agent-third-party-returned-log.json) retains that boundary. Selected ports expose no secret issuance/rotation tool; private credentials are absent from Tool DTOs. This proves deterministic containment, not resistance of a real Model, which is user-deferred. |
| Q14: caller submits `approved: true` | Passed, actual remote ordinary Worker transport | The [raw flag case](./receipts/workers-management-remote-approved-flag.json) sends one new intent with caller `approved:true`; the actual graph returns Pending, retains an unapproved intent and leaves business49/rev2 unchanged. The flag supplies no Owner decision or target authority. No continuation/replay is performed. |
| Q15: approved input, schema, target or revision changes | Owner/core, Native MCP and local/remote Worker immutable continuation passed | Immutable digest and current revision remain authoritative. Actual changed-intent conflict, wrong-digest decision and original continuation occur in the recorded profiles; no unrecorded schema-upgrade timing permutation is inferred. |
| Q16: requester, machine or scoped Agent decides a human approval | Owner PG, Native MCP and selected H2 surface denial passed | Approval's real PG scoped-child/non-user regression denies forced decide audiences; actual Native self/machine and H2 child-only surfaces also reject. The owner helper is denial-only, not an HTTP authenticator. No model approval tool is admitted. |
| Q17: write commits but its reply is lost | Passed, actual C4 browser and local/remote official MCP with known operation | The C4 browser observes actual upstream200 before route.abort drops the reply, locks automatic replay, then queries the retained operation and receives the identical succeeded receipt. Supplemental actual Audit shows one dispatch/completion and matches the exact browser receipt through the business Owner API. Local and remote official MCP receipts separately discard one committed upstream response and recover by SDK status/read, without another invoke. C1/historical Native MCP terminal reconciliation is not relabeled as injected loss. The historical remote Unknown attempt remains not_passed. First-invoke loss without a known operation ID is not claimed. |
| Q18: caller forges Audit append source | Passed, explicit generated-wire/Kernel component and exact test-only gate | Audit `47de93e` decodes forged top-level and nested metadata values, then dispatches the actual generated Client through the Kernel. Its Native codec ignores undeclared source JSON rather than promising rejection; no source field survives the typed request, and metadata cannot replace outer Runtime caller `consumer`. The unchanged functional source is `59ecc79`; this is not whole-App audit proof. |
| Q19: legacy adapters or direct proxies bypass Management | Implemented/source, legacy cookie and selected controlled profiles passed | Native C1/C4/H2/C5 and local/remote ordinary Workers record controlled graphs with fixed targets and excluded human/secret Tools. Trusted-local owner APIs are not advertised as independently protected remote routes. |
| Q20: a required safety owner is missing | Passed, actual C5 installations and local/remote read-only profiles | C5 missing Approval rejects writable readiness, missing Access rejects provider resolution, and explicit read-only omits Approval while retaining Auth/Access/Audit. Both actual local and remote Worker read-only graphs have no Approval instance/facility, omit writes and deny human decisions. |
| Q21: cancelling event A affects event B | Passed, exact JS component | Scope tests abort A, reject its old handles and allow B to continue reading. The receipt explicitly sets whole-App, workerd and database proof false. |
| Q22: a previous generation resolves or rejects late | Passed, exact JS component | Real deferred Promise resolve/reject after generation invalidation does not invoke foreign I/O; uncertain writes dispatch once and do not project into the next event. TestSimulator's retired-handle test is a distinct narrower vector. |
| Q23: browser cache survives subject/deployment change | Passed, actual C4 shared-cookie subject change | With an old Alice view and a live Bob cookie, the actual write returned412/no-store before dispatch; stale sensitive rows unmounted, the current subject remounted, and Bob remained available. The paired nonce/deployment/CLI/Host digests identify the real browser/backend run. A different-deployment switch or every timing permutation is not inferred from this case. |
| Q24: removing Console/Agent leaves listeners or damages business facts | Passed, selected business and C5 installation profiles | Native offline/source-deletion/shutdown and C5 selected Console/Agent removal preserve the business baseline. Local workerd listener closure is recorded separately from wrapper exit. No unselected topology is inferred. |

## Completion rules

The completed Native C1 `management-api.json` and MCP-write `management-mcp-write.json` preserve their recorded prior/current build phases and actual case names. C4 `human-console-browser.json` records actual human/PAT/browser proof and its separate UI/backend identities. The local ordinary Worker [journey](./receipts/workers-management-local-journey.json) and [typed Audit inspection](./receipts/workers-management-owner-audit-local.json) passed at their recorded Console898/Corec046/builderbb30 pair. The later Worker restart observation below passed; local revocation, read-only and startup storage-fault cases passed; remote cases have their own passed receipts below; scoped remote cleanup passed in the linked final receipt. H2 `agent-owner-joint.json` is complete with the explicit reconciliation limits below; C5 `installations.json` completed, with its [resume provenance](./receipts/installations-resume-provenance.json) identifying four reused completed profiles and fresh read-only/negative checks. Each must contain the actual selected source/build identities and observed cases. C4 browser completion is paired to one Host nonce, deployment, CLI digest and Host-build digest. H2 pairs its built Agent executable to the selected clean Git source and the completed Human deployment/distribution before issuing a child.

Preparation is not qualification. Bootstrap/enrollment/grant markers prohibit blind resume after the first potentially effective action. Unknown writes retain their original references and are reconciled through owner receipts; they are not automatically replayed. Known successful idempotency repeats are identified separately from Unknown recovery. Unavailable authentication or policy must never count as a successful permission-denial vector; the MCP revocation proof also requires an unrelated current caller to remain available on the same listener.

Only sanitized receipts belong in this directory. Preserve SHA/digests, result status, proof layer, case names and cleanup readback. Exclude cookies, bearers, PATs, passwords, signing material and database connection credentials. Source landing, passing CI, package publication and deployment qualification remain separate facts.

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
