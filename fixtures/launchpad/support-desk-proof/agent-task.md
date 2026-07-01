# Lenso Agent Context

## Launchpad

- Project: support-desk
- Blueprint: support-desk
- Status: configured
- Summary: Support desk app with one TypeScript API service and one Rust worker service.
- Next command: lenso dev up

## Addons

- support-sla (configured) services: support-sla

## Services

- support-api (ts) in `services/support-api` with `pnpm start`
- notification-worker (rust) in `services/notification-worker` with `cargo run`
- support-sla (ts) in `services/support-sla` with `pnpm start`

## Modules

- support-api owned by support-api for support.tickets.read
- notification-worker owned by notification-worker for support.notifications.send
- support-sla owned by support-sla for support.sla.read

## Boundaries

- Host owns auth, runtime queues, retries, outbox, Runtime Story, and Technical Operations.
- Services are remote processes that expose service manifests, routes, runtime functions, event handlers, and admin actions.
- Modules live inside services or the host; generated Launchpad JSON is control-plane state, not a hand-authored module contract.

## Service System

```json
{
  "dependencies": [
    {
      "capability": "support.tickets.read",
      "from": "notification-worker",
      "to": "support-api"
    },
    {
      "capability": "support.tickets.read",
      "from": "support-sla",
      "to": "support-api"
    }
  ],
  "environments": [
    "local"
  ],
  "modules": [
    {
      "capabilities": [
        "auth"
      ],
      "installTo": "host",
      "name": "auth"
    },
    {
      "capabilities": [
        "support.tickets.read",
        "support.tickets.write"
      ],
      "dependencies": [
        "auth"
      ],
      "installTo": "service:support-api",
      "name": "support-api"
    },
    {
      "capabilities": [
        "support.notifications.send"
      ],
      "dependencies": [
        "support.tickets.read"
      ],
      "installTo": "service:notification-worker",
      "name": "notification-worker"
    },
    {
      "capabilities": [
        "support.sla.read",
        "support.sla.write"
      ],
      "dependencies": [
        "support.tickets.read"
      ],
      "installTo": "service:support-sla",
      "name": "support-sla"
    }
  ],
  "name": "support-desk",
  "protocol": "lenso.system.v1",
  "services": [
    {
      "command": "pnpm start",
      "cwd": "services/support-api",
      "lang": "ts",
      "manifest": "http://127.0.0.1:4110/lenso/service/v1/manifest",
      "modules": [
        "support-api"
      ],
      "name": "support-api",
      "readyUrl": "http://127.0.0.1:4110/lenso/service/v1/status",
      "target": "local"
    },
    {
      "command": "cargo run",
      "cwd": "services/notification-worker",
      "lang": "rust",
      "manifest": "http://127.0.0.1:4120/lenso/service/v1/manifest",
      "modules": [
        "notification-worker"
      ],
      "name": "notification-worker",
      "readyUrl": "http://127.0.0.1:4120/lenso/service/v1/status",
      "target": "local"
    },
    {
      "command": "pnpm start",
      "cwd": "services/support-sla",
      "lang": "ts",
      "manifest": "http://127.0.0.1:4150/lenso/service/v1/manifest",
      "modules": [
        "support-sla"
      ],
      "name": "support-sla",
      "readyUrl": "http://127.0.0.1:4150/lenso/service/v1/status",
      "target": "local"
    }
  ]
}
```

## Service Workspace

```json
{
  "protocol": "lenso.service-workspace.v1",
  "services": [
    {
      "autoStart": true,
      "command": "cargo run",
      "cwd": "services/notification-worker",
      "lang": "rust",
      "manifest": "lenso.service.json",
      "modules": [
        "notification-worker"
      ],
      "name": "notification-worker",
      "readyTimeoutMs": 10000,
      "readyUrl": "http://127.0.0.1:4120/lenso/service/v1/status"
    },
    {
      "autoStart": true,
      "command": "pnpm start",
      "cwd": "services/support-api",
      "lang": "ts",
      "manifest": "lenso.service.json",
      "modules": [
        "support-api"
      ],
      "name": "support-api",
      "readyTimeoutMs": 10000,
      "readyUrl": "http://127.0.0.1:4110/lenso/service/v1/status"
    },
    {
      "autoStart": true,
      "command": "pnpm start",
      "cwd": "services/support-sla",
      "lang": "ts",
      "manifest": "lenso.service.json",
      "modules": [
        "support-sla"
      ],
      "name": "support-sla",
      "readyTimeoutMs": 10000,
      "readyUrl": "http://127.0.0.1:4150/lenso/service/v1/status"
    }
  ]
}
```

## Dev Doctor

- Status: ready
- Checks: 12
- Needs attention: 0

## App Proof

- Status: ready
- Drifts: 0
- Generated control-plane files may be repaired.
- Existing service source files are user code.
- Unknown services should not be deleted.

## Task

add enterprise SLA escalation
