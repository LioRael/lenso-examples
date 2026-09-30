# Supplemental Audit Owner inspection

This standalone reader queries existing Audit Log facts through the generated
`AuditLogClient.list_events` port. It mounts the actual PostgreSQL Audit Owner
in a separate local Kernel graph with one fixed reader and an exact deployment
scope. It does not run setup, migrations, append operations, or direct SQL.

The verifier requires five persisted phases for one approved `state.update`:
request, human attempt, human decision, dispatch, and successful completion.
It checks the sealed Management producer, exact operation references, distinct
actual requester/decider subjects, shared intent digest, phase states, and
opaque domain receipt ID. Audit events do not contain the business result JSON.
A self-approval rejection can retain an additional requester `human_attempt`.
The proof validates that attempt separately and still requires one approved
decider attempt/decision and exactly one dispatch/completion.
A retained exact operation and response receipt can narrow the lookup further.
The reader calls the business storage Owner's read-only
`PgStateHandle::receipt(operation_id)` API to correlate each completed audit
receipt with the actual value/revision in the completed qualification. It
requires one matching business receipt in the selected deployment and verifies
the opaque Audit receipt ID against it. It never replays a write to find an
operation. All SQL remains inside the owning storage library.

`../verify_audit.py` binds the supplemental proof to a completed ordinary-App
receipt and its retained Host build receipt. Human subjects come from the
actual enrollment receipt. Private inputs and database credentials are read
from files; public output contains no actor identifiers, database URI, secret,
or event metadata. The supplemental reader has a distinct executable identity
and is not proof that the original App contained an Audit inspection route.

Build this workspace with the repository's normal Cargo configuration. Then
pass the reader executable, retained private test directory, completed public
qualification, and new supplemental receipt path to `verify_audit.py`.
