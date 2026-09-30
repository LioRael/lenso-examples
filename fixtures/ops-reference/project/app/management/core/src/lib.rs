//! Optional source App wrapper. Every business and security call uses a bound owner port.
use example_ops_state_contract as state;
use futures::{FutureExt as _, future::LocalBoxFuture};
use human::ManagementHumanProvider as _;
use lenso_capability_access_control as access;
use lenso_capability_audit_log as audit;
use lenso_capability_business_approval as approval;
use lenso_capability_credential_state as credentials;
use lenso_capability_management as management;
use lenso_capability_management_human as human;
use lenso_kernel::{InvocationContext, NativeRequestFuture};
use lenso_management_authority::{
    EntryPolicy, HumanServiceProvider, OperatorsAuthority, OwnerPorts, ReadOnlyOwnerPorts,
};
use lenso_management_core::{
    Binding, Effect, Entry, Error, Management, Outcome, ServiceProvider, Target,
};
use management::ManagementProvider as _;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

pub mod host_facilities;
use host_facilities::AuthorityHandle;

#[derive(Clone, Debug, Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct Config {
    deployment: String,
    target_instance: String,
    #[serde(default)]
    read_only: bool,
}

fn validate(config: &Config) -> Result<(), lenso::RuntimeFailure> {
    if config.deployment.is_empty() || config.target_instance != "example.ops-state/primary" {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "Management selects the explicitly bound primary OpsState owner".into(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Assembly {
    management: Rc<Management>,
    authority: Rc<OperatorsAuthority>,
}

#[lenso::plugin(validate = validate)]
#[derive(Clone, Debug)]
struct OpsManagement {
    #[config]
    config: Config,
    #[facility(id = "authority")]
    host: AuthorityHandle,
    #[dependency(id = "state")]
    state: state::OpsStateClient,
    #[dependency(id = "credential_state")]
    credential_state: credentials::CredentialStateClient,
    #[dependency(id = "access")]
    access: access::AccessControlClient,
    #[dependency(id = "approval")]
    approval: Option<approval::BusinessApprovalClient>,
    #[dependency(id = "audit")]
    audit: audit::AuditLogClient,
    assembly: Rc<RefCell<Option<Assembly>>>,
}

#[lenso::plugin_impl]
impl OpsManagement {
    #[create]
    fn create(
        config: Config,
        host: AuthorityHandle,
        state: state::OpsStateClient,
        credential_state: credentials::CredentialStateClient,
        access: access::AccessControlClient,
        approval: Option<approval::BusinessApprovalClient>,
        audit: audit::AuditLogClient,
    ) -> Result<Self, String> {
        let owner = Self {
            config,
            host,
            state,
            credential_state,
            access,
            approval,
            audit,
            assembly: Rc::new(RefCell::new(None)),
        };
        owner.initialize().map_err(|error| match error {
            lenso::RuntimeFailure::InvalidResolvedPlan { detail }
            | lenso::RuntimeFailure::PluginFailure { detail } => detail,
            _ => "Management complete-object construction was rejected".into(),
        })?;
        Ok(owner)
    }
}
impl OpsManagement {
    fn initialize(&self) -> Result<(), lenso::RuntimeFailure> {
        if self.config.deployment != self.host.deployment || self.assembly.borrow().is_some() {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Management facility deployment differs or generation is already prepared"
                    .into(),
            });
        }
        if self.approval.is_some() == self.config.read_only {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "The write profile requires Approval; the explicit read-only profile excludes it".into(),
            });
        }
        let mut entries = catalog(&self.config.target_instance);
        if self.config.read_only {
            entries.retain(|entry| entry.effect == Effect::Read);
        }
        let mut policies = operation_policies();
        policies.retain(|id, _| entries.iter().any(|entry| &entry.id == id));
        let authority = Rc::new(
            if self.config.read_only {
                OperatorsAuthority::new_read_only(
                    self.host.verifier.clone(),
                    self.host.qualification.clone(),
                    policies,
                    ReadOnlyOwnerPorts {
                        credential_state: self.credential_state.clone(),
                        access: self.access.clone(),
                        audit: self.audit.clone(),
                    },
                    self.host.clock.clone(),
                )
            } else {
                OperatorsAuthority::new(
                    self.host.verifier.clone(),
                    self.host.qualification.clone(),
                    policies,
                    OwnerPorts {
                        credential_state: self.credential_state.clone(),
                        access: self.access.clone(),
                        approval: self.approval.clone().expect("validated write profile"),
                        audit: self.audit.clone(),
                    },
                    self.host.clock.clone(),
                )
            }
            .map_err(unavailable)?,
        );
        let bindings = entries
            .into_iter()
            .map(|entry| Binding {
                target: Rc::new(OpsTarget {
                    state: self.state.clone(),
                    write: entry.effect == Effect::Write,
                }),
                entry,
            })
            .collect();
        let management = Rc::new(
            Management::open(
                self.host.journal.as_path(),
                self.config.deployment.clone(),
                format!("ops-state.{}", state::DESCRIPTOR_VERSION),
                bindings,
                authority.clone(),
            )
            .map_err(unavailable)?,
        );
        *self.assembly.borrow_mut() = Some(Assembly {
            management,
            authority,
        });
        Ok(())
    }
}
fn unavailable(_: Error) -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {
        detail: "Management operator setup or owner binding is unavailable".into(),
    }
}

