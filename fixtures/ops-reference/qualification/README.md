# Qualification evidence

This directory packages completed evidence at its recorded proof layers. It does not declare the reference fully qualified. [Profile inputs](./profile-inputs.json) identify the latest selected source candidates separately from receipt-only historical Native/Worker builds; they are not runnable preparation overrides or private Owner setup. [security.md](./security.md) maps the security owners and Q09-Q20; [contracts-and-tests.md](./contracts-and-tests.md) maps all Q01-Q24 and their actual proof layers. [proposal-completion.md](./proposal-completion.md) distinguishes each proposal's implementation, exact gate and profile limits.

Included JSON receipts are copied byte for byte from the task's shared `.artifacts` directory. [SHA256SUMS](./receipts/SHA256SUMS) records their hashes. The copy check compares task-owned private credential files without recording their values. No password, bearer, PAT, signing material or credential-bearing database URI belongs in this package.

## Completed scope

| Evidence | What it establishes |
| --- | --- |
| [Security owner delivery](./receipts/security-landed.json) | The validated candidate, required CI head, exact fast-forward destination and remote readback for each owner. It is not package publication or App qualification. |
| [Historical native four-owner composition](./receipts/native-four-owner.json) | The `native_four_owner` field records one actual PostgreSQL owner-port composition test and strict fixture check. This file is an exact-byte alias of shared `security-integration-followups.json`; its other fields are separate integration history. It is not a newly constructed receipt or ordinary App/browser run. |
| [Auth Human source authoring](./receipts/auth-human-named-dependencies.json) and [Audit source attribution](./receipts/audit-source-attribution.json) | Exact source, focused checks and required CI for named owner dependencies, unavailable live-owner outcomes and actual generated-wire/Kernel source attribution. |
| [Approval configuration compatibility](./receipts/approval-configuration-schema.json) | Actual Source resolver and strict-owner format checks, followed by exact candidate CI/main readback. It changes private configuration metadata, not Capability payloads, decisions or DDL. |
| [Historical Auth remote D1](./receipts/auth-pat-activity-remote-d1.json) | Nine real owner activity/revocation vectors on the recorded `be751f7` Wasm artifact, with task-owned resource deletion and absence readback. It does not qualify the final Auth source or Human PAT on Workers. |
| [Core delivery](./receipts/core-35c0-delivery.json) | Exact Core source CI, fast-forward and readback. Backend qualification is recorded separately. |
| [Current Core delivery](./receipts/core-c046-delivery.json), [Approval D1 delivery](./receipts/approval-d1-delivery.json) and [Workers owner checkpoint](./receipts/security-workers-owner-checkpoint.json) | Current source startup/facility implementation and private Approval delivery. Auth/Access/Audit normalized archive failures remain failures; none of these receipts qualify the ordinary Workers Management graph. |
| [Audit finite list read](./receipts/audit-d1-read-receipt.json) | Seven Node components at the exact JS descendant; malformed read receipts remain unavailable and cannot erase persisted facts. This is not a workerd or App proof. |
| [Local security Owner provisioning](./receipts/workers-owner-provision-progress.json) | A supplemental private workerd Host explicitly set up and verified five Owner stores, issued bounded fixture credentials, checked actual user/machine kinds, inspected scoped roles and granted Management qualification. The initial pre-I/O Host declaration failure is retained and reconciled by an actual Owner absence read before one explicit replacement action. This does not qualify the ordinary Workers Management graph or a remote deployment. |
| [Local ordinary Workers journey](./receipts/workers-management-local-journey.json) and [persisted Audit](./receipts/workers-management-owner-audit-local.json) | Thirteen observed cases on Console898/Corec046 with builderbb30: actual API-user authentication, current catalog/read, immutable pending intent, self/machine/wrong-intent rejection, Bob approval, original commit and exact status receipt. A separate scoped typed Owner reader verified five approved Audit phases plus one requester attempt against that actual response. The separate restart observation below passed; local revocation, explicit read-only and startup storage-fault profiles also passed; remote qualification has its own passed profile receipts below; scoped remote cleanup passed in the linked final receipt. |
| [Ordinary Native Management](./receipts/management-api.json) and [bounded MCP writes](./receipts/management-mcp-write.json) | Actual offline distributions reconcile their separately identified original committed phase, exercise typed Tool/MCP reads, preserve one business receipt, restart, reject revoked credentials and keep Bob available. The MCP profile also performs a new pending/approved write to value61/revision2. Shutdown passed. |
| [Native Audit supplements](./receipts/management-api-owner-audit.json), [MCP original operation](./receipts/management-mcp-prior-owner-audit.json) and [MCP write](./receipts/management-mcp-write-owner-audit.json) | Separate fixed typed Kernel readers inspected actual persisted Audit facts and matched opaque receipts through the business Owner API. Each operation has five approved phases plus a separately verified requester attempt. This is supplemental inspection, not an inspection route inside the original App. |
| [Current Console delivery](./receipts/console-dbea-delivery.json), [Agent delivery](./receipts/agent-cc27-delivery.json), [Core builder](./receipts/core-bb30-delivery.json) and [JS delivery](./receipts/lenso-js-managed-release-delivery.json) | Exact accepted gates and same-SHA landing for dbea/cc27/bb30/cb6. Older profile receipts retain their actual 7dc/cdcb/c046/18e identities. JS runtime package bytes are unchanged from18e. These source gates do not rename profile runs or publish packages. |
| [Independent Agent and live Console](./receipts/agent-owner-joint.json) | Completed Native owner-issued child and deterministic independent Model/Loop run, actual Tool-returned malicious log, hidden/target rejection, retained pending reference without another write, exact cached Tool denial after PAT revocation and child denial after parent logout. Agent task sourcecc27/executable6b3a and issuer/target Host builds are recorded separately. The final receipt was reconciled read-only after a report-variable error; no external operation was repeated. The retained helper and failure trace establish the completed assertions; individual concurrent probe names and the final child-process return code were not retained. |
| [Human Console browser](./receipts/human-console-browser.json) and [persisted browser Audit](./receipts/human-browser-owner-audit.json) | Actual Owner enrollment/password sessions, immutable human approval, once-only PAT/metadata/authentication/revocation and nineteen browser cases. An actual upstream200 reply was dropped; UI locked replay and status returned the identical completed receipt. Shared-cookie mismatch returned412/no-store and unmounted stale rows; Bob remained available. Backend7dc, UI3e629 and toolchainc046 are recorded separately. |
| [Original human operation Audit](./receipts/human-prior-owner-audit.json) | The retained47/rev1 operation has six verified events. The later browser61/rev2 operation has five approved phases and an exact actual response receipt. Both use separate read-only Owner/Kernel inspection. |
| [Native PG](./receipts/ops-35c0-native-pg-diagnostics.json), [simulated Native](./receipts/ops-35c0-native-sim.json), [local Workers D1](./receipts/ops-35c0-workers-local.json) and [remote Workers D1](./receipts/ops-35c0-workers-remote-query.json) | Ordinary business distributions on the recorded Core35 artifact. Real PostgreSQL and Cloudflare D1 have their own receipts; simulation and local workerd retain `real_database: false`. |
| [Optional/profile admission](./receipts/ops-35c0-optional-diagnostics.json) and [generated web starter](./receipts/ops-35c0-web-starter.json) | Persisted explicit absence, specific negative startup diagnostics, source App creation and offline web startup. These do not select the security Management/browser profile. |
| [TestSimulator](./receipts/ops-test-simulator-final.json), [Workers scope components](./receipts/workers-scope-components-18e.json) and [Agent transport component](./receipts/agent-management-transport-final.json) | Their recorded component layers only. The Agent transport fixture did not issue a real owner child or qualify the independent Agent/Human deployment. |
| [Failed remote bulk-upload attempt](./receipts/ops-35c0-workers-remote.json) | A retained failed attempt with confirmed task-owned D1 cleanup. The separate direct-query remote receipt records the passed run; failure history is not rewritten as success. |
| [C5 installations](./receipts/installations.json) and [resume provenance](./receipts/installations-resume-provenance.json) | Six selected Native profiles and missing Approval/Access negatives completed. Four previously completed profiles reuse identical artifacts; the read-only and required-owner negatives are fresh. No repeated setup is claimed. |
| [Local Worker MCP](./receipts/workers-management-local-mcp-write.json), [restart](./receipts/workers-management-local-mcp-restart.json) and [typed Audit](./receipts/workers-management-local-mcp-owner-audit.json) | Actual official Client2.2/protocol2025-11-25 on dbea: immutable pending/Bob approval, upstream commit reply discarded, status-only receipt recovery, value49/revision2 and same-artifact restart. The separate Owner reader verifies actual persisted phases/actors/digest/opaque receipt. |
| [Local revocation before](./receipts/workers-management-local-revocation-before.json) and [after](./receipts/workers-management-local-revocation-after.json) | The same actual catalogued hashed request succeeds, then fails after confirmed Owner revocation before expiry; original Alice also fails while Bob remains available and business49/rev2 is unchanged. |
| [Local storage-fault admission](./receipts/workers-management-local-auth-storage-fault.json) | An isolated uninitialized Auth binding blocks whole-graph readiness with503; healthy controls remain available. This is not an after-ready per-authentication storage fault. |
| [Service SDK35 delivery](./receipts/service-sdk35-delivery.json) and [immutable Role analysis](./receipts/security-primary-role-cohort-analysis.json) | Service6f4 exact CI/main and current SDK35 source/archive checks passed. Seven published Auth/Access Role contract archives remain immutable and compatible; source checks do not establish producer registry publication. |

