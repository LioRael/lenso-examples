# gRPC Notes Service Module

This is the smallest native gRPC service module example for Lenso. It exposes:

- `GetManifest` for the module manifest;
- schema-admin list/detail reads for `notes`;
- one host-owned proxy route, `GET /notes`;
- one runtime function, `grpc-notes.summarize.v1`.

## Run

Install dependencies from the repository root:

```sh
pnpm install
```

Start the gRPC module:

```sh
pnpm start:grpc-notes
```

The default endpoint is:

```text
grpc://127.0.0.1:50051
```

Run the smoke test:

```sh
pnpm smoke:grpc-notes
```

## Install Into A Lenso Host

Use the checked-in manifest file as the install-time manifest reference, and
pass the running gRPC endpoint as the remote base URL:

```sh
lenso module catalog add ../lenso-examples/examples/grpc-notes/lenso.module.json --base-url grpc://127.0.0.1:50051 --summary "Native gRPC notes module"
lenso module add ../lenso-examples/examples/grpc-notes/lenso.module.json --base-url grpc://127.0.0.1:50051
lenso console package apply-plan
```

This example has no Runtime Console package, so `apply-plan` should not install
frontend dependencies. Restart the host after `REMOTE_MODULES` changes.
