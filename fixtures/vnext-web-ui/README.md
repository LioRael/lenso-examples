# Target-owned Web UI fixture

This fixture is the runnable proof for a target-owned Lenso Web UI. It keeps
the framework-independent contracts in released Lenso packages while the
executable Web product remains in `lenso-examples`.

## Ownership

- The App Composition selects the business Module, its UI Contribution, the
  Web Shell, the Browser Adapter, Auth, and the worker before boot.
- The Web Shell owns routes, navigation, contribution metadata, and asset
  assembly.
- The Browser Adapter owns loopback HTTP transport and projects only the
  explicitly bound `example.secure-greeting@1` Capability to the generated
  browser client.
- Auth owns credential authentication. The orders Module owns business
  authorization. Neither concern is implemented by the HTTP ingress.
- The portable Kernel owns lifecycle and invocation semantics only. It has no
  dependency on Axum, Tower, sockets, HTTP, browser APIs, or product policy.

The ingress boundary is intentionally one small internal interface:

```text
HTTP request -> IngressRequest -> Browser Adapter dispatch -> IngressResponse
```

Axum and Tower are replaceable implementation details behind that boundary.
The current host applies a 16 KiB request-body limit, a 16 KiB request-head
limit, 32-request concurrency backpressure, request IDs, sensitive credential
marking, `X-Content-Type-Options: nosniff`, and graceful shutdown bridged from
the Lenso cancellation token.

CORS, compression, rate limiting, and an additional HTTP timeout are omitted
from this loopback, same-origin proof. A network-facing product should add
them only after defining its origins, proxy boundary, traffic budget, and
latency policy.

## Verify

From the repository root:

```sh
cargo test --locked -p lenso-vnext-web-ui --test web_ui
cargo test --locked -p lenso-vnext-web-ui --test web_ui \
  generated_browser_client_invokes_the_allowlisted_app_capability \
  -- --ignored --test-threads=1
```

The second command requires Bun and executes the generated browser client
against the running App.
