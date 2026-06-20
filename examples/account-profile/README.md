# Account Profile Remote Module

This example keeps product profile data outside Lenso's auth module. It depends
on `auth`, then stores profiles, organizations, and memberships in its own
module-owned records.

Run it from the repository root:

```sh
pnpm start:account-profile
```

Smoke the module directly:

```sh
pnpm smoke:account-profile
```

Install its manifest into a local Lenso host:

```sh
lenso module install http://127.0.0.1:4120/lenso/module/v1/manifest
```
