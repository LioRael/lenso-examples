# @lenso/knowledge-excerpt 0.1.1 candidate

This is a second, local-only npm package candidate for the knowledge-base
reference App. Version 0.1.0 remains unchanged under `project/app/excerpt/`.

The five Agent Tools, optional Jobs Capability, request and result shapes,
idempotency key, queue, and job payload are unchanged. Version 0.1.1 counts
Unicode grapheme clusters when truncating excerpts, so combining characters
and joined emoji are not split. A new note can therefore have a different
excerpt than it would under 0.1.0; existing stored excerpts are not rewritten.
The App still owns notes and per-user settings, while the Jobs provider owns
durable job state. This package has no migration or database write path.

The package's `lenso` manifest identifies the same logical Plugin ID at
release 0.1.1. A signed local catalog must bind the exact `.tgz` SHA-256 digest.
Use the adjacent `UPGRADE.md` for the local v1-to-v2 signing and replacement
recipe. Neither this candidate nor its `publishConfig` indicates that it has
been published to npm or admitted by a production catalog.
