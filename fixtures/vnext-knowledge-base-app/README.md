# Knowledge base App

This is the first runnable slice of the Lenso knowledge base reference App. It
starts from an ordinary source App: the App owns one linked Rust Plugin under
`project/app/`, while Engine derives the Host Catalog, bindings, and immutable
Plan.

The slice intentionally proves one complete user action before adding Auth,
PostgreSQL, background processing, and dynamic configuration. It supports:

- a browser page at `/`;
- `POST /notes` to create a note; and
- `GET /notes/{note_id}` to read it back.

No App-owned Host, Plan, Runtime Profile, or binding document is present.

```sh
lenso app dev --root project
lenso app build --root project --out dist
lenso app start --from dist
python3 verify.py --cli /absolute/path/to/lenso
```

`verify.py` copies the source into a clean temporary directory, builds the
distribution, removes all source, and proves the create/read path through the
real HTTP listener. This is the consumer proof for the slice, not a second App
definition.