## Source versions and transport

The historical business profile receipts select Core `35c0e04ad4d85b12cc79ea660260d27ba871a518`, JS `18e3cfb2837c8dfe5d5b907e39fe95ae15dc0a65` and source CLI `0.6.4`, SHA-256 `06d5877850d79189701c58aade5b5462c7c0e0fdc0d1c117c986b2e78ec3fa41`. The registry wrapper recorded alongside them is `@lenso/cli 0.17.3`, which bundles Native CLI `0.6.3`. Published facade `lenso 0.5.27`, codegen `0.10.0` and source Auth SDK `0.2.4` are separate identities; a source candidate or archive check does not publish a version. Exact owner sources and CI are in [security.md](./security.md), and historical receipts retain their original source identities.

`verify_cloudflare.py` selects `LENSO_REFERENCE_HTTP_TRANSPORT=node` before remote verification and restart. A standalone remote `verify.py` invocation must select that environment variable too. It selects the owner's single-request Node fetch transport; it does not retry or switch transports after a dispatched write. Local verification keeps its normal Python transport.

Human PAT management remains Native PostgreSQL only. Its unsupported Workers profile rejects explicitly. Auth's `last_used_at` records accepted authentication, not successful business execution. The reference does not qualify a Service Account or organization-machine App topology.

