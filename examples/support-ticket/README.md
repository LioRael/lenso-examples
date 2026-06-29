# Support Ticket Service Suite

This example is the agent-ready service-suite proof for Lenso:
`support-ticket` is the business module, and `support-suite-provider` is the
provider service process that also exposes `support-notification` and
`support-knowledge-base`.

It exposes:

- a service manifest at `/lenso/service/v1/manifest`;
- service status at `/lenso/service/v1/status`;
- `support-ticket` under `/lenso/service/v1/modules/support-ticket`;
- `support-notification` under `/lenso/service/v1/modules/support-notification`;
- `support-knowledge-base` under `/lenso/service/v1/modules/support-knowledge-base`;
- ticket HTTP routes, `support-ticket.escalate-ticket.v1`, the `tickets`
  admin surface, and `assign_ticket`;
- `support-notification.send-ticket-update.v1`;
- `GET /articles/{id}` for knowledge-base articles.

Run it from the repository root:

```sh
pnpm --filter @lenso/example-support-ticket start
```

Run the smoke:

```sh
pnpm --filter @lenso/example-support-ticket smoke
pnpm --filter @lenso/example-support-ticket service:verify
```

Package the running service manifest for handoff. This writes the service
package, one module contract, and one module release artifact per provided
module:

```sh
pnpm service-package:support-ticket
```

Install the package artifact:

```sh
lenso service install dist/lenso-service/support-suite-provider/lenso.service-package.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
```

Install one business module through the generated module release artifact:

```sh
lenso module release inspect dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json
lenso module install dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
```

Or add the release artifact to a local Lenso host catalog, then install by
module name:

```sh
lenso module catalog add dist/lenso-service/support-suite-provider/modules/support-ticket/lenso.module-release.json \
  --base-url http://127.0.0.1:4110/lenso/service/v1
lenso module install support-ticket
lenso service verify support-suite-provider
lenso service doctor support-suite-provider --json
```

After the provider is installed, generate an environment-aware release plan
before applying a new package candidate:

```sh
lenso service env add staging \
  --service support-suite-provider \
  --target kubernetes \
  --namespace lenso-staging \
  --image ghcr.io/acme/support-suite-provider:0.4.0 \
  --public-base-url https://support-staging.example.com

lenso service release plan support-suite-provider \
  dist/lenso-service/support-suite-provider/lenso.service-package.json \
  --env staging \
  --output .lenso/support-suite-provider.staging.release-plan.json
lenso service policy check .lenso/support-suite-provider.staging.release-plan.json --fail-on breaking
lenso service deploy export support-suite-provider \
  --env staging \
  --target kubernetes \
  --output-dir examples/support-ticket/kubernetes/staging
lenso service release apply .lenso/support-suite-provider.staging.release-plan.json --env staging
```

`release plan` compares the installed manifest snapshot with the candidate
service package, `policy check` turns removed modules/capabilities/operations
and required env/config into an operator risk, and `apply` records the result in
`.lenso/service-releases.json` for Console Services. `deploy export` writes
reviewable Kubernetes manifests; apply them with `kubectl apply -k` when you are
ready to run the provider in a cluster.

`examples/support-ticket/lenso.module-release.json` is kept as a local dev
shortcut that points at the running provider manifest. V11 also keeps
`examples/support-ticket/lenso.module.json` as the standalone module contract:
it names the business capability before provider/release concerns. Sibling
module release shortcuts show that the same provider can be installed once
while modules remain independent install targets:

```sh
lenso module install examples/support-ticket/support-notification.module-release.json
lenso module install examples/support-ticket/support-knowledge-base.module-release.json
```

## Kubernetes Operator Path

Use the Lenso Operator when Kubernetes should continuously reconcile the
provider process from a `LensoServiceProvider` custom resource:

```sh
lenso operator export-crd --output dist/lenso-operator/crds
kubectl apply -k dist/lenso-operator/crds

lenso service env add staging \
  --service support-suite-provider \
  --target operator \
  --namespace lenso-staging \
  --image ghcr.io/lenso-dev/support-suite-provider:0.4.0 \
  --public-base-url https://support-staging.example.com \
  --manifest-reference https://support-staging.example.com/lenso/service/v1/manifest \
  --config port=4110 \
  --config replicas=2 \
  --config ingressHost=support-staging.example.com \
  --config autoscaling=true \
  --config disruptionBudget=true \
  --config networkPolicy=true

lenso service deploy export support-suite-provider \
  --env staging \
  --target operator \
  --output-dir dist/lenso-service/support-suite-provider/operator/staging

kubectl apply -k dist/lenso-service/support-suite-provider/operator/staging

lenso service deploy status support-suite-provider \
  --env staging \
  --source operator \
  --write-state

lenso service deploy wait support-suite-provider \
  --env staging \
  --source operator \
  --write-state
```

The Host still reads local Lenso state and runtime evidence. It does not need
kubeconfig.

Restart the API and worker after install. Open `/console` and check Modules:

- `support-ticket` is the installed business module;
- `support-suite-provider` is the configured service provider;
- `support-notification` and `support-knowledge-base` appear as sibling modules
  from the same provider;
- service status, compatibility, deployment, and health history are visible in
  Operations;
- the `tickets` data surface and `assign_ticket` action are available.

The host still owns Runtime Story, Remote Calls, queue, outbox, and retry
evidence after the modules are used through the host. Console shows
`support-ticket` as the installed business module.

If service verify or doctor reports `restart_pending`, restart the host. If it
reports `manifest_unreachable` or `service_not_ready`, start the provider again
and recheck:

```sh
pnpm --filter @lenso/example-support-ticket start
lenso service verify support-suite-provider
lenso service doctor support-suite-provider --json
```
