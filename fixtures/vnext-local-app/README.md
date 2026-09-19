# Local App: native Web, Bun tools, and shared Rust Plugin

This fixture exercises the convention-based App workflow from
[Lenso #727](https://github.com/LioRael/lenso/issues/727). Use a CLI built from the
local App implementation branch/main after its merge; an older registry CLI does
not contain these commands. Build dependencies are Cargo and Bun. The executable
profile supports macOS ARM64 and Linux x86_64.

```sh
cd fixtures/vnext-local-app
bun install --frozen-lockfile --cwd project/app/uppercase
lenso app dev --root project
```

Open the printed URL. The Web Plugin owns the HTML and HTTP routes. Its named
`uppercase` dependency calls a Bun Tool Provider. The shared `example.audit`
Plugin activates only because `project/plugins/example.audit/default.toml`
explicitly adopts it. The optional `project/lenso.toml` points to the local
`shared-plugins/` directory; it is not a marketplace or a second App manifest.
Remove that source configuration and its Root adoption together to use only
App-owned Plugins. No Host declaration, preset, or activation flag is required.

```sh
lenso app build --root project --out dist
lenso app start --from dist
# Repeatable build + offline end-to-end verification:
python3 verify.py --cli /absolute/path/to/lenso
```

The verifier copies the fixture into a temporary workspace, installs locked Bun
dependencies, builds the distribution, deletes the copied sources, clears toolchain
PATH, and checks the HTML and real HTTP -> Rust -> Bun invocation. It also checks
shared activation and graceful shutdown. The Python script is test tooling, not a
Python Plugin or a claimed Python SDK.

Rust dependencies use released versions. `app/web` owns business routes and its
static resource; `app/uppercase` owns the tool; `shared-plugins/audit` owns its
lifecycle behavior. Host generation, discovery, and adapters stay in the CLI and
runtime repositories. This fixture introduces no product-specific Host mechanics.
