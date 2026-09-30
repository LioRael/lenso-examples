use crate::{label, unavailable};
use lenso_access_control_d1_plugin as access_owner;
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    PluginInstancePlan,
};
use lenso_auth_api_token_plugin as auth_owner;
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_access_control as access;
use lenso_capability_access_control_admin as admin;
use lenso_capability_access_control_directory as directory;
use lenso_capability_api_token_admin as tokens;
use lenso_capability_auth as auth;
use lenso_capability_credential_state as credential;
use lenso_capability_secrets as secrets;
use lenso_kernel::{
    CancellationToken, InvocationContext, Kernel, NativeRequestFuture, RuntimeFailure,
};
use lenso_native_adapter::{
    NativePluginFactory, NativePluginFactoryContext, NativePluginInstance, NativePluginRegistry,
};
use lenso_workers_driver::WorkersDriver;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeMap, fmt, rc::Rc, time::Duration};
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

const CALLER: &str = "example.ops-workers-operator/default";
const CALLER_PACKAGE: &str = "example.ops-workers-operator";
const AUTH: &str = "lenso.auth.api-token/default";
const ACCESS: &str = "lenso.access-control.d1/default";
const SECRETS: &str = "example.ops-workers-operator-secrets/default";
const SECRETS_PACKAGE: &str = "example.ops-workers-operator-secrets";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Role {
    id: String,
    permissions: Vec<String>,
    subjects: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    kind: String,
    id: String,
    roles: Vec<Role>,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Bootstrap {
        subject: String,
        scopes: Vec<Scope>,
    },
    RevokeRole {
        kind: String,
        id: String,
        subject: String,
        role_id: String,
    },
    ListRoles {
        kind: String,
        id: String,
    },
}
fn validate(request: &Request) -> Result<(), JsValue> {
    let valid = match request {
        Request::Bootstrap { subject, scopes } => {
            label(subject)
                && !scopes.is_empty()
                && scopes.len() <= 16
                && scopes.iter().all(|scope| {
                    label(&scope.kind)
                        && label(&scope.id)
                        && scope.roles.len() <= 16
                        && scope.roles.iter().all(|role| {
                            label(&role.id)
                                && role.permissions.len() <= 64
                                && role.permissions.iter().all(|permission| label(permission))
                                && role.subjects.len() <= 32
                                && role.subjects.iter().all(|subject| label(subject))
                        })
                })
        }
        Request::RevokeRole {
            kind,
            id,
            subject,
            role_id,
        } => [kind, id, subject, role_id]
            .iter()
            .all(|value| label(value)),
        Request::ListRoles { kind, id } => label(kind) && label(id),
    };
    if valid { Ok(()) } else { Err(unavailable()) }
}
#[derive(Debug)]
struct Caller;
impl NativePluginFactory for Caller {
    fn package_id(&self) -> &'static str {
        CALLER_PACKAGE
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}
#[derive(Clone)]
struct BoundSecrets(Rc<BTreeMap<String, Zeroizing<String>>>);
impl fmt::Debug for BoundSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundOperatorSecrets")
            .finish_non_exhaustive()
    }
}
impl secrets::SecretsProvider for BoundSecrets {
    fn resolve(
        &self,
        context: InvocationContext,
        request: secrets::ResolveRequest,
    ) -> NativeRequestFuture<secrets::Secrets> {
        let result = (context.caller_instance() == Some(AUTH))
            .then(|| self.0.get(&request.reference))
            .flatten()
            .map(|value| secrets::ResolveResponse {
                value: value.to_string(),
            })
            .ok_or(secrets::ResolveError::UnknownReference);
        Box::pin(std::future::ready(Ok(result)))
    }
}
impl NativePluginFactory for BoundSecrets {
    fn package_id(&self) -> &'static str {
        SECRETS_PACKAGE
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        let endpoint = Rc::new(secrets::SecretsEndpoint::new(self.clone()))
            as Rc<dyn lenso_kernel::NativeRequestEndpoint>;
        Ok(NativePluginInstance::new(vec![endpoint]))
    }
}
struct Event(WorkersDriver);
impl Drop for Event {
    fn drop(&mut self) {
        self.0.request_shutdown();
    }
}

