# Knowledge base App

This is the runnable Lenso knowledge base reference App. It
starts from an ordinary source App: the App owns one linked Rust Plugin under
`project/app/`, while Engine derives the Host Catalog, bindings, and immutable
Plan.

The current slice supports:

- a normal Vite/React browser page at `/`;
- `POST /notes` to create a note; and
- `GET /notes/{note_id}` to read it back;
- deterministic excerpt work through an optional durable Jobs dependency; and
- `POST /jobs/process-next` plus `GET /job-status/{job_id}` for processing and
  observing that work.

Creating a note calls the App-owned Bun Plugin at `project/app/excerpt`. That
Plugin provides the knowledge tools through the public Agent Tool Capability,
so the request crosses the Rust/TypeScript boundary before the note is stored.
When Jobs is not adopted, the Plugin completes the deterministic excerpt
inline. When Jobs and Secrets are adopted, it enqueues, claims, completes, and
inspects the same work through the typed Jobs Capability. The excerpt is not
represented as a model result.

The React source under `frontend/` consumes generated types and the browser
runtime from an independently packed `@lenso/web-client` candidate. The
checked-in static assets keep the ordinary App build self-contained. To
regenerate them from a reviewed package candidate:

```sh
python3 verify.py \
  --cli /absolute/path/to/lenso \
  --web-client-package /absolute/path/to/lenso-web-client-0.1.0.tgz
```

No App-owned Host, Plan, Runtime Profile, or binding document is present.

```sh
lenso app dev --root project
lenso app build --root project --out dist
lenso app start --from dist
python3 verify.py --cli /absolute/path/to/lenso
```

`verify.py` can first install the candidate tarball, regenerate the client,
typecheck React, and rebuild the static assets. It then builds the distribution,
removes all source, and proves the asset/create/read paths through the real HTTP
listener. This is the consumer proof for the slice, not a second App definition.

The optional Jobs acceptance also needs local candidate source roots because
the linked Plugins have not been published. Point it at a disposable PostgreSQL
database: the command creates a uniquely named schema and intentionally leaves
it in place for inspection.

```sh
LENSO_JOBS_DATABASE_URL=postgresql://... python3 verify.py \
  --cli /absolute/path/to/lenso \
  --jobs-source /absolute/path/to/lenso-jobs-plugin/crates/lenso-jobs-plugin \
  --secrets-source /absolute/path/to/lenso-secrets-env-plugin/crates/lenso-secrets-env-plugin
```

This mode proves the source-deleted distribution, real PostgreSQL work, and
durable job inspection after a Host restart. Notes and generated excerpts are
still process-local, so this is not yet the complete persistent application.
Login, user isolation, file upload, persistent note/result storage, and dynamic
configuration remain to be implemented and must not be inferred from this
slice.