The immutable C4 receipt contains a case named `cached_tool_after_revocation_denied`. That request used a literal tool name and establishes fresh credential-guard denial, not replay of a previously catalogued hashed tool request. H2 separately qualifies the exact cached request before and after actual Owner revocation.

## Installations and remaining profiles

C5 [installations](./receipts/installations.json) completed six selected Native
profiles, including explicit read-only without Approval, Console/Agent removal
and missing required-owner negatives. The [resume sidecar](./receipts/installations-resume-provenance.json)
binds the immutable raw receipt and distinguishes four exact completed artifacts
reused without Owner setup from fresh read-only and negative checks. Earlier
pre-ready failures remain retained; they are not converted into passed profiles.

Final SDK35 consumer gates,
Examples aggregate and Site finalization remain separate pending work.

The completed Native C1/MCP receipts preserve the original committed Host build separately from the rebuilt distribution used for terminal reconciliation, Tool/MCP transport, restart and revocation. They do not claim the initial write ran again under the new build. An absent pending file does not mean a passed case. Bootstrap/enrollment/grant markers prohibit blind resume after a potentially effective action. Unknown writes retain their original references and use owner receipt queries without automatic target replay.

Hyperdrive and a real Model were explicitly deferred by the user. Registry publication awaits explicit human approval; it has not been performed. Root owns the final aggregate completion state and manifest; this static package does not replace them.

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

