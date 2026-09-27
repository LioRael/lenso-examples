# Knowledge-base Agent input (local package candidate)

Use this with a copy of this reference fixture and a separately staged,
version-matched package set. It is suitable for an independent Agent to read;
it contains no machine-specific checkout path, signing key, database password,
or token. It is **not** an official Directory listing or evidence that an Agent
has completed the task. The known local Host result used the business source
at Examples `ceeb9a33c7b00ecf36cd382fbc673954bf570e9f` and a Linux ARM64
`lenso` CLI built from Rust `c3e1c52ef0cef395216e5cc3bfe6c87a77f500b2`.

For a request such as “Build a two-user knowledge base with login, uploaded
notes, durable background excerpts, and a React page”, choose the following
exact candidates. Do not substitute a similarly named Plugin or infer a
newer version from a package README.

| Choice | Exact Release / input SHA-256 | Why it is needed and what it may access |
| --- | --- | --- |
| Auth | `lenso.auth.api-token@0.1.2` `.crate` `d3b0c6b8ab5065abbe468eb3bc44b1b6e5dcda0c91d9e72250c1644c37b9becf` | Authenticates bearer tokens and owns its PostgreSQL schema; needs only its database URL, signing-key, and pepper secret references. |
| Jobs | `lenso.jobs@0.1.8` `.crate` `8d9a10433ab2d5ff623e2e7ced9491ed6b4b37787e385710b39e725d7ddccab5` | Owns durable queue/lease/retry state in PostgreSQL; allow only the `knowledge` queue and the exact Excerpt producer, worker, and observer Instance. |
| Secrets Env | `lenso.secrets.env@0.1.7` `.crate` `6dd6a27548c53acaa56d5b65faa6aab3c755e1fb5b00365c406b47aed79c4787` | Resolves the five explicit Auth/Jobs/knowledge environment references shown in `verify.py`; it is not permission to read all environment variables. |
| Excerpt | `lenso.reference.knowledge-excerpt@0.1.2` / `@lenso/knowledge-excerpt@0.1.2` `.tgz` `0e07ecfff39dee892e11119765ff09dd9af4ee8774e1dde89e275436efbf901e` | Deterministic text Tools with an optional Jobs binding; it does not call a model. Binding it to the App-owned business Plugin does not expose every Tool to an Agent. |

The App-owned `lenso.reference.knowledge-base` linked Rust Plugin implements
HTTP routes, per-user note storage, and the PostgreSQL schema. Its dependency
closure pins `lenso@0.5.27` and
`lenso-capability-agent-tool-provider@0.3.0`; a matching offline Cargo source
must include those packages. The checked-in React production assets are part
of this App source. `lenso.web-ingress@0.4.9` is Host-provided: do not run
generic `app add` on it or reclassify it as a portable Plugin.

## Before allowing a build

An operator must stage the exact archives, a **currently valid** signed
linked-Cargo snapshot and independent public trust file, and a valid signed
Excerpt package snapshot/trust. Both snapshots in the prior local Host gate
were **test-only**, not official Directory publication; its original linked
snapshot has since expired. Do not extend an expiry, reuse a signature for
changed bytes, or give the Agent a signing key. If the operator has not
provided a fresh matching linked snapshot/trust, stop before `app add`.

The operator also supplies a compatible Linux ARM64 CLI (the previously
verified candidate SHA-256 is
`70c988d145b9d4a81b4f2bb1da64af3b1c57aee38fd189c2bdacd7efa663b309`),
Rust toolchain, Bun, immutable offline Cargo vendor sources, an offline Bun
cache, and a disposable PostgreSQL database. Build and run in a network-off,
resource-limited OS sandbox with read-only original inputs and a writable
scratch App. The launcher injects `CARGO_HOME` with a credential-free offline
vendor config, `LENSO_REFERENCE_BUN_CACHE`, and
`LENSO_REFERENCE_DATABASE_URL` without printing their values. No framework,
Auth, Jobs, or Secrets source checkout is an input. Review exact build grants
and package scripts before executing adopted code; a signature is not a
sandbox or blanket execution approval.

## Candidate path

In a disposable copy of this fixture, let `INPUTS_DIR` be an absolute path to
the operator-staged, read-only inputs. Keep your business changes only in the
copy's `project/app/notes-web` or `project/frontend`; do not edit Engine,
generated Host/Plan files, or the signed provider archives. First inspect the
four choices and the required secret/queue/Tool bindings above. If a user
request needs broader authority, ask rather than silently expanding the
binding, changing an Agent tool allowlist, or editing `AGENTS.md`.

The following command adopts all four exact packages, prepares the three
provider configurations and business schema in a disposable database, builds
and checks the distribution, deletes its source copy, then observes a real
Host-owned Jobs completion and user-B 404. It does not run a browser or claim
that the Agent authored new business code. The filenames are a staging
contract, not private filesystem locations:

```sh
python3 verify.py --background-only --package-only \
  --cli "$INPUTS_DIR/bin/lenso" --trust-linked-build-from-crates \
  --linked-snapshot "$INPUTS_DIR/catalog/linked-snapshot.json" \
  --trust "$INPUTS_DIR/catalog/linked-trust.json" \
  --auth-version 0.1.2 \
  --auth-crate "$INPUTS_DIR/packages/lenso-auth-api-token-plugin-0.1.2.crate" \
  --jobs-version 0.1.8 \
  --jobs-crate "$INPUTS_DIR/packages/lenso-jobs-plugin-0.1.8.crate" \
  --secrets-version 0.1.7 \
  --secrets-crate "$INPUTS_DIR/packages/lenso-secrets-env-plugin-0.1.7.crate" \
  --excerpt-version 0.1.2 \
  --excerpt-snapshot "$INPUTS_DIR/catalog/excerpt-snapshot.json" \
  --excerpt-trust "$INPUTS_DIR/catalog/excerpt-trust.json" \
  --excerpt-tgz "$INPUTS_DIR/packages/lenso-knowledge-excerpt-0.1.2.tgz"
```

Success requires the verifier's `PASS: Host-owned background processing and
user-scoped read-only polling` line. `app check` uses the distribution root;
`app show --json` uses its `intent` directory. The prior one-off local gate
passed with these exact archive hashes and a scratch-only version adapter;
the checked-in single-version option subsequently passed one offline local
gate with a fresh **test-only** signed catalog. An independent Agent selected
the four exact packages, made a scratch-only App-owned title-validation change,
invoked the fixed gate, and observed source-deleted Host/PostgreSQL background
completion and user-B 404. That scratch edit is not part of this repository;
its overlong-title rejection was not separately exercised at runtime. Neither
gate proves official publication, React browser behavior on the Agent edit,
upgrade/unadoption, or production deployment. Those are separate acceptance
steps, not implied by the command above.
