# Typed local App authoring

This fixture extends the local App workflow with one source-owned domain
Capability. Use a CLI build containing the authoring DX implementation; registry
installation is a separate delivery gate.

```sh
bun install --cwd project/app/uppercase --frozen-lockfile
lenso app dev --root project
```

Open the printed URL. The Web Plugin calls `TextClient.uppercase(request)` and the
Bun Plugin implements `provides: [Text]`. The request/response are typed throughout;
neither side manually encodes JSON or selects an Agent tool by name. Shared audit
adoption remains explicit through ordinary Plugin Root intent.

The contract lives in `project/contracts/text/src/contract.rs`. The normal
`app dev/build` command extracts it before compiling its consumers, regenerates
the Rust runtime and Bun TypeScript projections, and checks compatibility. The
ordinary Cargo build script independently checks generated freshness. Package
metadata locates these artifacts; there is no handwritten Host or required App
configuration. The optional `lenso.toml` only adds the shared local source.

To create a similar contract package in another App:

```sh
lenso app contract new example.text --source rust
# A Descriptor/JSON Schema source with TypeScript output, without Cargo:
lenso app contract new example.other
```

`@lenso/bun-plugin` 0.4.1 and codegen 0.9 provide the typed declaration API.
Native SDK consumers still use a namespace-qualified Client and explicit
requirement name. Language, business contract and execution class stay separate.

```sh
lenso app build --root project --out dist
lenso app start --from dist
python3 verify.py --cli /absolute/path/to/lenso
```

The verifier deletes generated artifacts in a temporary copy, builds without
manual codegen, explicitly advances the contract version and checks that both
language projections update. It then introduces an incompatible change and
verifies rejection without output replacement. Finally it removes all copied
source and toolchain PATH and proves HTTP -> native Rust -> Bun, shared activation,
and graceful shutdown from the offline distribution. This script is test tooling;
it does not imply a Python SDK.

Compatible version edits update projections before compilation. Invalid contract
or code edits retain the running development App. Changes remain reviewable;
release compatibility must still be compared against the published contract.
The local compatibility cache is not a substitute for that release gate.
