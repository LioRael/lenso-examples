# @lenso/knowledge-excerpt

This Bun Plugin is the knowledge-base reference App's business excerpt provider.
It exposes deterministic excerpt, enqueue, claim, complete, and inspect Agent
Tools. Its optional Jobs Capability binding queues durable work when present;
without Jobs, the same Plugin computes excerpts inline. The App owns note
storage and decides when a completed excerpt is persisted.

The package contains `src/plugin.ts`, its generated Jobs Capability projection,
and the root `bun.lock`. The package version is the npm artifact version;
`lenso.pluginId` and `lenso.releaseVersion` identify the logical Plugin Release.
An exact signed catalog distribution must name this package and version and
bind the SHA-256 digest of the packed `.tgz` bytes.

From this directory, run `bun install --frozen-lockfile --ignore-scripts` and
`bun run check`, then `npm pack --ignore-scripts`. The package defines no
install, postinstall, prepare, or other lifecycle script. A consumer must
verify the signed catalog and archive digest before extraction, use the bundled
lock for its isolated Bun dependency install with scripts disabled, and let its
Host select and admit the Bun implementation before App resolution. The
signature and package alone do not grant installation or execution authority.

A locally packed tarball is only a candidate. npm publication, catalog signing,
and product adoption require separate approval and verification.
