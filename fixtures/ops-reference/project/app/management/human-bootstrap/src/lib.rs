//! Explicit private account enrollment through real owner ports; absent from normal profiles.
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_access_control_admin as admin;
use lenso_capability_auth as auth;
use lenso_capability_credential_issuer as issuer;
use lenso_capability_password_auth as password;
use lenso_management_authority::QualificationStore;
use std::io::Write as _;
pub mod host_facilities;
use host_facilities::BootstrapHandle;
#[lenso::plugin(consumer)]
#[derive(Clone, Debug)]
struct HumanBootstrap {
    #[facility(id = "bootstrap")]
    host: BootstrapHandle,
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "password")]
    password: password::PasswordClient,
    #[dependency(id = "account_issuer")]
    issuer: issuer::CredentialIssuerClient,
    #[dependency(id = "access_admin")]
    access: admin::AccessControlAdminClient,
}
fn rejected() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {
        detail: "Explicit human owner bootstrap was rejected".into(),
    }
}
fn secret(path: &std::path::Path) -> Result<String, lenso::RuntimeFailure> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| rejected())?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(rejected());
    }
    let value = std::fs::read_to_string(path).map_err(|_| rejected())?;
    if value.is_empty() || value.len() > 8192 {
        return Err(rejected());
    }
    Ok(value)
}
#[lenso::plugin_impl]
impl HumanBootstrap {
    #[create]
    fn create(
        host: BootstrapHandle,
        auth: auth::AuthClient,
        password: password::PasswordClient,
        issuer: issuer::CredentialIssuerClient,
        access: admin::AccessControlAdminClient,
        #[lifecycle] lifecycle: lenso_native_adapter::LifecycleContext,
    ) -> Result<Self, &'static str> {
        let plugin = Self {
            host,
            auth,
            password,
            issuer,
            access,
        };
        let owner = plugin.clone();
        let ready = lifecycle
            .readiness()
            .map_err(|_| "Human bootstrap has no generation readiness handle")?;
        lifecycle
            .spawn_local(async move {
                ready.wait().await;
                if owner.run().await.is_ok() {
                    println!("Explicit human bootstrap completed");
                } else {
                    eprintln!("Explicit human bootstrap rejected");
                }
            })
            .map_err(|_| "Human bootstrap generation task was rejected")?;
        Ok(plugin)
    }
}
impl HumanBootstrap {
    async fn run(&self) -> Result<(), lenso::RuntimeFailure> {
        let response = self
            .auth
            .authenticate(authenticate_request(Some(CredentialEvidence::new(
                "bearer",
                secret(&self.host.bootstrap_token_file)?,
            ))))
            .await
            .map_err(|_| rejected())?;
        let AuthOutcome::Authenticated(actor) =
            decode_auth_response(response).map_err(|_| rejected())?
        else {
            return Err(rejected());
        };
        if actor.actor_kind() != "user" || actor.subject() != self.host.bootstrap_subject {
            return Err(rejected());
        }
        let qualification =
            QualificationStore::open(&self.host.qualification).map_err(|_| rejected())?;
        let mut accounts = serde_json::Map::new();
        for account in &self.host.accounts {
            let registered = self
                .password
                .register(password::RegisterRequest {
                    identifier: account.identifier.clone(),
                    password: secret(&account.password_file)?,
                })
                .await
                .map_err(|_| rejected())?;
            // The register reply proves the subject. Its initial bearer is never published or retained.
            self.issuer
                .revoke_credential(issuer::RevokeCredentialRequest {
                    credential: registered.credential,
                    scheme: "session".into(),
                })
                .await
                .map_err(|_| rejected())?;
            for role in &account.roles {
                match self
                    .access
                    .bootstrap_scope(admin::BootstrapScopeRequest {
                        subject: self.host.bootstrap_subject.clone(),
                        scope: admin::BootstrapScopeRequestScope {
                            kind: role.kind.clone(),
                            id: role.id.clone(),
                        },
                    })
                    .await
                {
                    Ok(_)
                    | Err(admin::AccessControlAdminBootstrapScopeInvocationError::Domain(
                        admin::BootstrapScopeError::ScopeAlreadyBootstrapped,
                    )) => {}
                    _ => return Err(rejected()),
                }
                let context = || {
                    actor
                        .attach(lenso::Ctx::new(0, None, lenso::CancellationToken::new()))
                        .map_err(|_| rejected())
                };
                match self
                    .access
                    .create_role_with_context(
                        context()?,
                        admin::CreateRoleRequest {
                            role_id: role.role_id.clone(),
                            name: role.role_id.clone(),
                            scope: admin::CreateRoleRequestScope {
                                kind: role.kind.clone(),
                                id: role.id.clone(),
                            },
                        },
                    )
                    .await
                {
                    Ok(_)
                    | Err(admin::AccessControlAdminCreateRoleInvocationError::Domain(
                        admin::CreateRoleError::RoleAlreadyExists,
                    )) => {}
                    _ => return Err(rejected()),
                }
                self.access
                    .set_role_permissions_with_context(
                        context()?,
                        admin::SetRolePermissionsRequest {
                            role_id: role.role_id.clone(),
                            permissions: role.permissions.clone(),
                            scope: admin::SetRolePermissionsRequestScope {
                                kind: role.kind.clone(),
                                id: role.id.clone(),
                            },
                        },
                    )
                    .await
                    .map_err(|_| rejected())?;
                self.access
                    .assign_role_with_context(
                        context()?,
                        admin::AssignRoleRequest {
                            subject: registered.subject.clone(),
                            role_id: role.role_id.clone(),
                            scope: admin::AssignRoleRequestScope {
                                kind: role.kind.clone(),
                                id: role.id.clone(),
                            },
                        },
                    )
                    .await
                    .map_err(|_| rejected())?;
            }
            qualification
                .grant(&self.host.deployment, &registered.subject)
                .map_err(|_| rejected())?;
            accounts.insert(
                account.alias.clone(),
                serde_json::json!({"subject":registered.subject,"identifier":account.identifier}),
            );
        }
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&self.host.receipt)
            .map_err(|_| rejected())?;
        output
            .write_all(
                serde_json::to_string_pretty(
                    &serde_json::json!({"deployment":self.host.deployment,"accounts":accounts}),
                )
                .map_err(|_| rejected())?
                .as_bytes(),
            )
            .map_err(|_| rejected())?;
        output.sync_all().map_err(|_| rejected())?;
        Ok(())
    }
}