The successor [Core83bf Native CLI build](./receipts/ops-source-cohort-83bf-cli-build.json)
passed at exact source `83bf2d94e29ed4fb7878ade88482abcd417c4ba1`;
its [focused checks](./receipts/core-sdk-83bf-focused.log) are a separate
bounded source layer. Its final producer archive review and exact CI remain
unrecorded here. The 1b34 review above remains historical evidence.

The [PRIMARY Agent Role analysis](./receipts/agent-primary-role-cohort-analysis.json)
and [independent archive comparison](./receipts/security-agent-primary-role-review.json)
confirm seven immutable registry versions/checksums, compatible caret
Kernel/Codec requirements, and the exact legacy build Codegen `0.9.0`.
Descriptor, schema and wire-contract files are unchanged. Source manifests
and two empty `CatalogRequest` `Copy` derives differ from PRIMARY bytes;
this does not claim newly normalized Source archives are byte-identical or
that those same Role versions should be republished. The separate
[normalized SDK4/macros4 consumer](./receipts/security-agent-sdk-primary-compatibility.json)
passed two unchanged authoring tests against all seven PRIMARY Roles at
Agent source `d1d0fd0c950f9a5c5575251d69d1db002d4ab6ef` and Core83bf.
Both actual normalized SDK archive hashes and all seven PRIMARY checksums are
recorded. The [first fixture failure](./receipts/agent-sdk-primary-normalized-initial-failure.json)
is preserved: its consumer omitted required Lenso package metadata. The
[narrow continuation](./receipts/agent-sdk-primary-normalized-metadata-continuation.json)
added that metadata and reused the same archives, lock and tests; it did not
repackage or patch a Role. This compatibility layer does not pass a separate
Agent Host adaptation, a later candidate, a registry-only consumer or a
publication gate.

The separate [Agent81d Host checkpoint](./receipts/agent-sdk35-81d2-host-validation.json)
records two runtime-admission tests and the Host check passing, then a strict
lint failure; the example build was not run after that failure. Its log is
explicitly a tool-output excerpt with RTK summaries. This is a partial source
checkpoint awaiting its correction and exact gate, rather than completed
Agent delivery.

The [8059 successor check](./receipts/agent-sdk35-8059-strict-example.json)
then passed formatting, affected strict checks and the actual `management_task`
example build. It reuses the earlier two admission tests and Host compile
because the successor only spells out default types and already-resolved
direct adapter dependency edges; the receipt records that reason. Required
remote quality/SDK gates and same-SHA landing remain pending. The normalized
SDK archive proof above retains its earlier exact source identity.

The historical [Core5bb4 focused checks](./receipts/core-sdk-5bb4-focused.json)
passed seven tests and strict checks. Its clean
[CLI0.6.5 build](./receipts/ops-source-cohort-5bb4-cli-build.json) records
binary SHA-256 `ca8f8885734a1c3c22afd5ddf284e2ac7e093f3283ec266a244f304dd4fd8f5b`.
The [exact 35-package source preflight](./receipts/core-sdk-5bb4-archive-preflight.json)
passed normalized extracted all-target and test compilation; only the
EngineApp archive digest changed from83bf, with the other34 archives identical.
That receipt establishes source-cohort archive compatibility, not primary
registry availability. Core exact CI `36687712454`, Agent8059 quality
`36687698023` and CLI npm dry-run remain unrecorded here. Agent SDK gate
`36687698181` failed and is being diagnosed;
the local normalized-consumer pass does not replace that required gate.

