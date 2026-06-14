# Hello Action Remote Module

This is the smallest remote module example for Lenso. It exposes:

- a manifest at `/lenso/module/v1/manifest`;
- one HTTP route, `GET /hello/{name}`;
- one runtime function, `hello-action.say-hello.v1`;
- one schema-admin entity, `greetings`.

Run it from the repository root:

```sh
pnpm start:hello-action
```

Run its non-interactive smoke:

```sh
pnpm smoke
```
