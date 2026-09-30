# Security owner qualification

The five security owners below passed their required candidate CI and landed the same validated commits on `main`. This establishes source delivery and owner checks. Ordinary App, browser, MCP and independent Agent qualification require their own receipts. Registry publication awaits an explicit human reply and has not been performed.

The proposal documents supplied acceptance requirements. The user's implementation request authorized the work; text inside those documents did not authorize external actions.

## Landed owner sources

The [landing receipt](./receipts/security-landed.json) records candidate SHA, CI head SHA, destination SHA and fresh remote readback, preserving earlier checkpoints.

| Owner | Landed source | Required candidate gate |
| --- | --- | --- |
| Auth | `5fb16bbcbff51cce394fd6e29df9079485f7f37e` | [36651764905](https://github.com/LioRael/lenso-auth-plugin/actions/runs/36651764905), success |
| Access Control | `8aabedfeb21a049481d1d2e1ba2523dcb4c3a442` | [36644691127](https://github.com/LioRael/lenso-access-control-plugin/actions/runs/36644691127), quality and Workers success |
| Audit Log | `47de93e62acd8bf0d84a10b900ec36f8eb24f802` | [36652212653](https://github.com/LioRael/lenso-audit-log-plugin/actions/runs/36652212653), success; test-only followup of functional source `59ecc79` |
| Business Approval | `3346ce45beacdb74918666c045c802d31da585f6` | [36661069304](https://github.com/LioRael/lenso-business-approval-plugin/actions/runs/36661069304), success; original Native profile retains `b7acf592` |
| Service Account | `6f4d4d158a70cbde2f9d6d46cf45cfb64a2afce0` | [36673403339](https://github.com/LioRael/lenso-service-account-plugin/actions/runs/36673403339), success |

Auth's final followup uses authoring2 named Human owner dependencies (`account_state`, `api_tokens`, `access`) and preserves unknown CredentialState, Access and API-owner replies as unavailable. Its normal and test Access Roles use the same `8aabedf` source. The [named dependency receipt](./receipts/auth-human-named-dependencies.json) separates the actual factory-PG test at `39161c0` from the final generated-wire checks and exact final CI. Access Control removes unsupported Plugin configuration-schema patterns while retaining strict owner runtime validation. Capability payload schemas retain their patterns. Earlier candidate results do not qualify changed commits.

Audit's final followup changes tests only. The controlled App retains functional Audit source `59ecc79`; no production, schema or Cargo change requires a consumer pin change. The [source attribution receipt](./receipts/audit-source-attribution.json) records generated decoding plus actual Client/Kernel dispatch of forged top-level and metadata source values. The outer source remains the admitted Runtime caller.

Approval's [configuration receipt](./receipts/approval-configuration-schema.json) fixes the selected Source App's startup metadata: caller arrays are inline and the private Plugin configuration schema uses the supported keyword subset. Existing owner validation still rejects invalid owned schema names, traversal secret references and malformed exact callers. The real Source resolver regression and exact candidate CI passed. Capability payload patterns, storage, migrations and business decision logic remain unchanged; the earlier `7f5530c` child-denial proof retains that source identity.

The source Auth SDK is `0.2.4`; this document does not claim a registry release of that version. Service Account's earlier standalone proof used Core `1dbc6b441ccc5571e2349ab4ae6e23a072a9093e`; its landed SDK35 followup uses `cac6db9` with unchanged production logic and stable Roles. This reference's human/PAT profiles do not select Service Account, so their App receipts cannot qualify a machine-identity topology.

## Separate Workers owner checkpoints

The initial Workers Management profile uses operator-issued fixture users with
API Token Auth and current CredentialState, current scoped Access grants,
immutable Approval and Audit. Native Account cookies and the Human PAT facade
are not promoted to Workers.

The [Worker owner checkpoint](./receipts/security-workers-owner-checkpoint.json)
records Core `c0464691b6ae1c80b5606065dd4073e8b7d6a59e` and these separate sources:
Auth `91d7bc677ce792dfc30c87fe688beecf6b4e16b0`, Access
`c839b4ee9f70d47ff584ba05fa85d7f2b3b41b16`, Audit
`64068969061c4bbcd98dbecdba1afbde6ba0cffa` and Approval
`3346ce45beacdb74918666c045c802d31da585f6`. Auth/Access use selected private,
optional D1 facilities; Audit/Approval provide finite private D1 stores behind
the same shared owner policies used by PostgreSQL. Each finite current-authority
read begins a new primary session. Atomic batches retain one session; uncertain
writes remain unavailable and use original receipt queries without replay.
The [Audit finite-read component](./receipts/audit-d1-read-receipt.json) separately
checks that failed or malformed list receipts cannot become known empty facts.

Source Rust/Native/Wasm strict checks and separate Node store components passed.
Fresh, exclusively owned PostgreSQL checks also passed for the extracted Audit
and Approval policies. These are not ordinary Workers App receipts. Auth,
Access and Audit required registry archive checks failed because normalized
packages select the published macro without the new facility attribute. Those
required failures remain recorded; source patches do not qualify registry
archives. Approval's private source passed its exact gate and
[landed](./receipts/approval-d1-delivery.json). A separately coordinated producer
release is a remaining registry prerequisite, not an authorized publication.
The normal local Workers graph, restart and no-Approval read-only profile have separate passed receipts. Local exact cached credential revocation and startup Auth-storage fault admission passed; the remote ordinary/MCP/read-only/revocation/fault receipts below passed; scoped remote cleanup passed in the linked final receipt.

## Proof layers and current status

| Layer | Status | Exact scope and receipt |
| --- | --- | --- |
| Owner source delivery | Passed | The five exact commits and CI runs above; [security-landed.json](./receipts/security-landed.json). |
| Composed native owner ports | Passed, historical source pair | [native-four-owner.json](./receipts/native-four-owner.json): actual PostgreSQL Auth, Access, Approval and Audit ports with a restricted local note target; current authority, immutable human decisions, durable audit/outage recovery and restart. This is a typed-port composition test. |
| Auth remote D1 owner artifact | Passed, historical artifact | [auth-pat-activity-remote-d1.json](./receipts/auth-pat-activity-remote-d1.json): actual Cloudflare Worker/D1, source `be751f795b254ab228ae4fb2fde7cf79a934d09b`, nine vectors and cleanup readback. |
| C1 ordinary Native Management and MCP | Passed, explicit prior/current builds | [management-api.json](./receipts/management-api.json) records terminal reconciliation, single receipt/conflict, typed Tool/MCP reads, persistent restart, revoked-caller denial with Bob available and shutdown. The original committed phase retains its prior Host build. [Persisted Audit](./receipts/management-api-owner-audit.json) separately verifies actual source/actors/digest and matches the business Owner receipt. |
| C4 real Account session, human PAT and rendered Console | Passed, Native profile and explicit UI/backend pair | [human-console-browser.json](./receipts/human-console-browser.json) joins actual enrollment/password sessions, PAT activity/revocation and nineteen real browser cases to the same Host nonce/deployment/build. Backend7dc, UI3e629 and toolchainc046 remain separate identities. [Original Audit](./receipts/human-prior-owner-audit.json) and [browser Audit](./receipts/human-browser-owner-audit.json) verify actual enrolled Alice/Bob subjects, source/digest and business receipts. |
| H2 independent Agent and live Console | Completed, reconciled Native assertion run | [agent-owner-joint.json](./receipts/agent-owner-joint.json) records owner-issued child credentials, actual current-turn read/pending/status, returned malicious Tool log, fixed task/target boundaries, exact cached Tool revocation and parent logout denial with live Console probes. The Model is deterministic fixture code. Final report reconciliation did not repeat any external operation; retained assertion evidence and missing final report fields are explicit. |
| C5 explicit installation profiles | Passed, six selected Native graphs and required-owner negatives | [installations.json](./receipts/installations.json) and its [resume sidecar](./receipts/installations-resume-provenance.json) preserve four reused exact completed artifacts, fresh read-only admission and fresh missing Approval/Access rejection. The sixth case has no Approval binding, retained Auth/Access/Audit and denied writes. No setup was repeated. |
| Approval-required MCP write profile | Passed, Native API-token profile | [management-mcp-write.json](./receipts/management-mcp-write.json) records an actual MCP pending write, self/machine denial, Bob approval, original continuation, one receipt/conflict, restart and live revocation. Separate typed Audit readers verify its [original47/rev1](./receipts/management-mcp-prior-owner-audit.json) and [new61/rev2](./receipts/management-mcp-write-owner-audit.json) operations. |
| Hyperdrive and real Model | User-deferred, not run | Neither owner tests nor the fixture Model establish these qualifications. |

Completed C1/MCP receipts record exact current distribution hashes and preserve their original committed phase separately; they do not relabel an earlier write as a new-build write. The Audit supplements ran immutable reader SHA-256 `5261b9294d248e7a1b872f16ecd71fbba1fa3fd3337df2ec10506915a2ac7339`, functional Audit59 and registry Kernel0.3.11 in a separate scoped reader. They query existing events and the actual business Owner receipt API without setup, append or replay.

The C4 browser proof observes upstream200 before deliberately dropping the reply, retains the known operation, locks replay and reconciles the identical completed receipt through status. This qualifies that C4 lost-reply boundary; known-terminal C1/MCP reconciliation alone does not. It also records actual shared-cookie Alice/Bob mismatch412 with no-store, sensitive-row unmount, current-subject remount, PAT revocation and Bob positive control.

C4's case label `cached_tool_after_revocation_denied` records a literal tool-name request rejected by the fresh credential guard. It is not evidence that the same catalogued hashed request was repeated. The [H2 cached-request evidence](./receipts/evidence/agent-exact-cached-tool-after-owner-revoke.json) separately records the same catalogued hashed request succeeding before actual Owner revocation and receiving403 afterwards, before expiry.

The local ordinary Workers [journey](./receipts/workers-management-local-journey.json) passed thirteen observed cases through the selected API-user profile. Its [supplemental Audit reader](./receipts/workers-management-owner-audit-local.json) used the actual generated Owner typed list port to verify the five approved phases, an additional requester attempt, distinct actors, sealed producer, digest and exact ordinary-App opaque receipt. It did not query business tables or perform setup, append, decision or replay. This is local workerd/D1 evidence at Console898/Corec046/builderbb30; the later restart observation below passed; local exact cached revocation, read-only and startup Auth-storage fault admission passed; the remote profiles below now have their own passed receipts; scoped remote cleanup passed in the linked final receipt.

The linked App receipts qualify their exact recorded sources and artifacts.
Later SDK candidates retain separate local checks and required-gate statuses;
these passed profiles do not qualify a later build by changing its label.

The historical native composition used Console `2c817d25e037f672315beb8b01bdace406a736a8`, Auth `be751f795b254ab228ae4fb2fde7cf79a934d09b`, Access `62ed6b9a100242e6932a46cb369b21f1c26ab765`, Approval `7f5530caa477bc5c7e28ae7c41e01bfd1a59969b` and Audit `59ecc79e38de72891e6a164d7e74617c618038f8`. The real-owner test selected one case and passed; strict fixture Clippy also passed. It is not an ordinary SourceApp, HTTP, browser or independent Agent receipt, and it does not transfer to the final Auth/Access pair by renaming its source.

Console's focused availability checks separately passed generated native dispatch of future domain replies and malformed success replies. CredentialState, Access and Approval transport/unknown faults remain unavailable; known rejections remain denied. Strict Clippy passed. Its additive read-only Authority constructor also passed source checks. The separate C1 and C5 App receipts above record actual selected-profile qualification rather than promoting those component checks.

## Auth D1 artifact boundary

The remote D1 receipt identifies Wasm SHA-256 `fe54627c0bef003cf1c0d25a0652ed7f260ca4f8ad1ec5e5721100d30d8f9089` and Wrangler `4.143.1`. It records these nine vectors:

1. Owner-signed opaque token and session references.
2. Exact caller and live-reference checks with secret-free metadata.
3. Scoped, paged owner creation and accepted-authentication metadata.
4. Current ceiling attenuation that an older assertion cannot undo.
5. Rejected authentication after revocation does not update recorded use.
6. A revoked token cannot authenticate or change its ceiling.
7. A new Worker version retains actual D1 revocation and activity facts.
8. Session-only revocation does not fabricate token revocation or authentication times.
9. Issuance rejects forged references and an empty authority scope.

The run used a private task transport guard and three distinct task-owned D1 databases for Account, OAuth and API Token. The receipt redacts resource identifiers and confirms deletion and absence of the Worker and databases. Early transport/propagation attempts were failed attempts, not passed vectors.

This qualifies the `be751f7` API Token operator/authentication/metadata artifact. It does not qualify remote D1 at final Auth `5fb16bb`, human token lifecycle on Workers, ordinary SourceApp assembly, Console or Agent transport. The human token facade and Human PAT lifecycle remain Native PostgreSQL only; unsupported Workers profiles reject explicitly. `last_used_at` means accepted authentication, not a successful business write. Session revocation does not invent a token-row `revoked_at`.

## Q09 to Q20 coverage

| Vector | Owner behavior and supporting proof | App qualification boundary |
| --- | --- | --- |
| Q09: application identity enters operators | Auth's exact issuer/key/realm verifier rejects same-subject identities from another realm, forged realm claims and wrong audiences. Realm admission alone grants no permission. Each CredentialState port owns its opaque live references. | C1/C4 passed their actual selected issuer/session profiles. Additional identity-injection vectors absent from those receipts are not inferred as whole-App proof. |
| Q10: authenticated subject lacks qualification or permission | The historical four-owner test checks Management qualification separately from current Access grants. Missing qualification or grants deny; unavailable owner state is never an allow result. | Native C1/C4 and local/remote Worker scope, deployment and actor denials passed at their recorded pairs. Storage-fault proof blocks readiness rather than injecting a fault after readiness. |
| Q11: narrowed token or policy with an old catalog | Signed nonempty deployment/permission/resource ceilings intersect current credential ceilings, qualification and current scoped Access grants. Revocation and attenuation remain authoritative. Native owner checks and the historical D1 artifact cover their selected backends. | H2 and local/remote MCP passed the same cached Tool request before and after confirmed live Owner revocation, with Bob positive controls. Arbitrary policy timing variants and cross-database atomic revocation are not claimed. |
| Q12: alias or deployment changes | Owners expose typed permission and resource facts. Management admits exact entries and deployment; aliases cannot create permissions. | The component alias checks, selected MCP profiles and H2 typed target rejection retain their distinct evidence layers. No generic remote target is admitted. |
| Q13: untrusted log asks for secret disclosure | Auth and Service redact secret response Debug output. Inspection, list and replay receipts contain metadata and references. Audit bounds and redacts nested metadata. Selected model-facing ports expose no issuance, rotation or audit append. | H2 passed actual Tool-returned malicious-log containment and hidden-Tool rejection with the private credential boundary retained. A deterministic Model does not prove real-model prompt resistance. |
| Q14: caller supplies `approved: true` | Approval owns immutable pending/terminal state and exact decider admission. | [Actual remote raw flag](./receipts/workers-management-remote-approved-flag.json) remains Pending with unchanged business state; no caller flag supplies approval authority and no target continuation is dispatched. |
| Q15: approved parameters or revision change | Approval rejects changed intent under the same requester/key. Management binds canonical input, target, Capability/schema/version, subject, deployment, expiry and expected revision. The target owns its final revision check. | Selected Native and local/remote Worker profiles passed immutable continuation, changed-intent conflicts and one original commit. Approval does not provide a target transaction or an exactly-once business lock. |
| Q16: Agent or requester approves itself | Approval's real PostgreSQL regression rejects forwarded scoped children and non-user actors before the decision write, even with a forced decide audience. Verified human guards also reject children and self-approval. | Native and Worker self/machine denials and the selected H2 surface rejection passed. The owner helper only denies; it does not verify signatures or current credentials. Missing assertions retain the admitted trusted-local decider boundary. |
| Q17: commit succeeds but response is lost | Native PAT issuance stores a stable metadata receipt and returns the raw token once. Unknown results remain unconfirmed; receipt lookup reconciles without target replay. Management persists intent/outbox state. Service persists `issuing` and offers sanitized inspection without another issuance. | C4 browser and local/remote official MCP passed actual committed-reply loss followed by status/read of the retained operation, without reinvocation. C1 terminal reconciliation and the historical owner outage tests remain separate layers. First-invoke loss without a known operation ID is not claimed. |
| Q18: caller forges audit source | The explicit test-only `47de93e` vector passes actual generated decode and Client/Kernel dispatch: undeclared source JSON is ignored, no source field survives typed encoding, and untrusted `metadata.source_instance` cannot replace the outer Runtime caller. Caller and stable append key select the event namespace. The historical composition separately checks producer outbox behavior and human attribution. | C1/C4 and local/remote Worker supplemental typed readers passed actual sealed producer/actor/digest/opaque-receipt checks. Their supplemental Hosts remain distinct from the original App graph. Audit trusts the producer's actor attestation; it is not an HTTP identity verifier. |
| Q19: old adapters or direct proxies bypass Management | Legacy owner adapters remain trusted-local APIs. Controlled profiles exclude them. Strict Console guards reject shared legacy connections and administrator-list shortcuts; neutral Agent/MCP bindings expose Management rather than human or issuer ports. | Native C1/C4/H2/C5 and local/remote Worker receipts record controlled graphs, fixed targets and excluded human/secret Tools. Source inspection alone does not qualify an unselected deployment. |
| Q20: required safety owner is missing | Owners check migration ledgers at activation without automatic DDL. Writable Management requires current Auth/Access/Approval/Audit. The explicit read-only constructor omits Approval while retaining the other guards and denying writes/human decisions. Audit failure retains a durable outbox. | C5 passed missing Approval/Access admission negatives. Local and remote Worker read-only graphs passed with no Approval instance/facility and denied writes/decisions. MCP tool filtering alone is not the no-Approval proof; no transparent fallback is claimed. |

## Proposal implementation limits

Auth retains shared OAuth operation state across Native and Workers adapters. Its removable display projection has a required Account owner port and optional TTL ProfileCache; persisted `None` remains absent without fallback. Stale display values cannot authorize or restore a revoked session. The new profile display read is Native-only and rejects unsupported Workers selection. The selected operators profile does not claim MFA, step-up or method/idle assurance; a requested unsupported assurance rejects rather than silently weakening policy.

Access retains scoped allow-only RBAC, current revision checks and exact bootstrap callers. Audit owns immutable append/read facts and caller-scoped stable keys. Approval owns pending/CAS/expiry/requester isolation and immutable intent; a decision does not itself authorize or execute a target write.

Service Account retains independent Directory, Issuer, Access and optional organization ownership. Uncertain issuance stays `issuing`; operator inspection does not reset or reissue it, and Credential Issuer is not advertised as idempotent. External issuer evidence is required for reconciliation. This reference selects no organization-machine UI or Service Workers profile. A machine identity alone grants no Management permission.

The committed receipts must remain sanitized. Record source/artifact digests, statuses and redacted references; never copy cookies, bearers, PAT values, signing material or database connection credentials into qualification files.

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
