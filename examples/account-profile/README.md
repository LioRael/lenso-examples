# Account Profile Service

This service keeps product profile data outside Lenso's auth module. The
service provider is `account-profile-service`; it provides the
`account-profile` module, which depends on `auth` and owns profiles,
organizations, and memberships.

Run it from the repository root:

```sh
pnpm start:account-profile
```

Smoke the service directly:

```sh
pnpm smoke:account-profile
```

Install its manifest into a local Lenso host:

```sh
lenso service install http://127.0.0.1:4120/lenso/service/v1/manifest
```