#[wasm_bindgen]
pub async fn setup_access(batch: js_sys::Function) -> Result<(), JsValue> {
    access_owner::schema::plan()
        .map_err(|_| unavailable())?
        .setup(&access_owner::workers::D1Binding(batch))
        .await
        .map_err(|_| unavailable())
}
#[wasm_bindgen]
pub async fn verify_access(batch: js_sys::Function) -> Result<(), JsValue> {
    access_owner::schema::plan()
        .map_err(|_| unavailable())?
        .verify(&access_owner::workers::D1Binding(batch))
        .await
        .map_err(|_| unavailable())
}

// This supplementary private Host seals the caller through real typed Kernel dispatch.
// It is not the ordinary Source App whose runtime is qualified separately.
#[wasm_bindgen]
#[allow(clippy::too_many_lines)]
pub async fn operate_access(
    auth_batch: js_sys::Function,
    access_batch: js_sys::Function,
    auth_configuration: String,
    private_secrets: String,
    token: String,
    request: String,
) -> Result<String, JsValue> {
    let request: Request = serde_json::from_str(&request).map_err(|_| unavailable())?;
    validate(&request)?;
    let config: serde_json::Value =
        serde_json::from_str(&auth_configuration).map_err(|_| unavailable())?;
    if config["d1_binding"] != "AUTH_DB" || config["database_url_secret"] != "" {
        return Err(unavailable());
    }
    let private_secrets = Zeroizing::new(private_secrets);
    let token = Zeroizing::new(token);
    let raw: BTreeMap<String, String> =
        serde_json::from_str(&private_secrets).map_err(|_| unavailable())?;
    let expected = [
        config["assertion_signing_key_secret"].as_str(),
        config["token_pepper_secret"].as_str(),
    ];
    if raw.len() != 2
        || expected.iter().any(|key| {
            key.is_none_or(|key| {
                raw.get(key)
                    .is_none_or(|value| value.len() < 32 || value.len() > 8192)
            })
        })
    {
        return Err(unavailable());
    }
    let bound = BoundSecrets(Rc::new(
        raw.into_iter()
            .map(|(key, value)| (key, Zeroizing::new(value)))
            .collect(),
    ));
    let access_config = json!({"binding":"ACCESS_CONTROL_DB","auth_issuer":config["issuer"],"auth_assertion_public_key":config["assertion_public_key"],"bootstrap_callers":[CALLER],"directory_callers":[CALLER]});
    let mut caller = PluginInstancePlan::new(CALLER, CALLER_PACKAGE);
    let mut auth_provider = PluginInstancePlan::new(AUTH, auth_owner::PACKAGE_ID)
        .with_configuration(config.to_string())
        .with_requirement(CapabilityRequirementPlan::one(
            secrets::CAPABILITY_ID,
            secrets::DESCRIPTOR_VERSION,
        ));
    let mut access_provider = PluginInstancePlan::new(ACCESS, access_owner::PACKAGE_ID)
        .with_configuration(access_config.to_string());
    for (id, version, mut operations) in [
        (
            auth::CAPABILITY_ID,
            auth::DESCRIPTOR_VERSION,
            vec![auth::AUTHENTICATE_OPERATION],
        ),
        (
            credential::CAPABILITY_ID,
            credential::DESCRIPTOR_VERSION,
            vec![credential::INSPECT_OPERATION],
        ),
        (
            tokens::CAPABILITY_ID,
            tokens::DESCRIPTOR_VERSION,
            vec![
                tokens::ISSUE_OPERATION,
                tokens::LIST_OPERATION,
                tokens::REVOKE_OPERATION,
                tokens::RECEIPT_OPERATION,
            ],
        ),
    ] {
        operations.sort_unstable();
        auth_provider =
            auth_provider.with_capability(CapabilityEndpointPlan::new(id, version, operations));
    }
    for (id, version, mut operations) in [
        (
            access::CAPABILITY_ID,
            access::DESCRIPTOR_VERSION,
            vec![access::CHECK_PERMISSION_OPERATION],
        ),
        (
            admin::CAPABILITY_ID,
            admin::DESCRIPTOR_VERSION,
            vec![
                admin::BOOTSTRAP_SCOPE_OPERATION,
                admin::CREATE_ROLE_OPERATION,
                admin::ASSIGN_ROLE_OPERATION,
                admin::REVOKE_ROLE_OPERATION,
                admin::DELETE_ROLE_OPERATION,
                admin::SET_ROLE_PERMISSIONS_OPERATION,
            ],
        ),
        (
            directory::CAPABILITY_ID,
            directory::DESCRIPTOR_VERSION,
            vec![
                directory::GET_ROLE_OPERATION,
                directory::LIST_ROLES_OPERATION,
                directory::LIST_SUBJECT_ROLES_OPERATION,
            ],
        ),
    ] {
        operations.sort_unstable();
        access_provider =
            access_provider.with_capability(CapabilityEndpointPlan::new(id, version, operations));
    }
    for (id, version) in [
        (auth::CAPABILITY_ID, auth::DESCRIPTOR_VERSION),
        (admin::CAPABILITY_ID, admin::DESCRIPTOR_VERSION),
        (directory::CAPABILITY_ID, directory::DESCRIPTOR_VERSION),
    ] {
        caller = caller.with_requirement(CapabilityRequirementPlan::one(id, version));
    }
    let secret_provider = PluginInstancePlan::new(SECRETS, SECRETS_PACKAGE).with_capability(
        CapabilityEndpointPlan::new(
            secrets::CAPABILITY_ID,
            secrets::DESCRIPTOR_VERSION,
            vec![secrets::RESOLVE_OPERATION],
        ),
    );
    let plan = AppComposition::new(
        vec![caller, auth_provider, access_provider, secret_provider],
        vec![
            CapabilityBinding::new(CALLER, auth::CAPABILITY_ID, auth::DESCRIPTOR_VERSION, AUTH),
            CapabilityBinding::new(
                CALLER,
                admin::CAPABILITY_ID,
                admin::DESCRIPTOR_VERSION,
                ACCESS,
            ),
            CapabilityBinding::new(
                CALLER,
                directory::CAPABILITY_ID,
                directory::DESCRIPTOR_VERSION,
                ACCESS,
            ),
            CapabilityBinding::new(
                AUTH,
                secrets::CAPABILITY_ID,
                secrets::DESCRIPTOR_VERSION,
                SECRETS,
            ),
        ],
    )
    .resolve()
    .map_err(|_| JsValue::from_str("operator_access_plan_unavailable"))?;
    let driver = WorkersDriver::new();
    let _event = Event(driver.clone());
    let app = Kernel::start_native(
        plan,
        driver,
        NativePluginRegistry::new()
            .with_factory(Caller)
            .with_factory(bound)
            .with_factory(auth_owner::workers_factory("AUTH_DB", auth_batch))
            .with_factory(access_owner::workers::factory(
                "ACCESS_CONTROL_DB",
                access_batch,
            )),
    )
    .await
    .map_err(|failure| {
        JsValue::from_str(match failure {
            RuntimeFailure::InvalidResolvedPlan { detail } => {
                if detail.contains("no linked constructor") {
                    "operator_access_start_constructor_missing"
                } else if detail.contains("multiple") && detail.contains("constructor") {
                    "operator_access_start_constructor_duplicate"
                } else if detail.contains("exclusively constructed") {
                    "operator_access_start_constructor_not_exclusive"
                } else if detail.contains("config") {
                    "operator_access_start_configuration_invalid"
                } else if detail.contains("endpoint") {
                    "operator_access_start_endpoint_invalid"
                } else if detail.contains("factory") || detail.contains("factories") {
                    "operator_access_start_factory_invalid"
                } else {
                    "operator_access_start_invalid_plan"
                }
            }
            RuntimeFailure::PluginFailure { .. } => "operator_access_start_plugin_failure",
            RuntimeFailure::AdmissionClosed => "operator_access_start_admission_closed",
            _ => "operator_access_start_unavailable",
        })
    })?;
    let response = app
        .invoke::<auth::Auth>(
            CALLER,
            auth::AUTHENTICATE_OPERATION,
            authenticate_request(Some(CredentialEvidence::new("bearer", token.to_string()))),
        )
        .await
        .map_err(|_| JsValue::from_str("operator_access_auth_runtime_unavailable"))?
        .map_err(|_| JsValue::from_str("operator_access_auth_domain_rejected"))?;
    let AuthOutcome::Authenticated(actor) =
        decode_auth_response(response).map_err(|_| unavailable())?
    else {
        return Err(unavailable());
    };
    if actor.actor_kind() != "user" {
        return Err(unavailable());
    }
    let context = || {
        actor
            .attach(app.invocation_context_after(Duration::from_secs(10), CancellationToken::new()))
            .map_err(|_| unavailable())
    };
    let result = match request {
        Request::Bootstrap { subject, scopes } => {
            if subject != actor.subject()
                || !label(&subject)
                || scopes.is_empty()
                || scopes.len() > 16
            {
                return Err(unavailable());
            }
            for scope in scopes {
                if !label(&scope.kind) || !label(&scope.id) || scope.roles.len() > 16 {
                    return Err(unavailable());
                }
                app.invoke::<admin::AccessControlAdminBootstrapScope>(
                    CALLER,
                    admin::BOOTSTRAP_SCOPE_OPERATION,
                    admin::BootstrapScopeRequest {
                        subject: subject.clone(),
                        scope: admin::BootstrapScopeRequestScope {
                            kind: scope.kind.clone(),
                            id: scope.id.clone(),
                        },
                    },
                )
                .await
                .map_err(|_| unavailable())?
                .map_err(|_| unavailable())?;
                for role in scope.roles {
                    if !label(&role.id) || role.subjects.len() > 32 {
                        return Err(unavailable());
                    }
                    app.invoke_with_context::<admin::AccessControlAdminCreateRole>(
                        CALLER,
                        admin::CREATE_ROLE_OPERATION,
                        context()?,
                        admin::CreateRoleRequest {
                            role_id: role.id.clone(),
                            name: role.id.clone(),
                            scope: admin::CreateRoleRequestScope {
                                kind: scope.kind.clone(),
                                id: scope.id.clone(),
                            },
                        },
                    )
                    .await
                    .map_err(|_| unavailable())?
                    .map_err(|_| unavailable())?;
                    app.invoke_with_context::<admin::AccessControlAdminSetRolePermissions>(
                        CALLER,
                        admin::SET_ROLE_PERMISSIONS_OPERATION,
                        context()?,
                        admin::SetRolePermissionsRequest {
                            role_id: role.id.clone(),
                            permissions: role.permissions,
                            scope: admin::SetRolePermissionsRequestScope {
                                kind: scope.kind.clone(),
                                id: scope.id.clone(),
                            },
                        },
                    )
                    .await
                    .map_err(|_| unavailable())?
                    .map_err(|_| unavailable())?;
                    for assigned in role.subjects {
                        if !label(&assigned) {
                            return Err(unavailable());
                        }
                        app.invoke_with_context::<admin::AccessControlAdminAssignRole>(
                            CALLER,
                            admin::ASSIGN_ROLE_OPERATION,
                            context()?,
                            admin::AssignRoleRequest {
                                role_id: role.id.clone(),
                                subject: assigned,
                                scope: admin::AssignRoleRequestScope {
                                    kind: scope.kind.clone(),
                                    id: scope.id.clone(),
                                },
                            },
                        )
                        .await
                        .map_err(|_| unavailable())?
                        .map_err(|_| unavailable())?;
                    }
                }
            }
            json!({"completed":true})
        }
        Request::RevokeRole {
            kind,
            id,
            subject,
            role_id,
        } => {
            let result = app
                .invoke_with_context::<admin::AccessControlAdminRevokeRole>(
                    CALLER,
                    admin::REVOKE_ROLE_OPERATION,
                    context()?,
                    admin::RevokeRoleRequest {
                        role_id,
                        subject,
                        scope: admin::RevokeRoleRequestScope { kind, id },
                    },
                )
                .await
                .map_err(|_| unavailable())?
                .map_err(|_| unavailable())?;
            serde_json::to_value(result).map_err(|_| unavailable())?
        }
        Request::ListRoles { kind, id } => {
            let result = app
                .invoke::<directory::AccessControlDirectoryListRoles>(
                    CALLER,
                    directory::LIST_ROLES_OPERATION,
                    directory::ListRolesRequest {
                        scope: directory::Scope { kind, id },
                        cursor: None,
                        limit: 100,
                    },
                )
                .await
                .map_err(|_| unavailable())?;
            match result {
                Ok(result) => serde_json::to_value(result).map_err(|_| unavailable())?,
                Err(directory::ListRolesError::ScopeNotBootstrapped) => {
                    json!({"scope_bootstrapped":false,"roles":[]})
                }
                Err(_) => return Err(unavailable()),
            }
        }
    };
    if app.shutdown(Duration::from_secs(1)).await != lenso_kernel::ShutdownOutcome::Clean {
        return Err(unavailable());
    }
    serde_json::to_string(&result).map_err(|_| unavailable())
}
