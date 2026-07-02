# Support Desk Composer Fixture

Generated with:

```sh
lenso app compose ./acme-support --blueprint support-desk --addon support-sla --addon customer-profile --apply
lenso dev doctor --write-state
lenso app verify --write-proof
lenso app compose --repo-root . --addon support-sla --addon customer-profile --write-plan
lenso agent task --from-app-plan "add enterprise SLA escalation"
```