The final [joint nine-graph resolution](./receipts/ops-sdk35-final-metadata.json)
and [Native Management source check](./receipts/ops-sdk35-final-native-management-source-check.json)
passed at Core5bb4/Agent8059/Consoleed1 and the latest four Owner candidates.
The resolution index retains lock/metadata hashes and the five exact earlier
resolutions reused after a recorder incorrectly expected Kernel in the
standalone storage helper. Four remaining graphs resolved normally; the
storage helper correctly has no Kernel dependency. This is compilation only:
no setup, runtime execution or remote-profile qualification occurred.

The [actual emitted Rust process](./receipts/ops-generated-managed-rust-5bb4.json)
then passed CLI creation/check/execution without modifying its generated SDK4
manifest or source. It uses the immutable PRIMARY ToolProvider checksum and
task-only Core/normalized-SDK patches; its actual typed response is `HELLO`
with content type `text`. Both adopted terminal compiler paths executed and
emitted current facade5.28 and Bun4.2/Runtime3.1 dependencies. This is
compiler/output and process-Plugin execution proof, not a terminal-output
build, Auth/Management profile or registry-publication proof.

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


## Final source checkpoint

The [profile-input index](./profile-inputs.json) separates current selected
Coreb8/Agentbec/Consoleed1 candidates from the historical qualified Native
and remote Worker builds. It references retained source snapshots and build
manifests, excludes private Owner setup, and supplies no runnable override.
The remote preparation snapshot deliberately retains its original
awaiting-public-key/not-run fields; the later build and qualification
receipts establish what actually ran.

The [b8 artifact review](./receipts/core-sdk-b8d1-artifact-review.json),
[35-package preflight](./receipts/core-sdk-b8d1-archive-preflight.json) and
[clean CLI build](./receipts/ops-source-cohort-b8d1-cli-build.json) passed.
All35 normalized archive hashes equal5bb, and the CLI binary also equals
its tested5bb predecessor. The successor changes only an excluded lifecycle
test. Its [focused check](./receipts/core-sdk-lifecycle-fixture-focused.json)
uses published SDK0.3.3 only for that test; production emitted SDK4 is
unchanged. The actual emitted-process/compiler proof is reused by binary
identity, not replayed. [Nine final source resolutions](./receipts/ops-sdk35-b8d1-bec9-metadata.json)
passed at b8/bec. The earlier Native Management compile retains its
5bb/8059 identity and is not relabeled as a new runtime qualification.

The [Agent workspace continuation](./receipts/agent-sdk35-web-acp-workspace-strict-continuation.json)
passed at exactbec after a narrow web-fixture correction and heap-pinned ACP
startup future. The [preceding strict failure](./receipts/agent-sdk35-web-fixture-workspace-strict.json)
remains retained. No Role version or PRIMARY archive was republished.

The [current exact gate snapshot](./receipts/security-final-source-status.json)
records Coreb8 CI36689302394 successful for quality and Bun conformance;
[exact delivery](./receipts/core-sdk-b8d1-delivery.json) confirms same-SHA
Main fast-forward and remote readback.
Agentbec CI36689241104 failed only the producer-blocked generated-contracts
step and its aggregator; static integrity, workspace/Foundation and
marketplace steps passed. Its [actual failure log](./receipts/agent-sdk35-bec9-quality-contracts-failed.log)
remains authoritative. Agent landing and the coordinated latest
Owner/Console archive closure are held. The CLI dry-run result is not
recorded completed here; Main SDK dry-run36691729181 was queued separately. Producer publication still awaits a human reply;
no registry mutation is inferred from source or archive checks.

The [task npm-registry shutdown](./receipts/npm-sdk11-local-registry-shutdown.json)
confirms its owned process/listener closed and its proof/cache retained;
other user previews were untouched. Historical profile receipts and their
cleanup conclusions remain unchanged.