fn catalog(instance: &str) -> Vec<Entry> {
    [("state.read", "read", "Read the primary state", Effect::Read, false, json!({"type":"object","properties":{},"additionalProperties":false})),
     ("state.update", "update", "Change the primary state at the reviewed revision", Effect::Write, true, json!({"type":"object","properties":{"value":{"type":"integer","minimum":-9007199254740991i64,"maximum":9007199254740991i64}},"required":["value"],"additionalProperties":false}))]
        .into_iter().map(|(id, operation, description, effect, requires_approval, schema)| Entry {
            id: id.into(), target_instance: instance.into(), capability: state::CAPABILITY_ID.into(),
            version: state::DESCRIPTOR_VERSION.into(), operation: operation.into(), description: description.into(), effect, requires_approval,
            input_schema_json: schema.to_string().parse().expect("static bounded schema"),
        }).collect()
}

#[derive(Debug)]
struct OpsTarget {
    state: state::OpsStateClient,
    write: bool,
}
impl Target for OpsTarget {
    fn invoke<'a>(
        &'a self,
        context: InvocationContext,
        input: Value,
        expected_revision: Option<String>,
        operation_id: Option<String>,
    ) -> LocalBoxFuture<'a, Result<Outcome, Error>> {
        async move {
            if !self.write {
                let response = self
                    .state
                    .read_with_context(context, state::ReadRequest {})
                    .await
                    .map_err(|_| Error::Unavailable)?;
                return Ok(Outcome::Committed {
                    result: serde_json::to_value(response).map_err(|_| Error::Unavailable)?,
                    receipt: "read-only".into(),
                });
            }
            let request = state::UpdateRequest {
                value: input
                    .get("value")
                    .and_then(Value::as_i64)
                    .ok_or(Error::InvalidInput)?,
                expected_revision: expected_revision
                    .ok_or(Error::InvalidInput)?
                    .parse()
                    .map_err(|_| Error::InvalidInput)?,
                idempotency_key: operation_id.ok_or(Error::InvalidInput)?,
            };
            match self.state.update_with_context(context, request).await {
                Ok(response) => Ok(Outcome::Committed {
                    receipt: response.receipt_id.clone(),
                    result: serde_json::to_value(response).map_err(|_| Error::Unavailable)?,
                }),
                Err(state::OpsStateUpdateInvocationError::Domain(
                    state::UpdateError::InvalidInput
                    | state::UpdateError::StaleRevision
                    | state::UpdateError::IdempotencyConflict,
                )) => Ok(Outcome::Failed),
                Err(_) => Ok(Outcome::Unknown),
            }
        }
        .boxed_local()
    }
    fn receipt<'a>(
        &'a self,
        context: InvocationContext,
        operation_id: &'a str,
    ) -> LocalBoxFuture<'a, Result<Option<(Value, String)>, Error>> {
        async move {
            let response = self
                .state
                .receipt_with_context(
                    context,
                    state::ReceiptRequest {
                        idempotency_key: operation_id.into(),
                    },
                )
                .await
                .map_err(|_| Error::Unavailable)?;
            if !response.found {
                return Ok(None);
            }
            let receipt = response.receipt_id.flatten().ok_or(Error::Unavailable)?;
            let restored = state::UpdateResponse {
                label: response.label.flatten().ok_or(Error::Unavailable)?,
                value: response.value.flatten().ok_or(Error::Unavailable)?,
                revision: response.revision.flatten().ok_or(Error::Unavailable)?,
                receipt_id: receipt.clone(),
            };
            Ok(Some((
                serde_json::to_value(restored).map_err(|_| Error::Unavailable)?,
                receipt,
            )))
        }
        .boxed_local()
    }
}

