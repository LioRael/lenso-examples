# Local knowledge-excerpt 0.1.0 → 0.1.1 candidate upgrade

This is an unpublished, local-candidate recipe. `project/app/excerpt/` and its
0.1.0 archive remain intact. The separate 0.1.1 package changes only excerpt
truncation at a grapheme boundary. Both releases keep the same five Tool names,
schemas, optional Jobs binding, idempotency key, queue, and job payload. The
App-owned notes/settings tables and Jobs-owned durable queue are not migrated
or reset by the package.

## Prepare exact local inputs

In a bounded, network-disabled build sandbox with Node/npm already available,
pack `knowledge-excerpt-0.1.1/` using `npm pack --ignore-scripts --offline`.
The `files` allowlist must yield `src/plugin.ts`, `src/excerpt.ts`, the generated
Jobs projection, `bun.lock`, `tsconfig.json`, README, LICENSE, and manifest.
Place the `.tgz` in a disposable output directory; do not run lifecycle scripts.

Sign both archives with one fresh *ephemeral* Ed25519 key:

```sh
node sign-local-upgrade.mjs \
  ../project/app/excerpt/dist/lenso-knowledge-excerpt-0.1.0.tgz \
  knowledge-excerpt-0.1.1/dist/lenso-knowledge-excerpt-0.1.1.tgz \
  NEW_EXAMPLES_COMMIT_SHA \
  knowledge-excerpt-0.1.1/dist
```

Run the command from this `package-candidates/` directory. The script refuses
any 0.1.0 archive other than the original candidate SHA-256, checks both
archive manifests, and creates `package-snapshot-r1.json`,
`package-snapshot-r2.json`, and `trust.json`. Revision 2 retains the immutable
0.1.0 release and adds 0.1.1. Both snapshots expire six days after signing.
The private key is discarded; these files are **not** official catalog or npm
publication evidence. A new signing run requires a fresh output directory.

## Check exact replacement

Use a disposable source App that does **not** already discover
`project/app/excerpt/`; do not edit or delete the checked-in fixture. Set
`APP` to that App root, `CLI` to a build of the exact candidate CLI that
supports signed npm adoption, and `CANDIDATES` to this directory. Run all CLI
commands and dependency installation inside a bounded, offline OS sandbox.

```sh
"$CLI" app add lenso.reference.knowledge-excerpt@0.1.0 \
  --root "$APP" --package-snapshot "$CANDIDATES/knowledge-excerpt-0.1.1/dist/package-snapshot-r1.json" \
  --trust "$CANDIDATES/knowledge-excerpt-0.1.1/dist/trust.json" \
  --tgz "$CANDIDATES/../project/app/excerpt/dist/lenso-knowledge-excerpt-0.1.0.tgz" \
  --no-install
"$CLI" app discover --root "$APP" --json
"$CLI" app add lenso.reference.knowledge-excerpt@0.1.1 --replace \
  --root "$APP" --package-snapshot "$CANDIDATES/knowledge-excerpt-0.1.1/dist/package-snapshot-r2.json" \
  --trust "$CANDIDATES/knowledge-excerpt-0.1.1/dist/trust.json" \
  --tgz "$CANDIDATES/knowledge-excerpt-0.1.1/dist/lenso-knowledge-excerpt-0.1.1.tgz" \
  --no-install
"$CLI" app discover --root "$APP" --json
```

After replacement, discovery must select exactly 0.1.1, `lenso.toml` must
select only its `vendor/lenso/npm/.../0.1.1` source, and the 0.1.0 vendor
directory must remain recoverable. The signed catalog checkpoint must advance
from revision 1 to 2. `--no-install` proves admission and source selection
only: it does not prove a build, readiness, or dependency provenance.

For a **runtime and data-preservation** acceptance, use a separate disposable
copy of the full knowledge App with its Auth/Jobs/Secrets inputs and a disposable
PostgreSQL database. Before replacement, record one owner's `/settings`, an
existing `/notes/{note_id}`, and a durable `/job-status/{job_id}` through the
public bearer-authenticated API. Adopt/build/start 0.1.0, then replace with
0.1.1 using the same signed inputs and database. Install only the pinned Bun
dependencies with scripts disabled, use an untrusted build rejection to inspect
the exact installed-content `--trust-adopted-build` digest for each build, and
require the new App to pass readiness before routing traffic. Re-read those
three records after restart:
settings revision/value, note body/excerpt, and job identity/status must be
unchanged. Process a pre-upgrade queued job once and create a new note whose
body contains a combining character or joined emoji; its new excerpt must
end at a complete grapheme. Do not rerun schema operators against existing
tables or reset the database for this check.

The optional `verify.py` path automates that focused check with the same
temporary App and database across both builds. Supply the ordinary source
Auth/Jobs/Secrets inputs plus all five excerpt inputs:

```sh
LENSO_REFERENCE_BUN_CACHE="$SCRATCH_VERIFIED_BUN_CACHE" \
LENSO_REFERENCE_DATABASE_URL="$DISPOSABLE_DB_URL" python3 verify.py \
  --cli "$CLI" \
  --auth-source "$AUTH_SOURCE" --jobs-source "$JOBS_SOURCE" \
  --secrets-source "$SECRETS_SOURCE" \
  --excerpt-snapshot-r1 "$CANDIDATES/knowledge-excerpt-0.1.1/dist/package-snapshot-r1.json" \
  --excerpt-snapshot-r2 "$CANDIDATES/knowledge-excerpt-0.1.1/dist/package-snapshot-r2.json" \
  --excerpt-trust "$CANDIDATES/knowledge-excerpt-0.1.1/dist/trust.json" \
  --excerpt-tgz-r1 "$CANDIDATES/../project/app/excerpt/dist/lenso-knowledge-excerpt-0.1.0.tgz" \
  --excerpt-tgz-r2 "$CANDIDATES/knowledge-excerpt-0.1.1/dist/lenso-knowledge-excerpt-0.1.1.tgz"
```

Run this only in an offline OS sandbox with read-only inputs and bounded
resources. Seed `SCRATCH_VERIFIED_BUN_CACHE` only with locally verified locked
packages; do not inherit the host home or allow network access. The opt-in path
omits the fixture's local excerpt source, adopts both signed npm versions with
`--no-install`, installs frozen dependencies without lifecycle scripts using
`--offline`, then requires an untrusted build to fail closed and print the
exact installed-content grant before each authorized build. It does
not exercise the separate `--package-only` Auth/Jobs/Secrets provider path.

This full runtime/data step still requires a successful execution with a
coherent current CLI and provider dependency closure; packing and signed
adoption alone do not establish it. Neither local success nor the registry URL
in a snapshot means that 0.1.1 is available on npm.
