# Knowledge base App

This is the first runnable slice of the Lenso knowledge base reference App. It
starts from an ordinary source App: the App owns one linked Rust Plugin under
`project/app/`, while Engine derives the Host Catalog, bindings, and immutable
Plan.

The slice intentionally proves one complete user action before adding Auth,
PostgreSQL, background processing, and dynamic configuration. It supports:

- a normal Vite/React browser page at `/`;
- `POST /notes` to create a note; and
- `GET /notes/{note_id}` to read it back.

Creating a note calls the App-owned Bun Plugin at `project/app/excerpt`. That
Plugin provides the `knowledge.excerpt` tool through the public Agent Tool
Capability, so the same request crosses the Rust/TypeScript boundary before the
note is stored. The excerpt is deterministic and is not represented as a model
result.

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

Login, user isolation, upload, background processing, PostgreSQL, and dynamic
configuration are not implemented by this slice and must not be inferred from
the typed public API and cross-language proof.