#[lenso::provides(management::Management, human::ManagementHuman)]
impl OpsManagement {}
impl OpsManagement {
    fn service(&self) -> Option<ServiceProvider> {
        self.assembly
            .borrow()
            .as_ref()
            .map(|assembly| ServiceProvider(assembly.management.clone()))
    }
    fn human(&self) -> Option<HumanServiceProvider> {
        self.assembly
            .borrow()
            .as_ref()
            .map(|assembly| HumanServiceProvider {
                management: assembly.management.clone(),
                authority: assembly.authority.clone(),
            })
    }
    fn catalog(
        &self,
        context: InvocationContext,
        request: management::CatalogRequest,
    ) -> NativeRequestFuture<management::ManagementCatalog> {
        self.service().map_or_else(
            || {
                Box::pin(std::future::ready(Ok(Err(
                    management::CatalogError::Unavailable,
                )))) as NativeRequestFuture<management::ManagementCatalog>
            },
            |service| service.catalog(context, request),
        )
    }
    fn invoke(
        &self,
        context: InvocationContext,
        request: management::InvokeRequest,
    ) -> NativeRequestFuture<management::ManagementInvoke> {
        self.service().map_or_else(
            || {
                Box::pin(std::future::ready(Ok(Err(
                    management::InvokeError::Unavailable,
                )))) as NativeRequestFuture<management::ManagementInvoke>
            },
            |service| service.invoke(context, request),
        )
    }
    fn status(
        &self,
        context: InvocationContext,
        request: management::StatusRequest,
    ) -> NativeRequestFuture<management::ManagementStatus> {
        self.service().map_or_else(
            || {
                Box::pin(std::future::ready(Ok(Err(
                    management::StatusError::Unavailable,
                )))) as NativeRequestFuture<management::ManagementStatus>
            },
            |service| service.status(context, request),
        )
    }
    fn read_intent(
        &self,
        context: InvocationContext,
        request: human::ReadIntentRequest,
    ) -> NativeRequestFuture<human::ManagementHumanReadIntent> {
        self.human().map_or_else(
            || {
                Box::pin(std::future::ready(Ok(Err(
                    human::ReadIntentError::Unavailable,
                )))) as NativeRequestFuture<human::ManagementHumanReadIntent>
            },
            |service| service.read_intent(context, request),
        )
    }
    fn decide(
        &self,
        context: InvocationContext,
        request: human::DecideRequest,
    ) -> NativeRequestFuture<human::ManagementHumanDecide> {
        self.human().map_or_else(
            || {
                Box::pin(std::future::ready(Ok(Err(human::DecideError::Unavailable))))
                    as NativeRequestFuture<human::ManagementHumanDecide>
            },
            |service| service.decide(context, request),
        )
    }
}

/// Fixed business resource policies shared by the optional human token wrapper.
pub fn operation_policies() -> std::collections::BTreeMap<String, EntryPolicy> {
    ["read", "update"]
        .into_iter()
        .map(|operation| {
            (
                format!("state.{operation}"),
                EntryPolicy {
                    permission: format!("ops.state.{operation}"),
                    scope_kind: "ops-state".into(),
                    scope_id: "primary-state".into(),
                },
            )
        })
        .collect()
}
