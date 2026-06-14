# Lenso Examples

Runnable examples for Lenso module authors and API consumers.

This repository uses published packages instead of sibling workspace paths:

- `@lenso/remote-module-kit`
- `@lenso/ts-sdk`

## Hello Action Remote Module

Install dependencies and run the smoke test:

```sh
pnpm install
pnpm smoke
```

Start the module server:

```sh
pnpm start:hello-action
```

The server prints a manifest URL like:

```text
http://127.0.0.1:4100/lenso/module/v1/manifest
```

Use that URL with a local Lenso checkout:

```sh
lenso module add http://127.0.0.1:4100/lenso/module/v1/manifest
lenso console-package apply-plan
```

## Repositories

- Backend framework: https://github.com/LioRael/lenso
- Runtime Console and remote module kit: https://github.com/LioRael/lenso-runtime-console
