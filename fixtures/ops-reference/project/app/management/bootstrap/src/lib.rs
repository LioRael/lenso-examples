//! Selected only by the explicit operator bootstrap command, never the normal App profile.
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_access_control_admin as admin;
use lenso_capability_auth as auth;
pub mod host_facilities;
use host_facilities::BootstrapHandle;

#[lenso::plugin(consumer)]
#[derive(Clone, Debug)]
struct OperatorBootstrap {
    #[facility(id = "bootstrap")]
    host: BootstrapHandle,
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "access_admin")]
    access: admin::AccessControlAdminClient,
}
fn rejected() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {
        detail: "Explicit Access Control bootstrap was rejected by its owner".into(),
    }
}
#[lenso::plugin_impl]
impl OperatorBootstrap {
    #[create]
    fn create(
        host: BootstrapHandle,
        auth: auth::AuthClient,
        access: admin::AccessControlAdminClient,
        #[lifecycle] lifecycle: lenso_native_adapter::LifecycleContext,
    ) -> Result<Self, &'static str> {
        let plugin = Self { host, auth, access };
        let owner = plugin.clone();
        let readiness = lifecycle
            .readiness()
            .map_err(|_| "Bootstrap has no generation readiness handle")?;
        lifecycle
            .spawn_local(async move {
                readiness.wait().await;
                if owner.run().await.is_ok() {
                    println!("Explicit operators bootstrap completed");
                } else {
                    eprintln!("Explicit operators bootstrap rejected");
                }
            })
            .map_err(|_| "Bootstrap generation task was rejected")?;
        Ok(plugin)
    }
}
impl OperatorBootstrap {
    async fn run(&self) -> Result<(), lenso::RuntimeFailure> {
        let token =
            std::fs::read_to_string(&self.host.bootstrap_token_file).map_err(|_| rejected())?;
        let response = self
            .auth
            .authenticate(authenticate_request(Some(CredentialEvidence::new(
                "bearer", token,
            ))))
            .await
            .map_err(|_| rejected())?;
        let AuthOutcome::Authenticated(actor) =
            decode_auth_response(response).map_err(|_| rejected())?
        else {
            return Err(rejected());
        };
        if actor.actor_kind() != "user" || actor.subject() != self.host.subject {
            return Err(rejected());
        }
        for scope in &self.host.scopes {
            self.access
                .bootstrap_scope(admin::BootstrapScopeRequest {
                    subject: self.host.subject.clone(),
                    scope: admin::BootstrapScopeRequestScope {
                        kind: scope.kind.clone(),
                        id: scope.id.clone(),
                    },
                })
                .await
                .map_err(|_| rejected())?;
            for role in &scope.roles {
                let context = || {
                    actor
                        .attach(lenso::Ctx::new(0, None, lenso::CancellationToken::new()))
                        .map_err(|_| rejected())
                };
                self.access
                    .create_role_with_context(
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
                    .map_err(|_| rejected())?;
                self.access
                    .set_role_permissions_with_context(
                        context()?,
                        admin::SetRolePermissionsRequest {
                            role_id: role.id.clone(),
                            permissions: role.permissions.clone(),
                            scope: admin::SetRolePermissionsRequestScope {
                                kind: scope.kind.clone(),
                                id: scope.id.clone(),
                            },
                        },
                    )
                    .await
                    .map_err(|_| rejected())?;
                for subject in &role.subjects {
                    self.access
                        .assign_role_with_context(
                            context()?,
                            admin::AssignRoleRequest {
                                subject: subject.clone(),
                                role_id: role.id.clone(),
                                scope: admin::AssignRoleRequestScope {
                                    kind: scope.kind.clone(),
                                    id: scope.id.clone(),
                                },
                            },
                        )
                        .await
                        .map_err(|_| rejected())?;
                }
            }
        }
        Ok(())
    }
}
