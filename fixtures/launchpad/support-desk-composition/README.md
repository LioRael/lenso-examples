# Exact Support Desk App Composition Fixture

This fixture is the materialized output of:

```sh
lenso app compose ./support-desk --blueprint support-desk --apply
lenso system dev --system-file ./support-desk/lenso.app.json --dry-run --json
lenso system dev --system-file ./support-desk/lenso.app.json
```

`lenso.app.json` is the only application-level composition and lock. It pins
the selected Module release digests, `linked` or stable Service Reference
bindings, resolved dependency selections, and the optimistic revision.
`lenso.workspace.json` remains local connection input for the replaceable Local
Control Adapter; its process commands are not copied into the App Composition.

The fixture intentionally has no `lenso.system.json`,
`lenso.system-sandbox.json`, or App Change Plan overlay. Stop adapter-owned
workloads with:

```sh
lenso system dev --system-file ./support-desk/lenso.app.json --cleanup
```
