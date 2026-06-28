# Rust Service Provider Example

This example is a real out-of-process Lenso service provider written in Rust
with Axum. It exposes the `rust-audit-log` module from a standalone backend
process.

It serves:

- `GET /lenso/service/v1/manifest`
- `GET /lenso/service/v1/status`
- `GET /lenso/service/v1/ready`
- `GET /lenso/service/v1/modules/rust-audit-log/manifest`
- `GET /lenso/service/v1/modules/rust-audit-log/events`
- `POST /lenso/service/v1/modules/rust-audit-log/runtime/functions/rust-audit-log.summarize-events.v1/invoke`

The manifest declares service operation metadata for the `/events` HTTP route
and the `rust-audit-log.summarize-events.v1` runtime function, so V8 service
checks can prove both HTTP probing and runtime operation discovery.

Run it from the repository root:

```sh
pnpm start:rust-service
```

The server prints a manifest URL like:

```text
http://127.0.0.1:4130/lenso/service/v1/manifest
```

Install that manifest into a local host:

```sh
lenso service install http://127.0.0.1:4130/lenso/service/v1/manifest
```

After install, the Host records `rust-audit-service` as the provider and
`rust-audit-log` as the provided module. Open Runtime Console `/services` to
see provider state, health, compatibility, Remote Calls, Runtime Story, and
Technical Operations links.

Check the service provider contract and direct HTTP route probe:

```sh
lenso service check http://127.0.0.1:4130/lenso/service/v1/manifest \
  --serve-command "PORT=4130 cargo run --manifest-path examples/rust-service/Cargo.toml" \
  --ready-timeout-ms 30000
```

Compare an installed provider with the running Rust manifest:

```sh
lenso service diff rust-audit-service http://127.0.0.1:4130/lenso/service/v1/manifest
```

Preview an upgrade from the running manifest:

```sh
lenso service upgrade rust-audit-service http://127.0.0.1:4130/lenso/service/v1/manifest --dry-run
```

Export deployment fragments for review:

```sh
lenso service export --module rust-audit-log --format compose
lenso service export --module rust-audit-log --format systemd
lenso service export --module rust-audit-log --format dockerfile
lenso service export --module rust-audit-log --format env
```

If a real upgrade has already saved a previous service snapshot, preview the
rollback path:

```sh
lenso service rollback rust-audit-service --dry-run
```

Print the manifest without starting the server:

```sh
pnpm rust-service:check
```

With the service running, package its manifest for handoff:

```sh
pnpm service-package:rust-service
```
