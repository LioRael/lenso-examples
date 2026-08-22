//! Target-owned App Web UI tracer bullet.

mod auth;
mod browser;
mod orders;
mod plan;
mod shell;

use std::{net::SocketAddr, time::Duration};

use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_authoring::{ProjectAuthoring, ResolutionOptions};
use lenso_capability_auth::{AUTHENTICATE_OPERATION, Auth};
use lenso_capability_secure_greeting::{GREET_OPERATION, GreetRequest, SecureGreeting};
use lenso_kernel::{CancellationToken, Kernel, NativeApp, RuntimeFailure, ShutdownOutcome};
use lenso_native_adapter::NativeModuleRegistry;
use lenso_runner::TokioDriver;

pub use orders::ORDERS_PACKAGE_ID;
pub use plan::{UI_CONTRIBUTION_CAPABILITY_ID, WEB_SHELL_CAPABILITY_ID};

use auth::{AuthModuleFactory, fixture_issuer};
use browser::BrowserAdapterFactory;
use orders::{OrdersModuleFactory, WorkerModuleFactory};
use shell::WebShellFactory;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MetadataScenario {
    #[default]
    Valid,
    MissingRoute,
    CollidingRoute,
}

/// One target-owned Web profile selected entirely before Kernel boot.
#[derive(Clone, Debug, Default)]
pub struct WebUiFixture {
    metadata: MetadataScenario,
    web_enabled: bool,
    open_system_browser: bool,
}

impl WebUiFixture {
    pub const fn orders() -> Self {
        Self {
            metadata: MetadataScenario::Valid,
            web_enabled: true,
            open_system_browser: false,
        }
    }

    #[must_use]
    pub const fn with_metadata_scenario(mut self, metadata: MetadataScenario) -> Self {
        self.metadata = metadata;
        self
    }

    #[must_use]
    pub const fn without_web(mut self) -> Self {
        self.web_enabled = false;
        self
    }

    /// Selects the host browser launcher; tests use the default recorder.
    #[must_use]
    pub const fn with_system_browser(mut self) -> Self {
        self.open_system_browser = true;
        self
    }

    pub fn project_file(&self) -> lenso_authoring::ProjectFile {
        plan::project(self.metadata, self.web_enabled)
    }

    pub fn resolved_plan(
        &self,
    ) -> Result<lenso_app_plan::ResolvedAppPlan, lenso_authoring::AuthoringError> {
        let options = if self.web_enabled {
            ResolutionOptions::default().with_profile("web")
        } else {
            ResolutionOptions::default()
        };
        self.project_file()
            .resolve(&plan::workspace_root(), &options)
            .map(|project| project.plan().clone())
    }

    pub async fn start(&self) -> Result<RunningWebUi, RuntimeFailure> {
        let issuer = fixture_issuer();
        let browser = if self.open_system_browser {
            BrowserAdapterFactory::with_system_browser()
        } else {
            BrowserAdapterFactory::new()
        };
        let registry = NativeModuleRegistry::new()
            .with_factory(OrdersModuleFactory::new(issuer.verifier()))
            .with_factory(AuthModuleFactory::new(issuer))
            .with_factory(WebShellFactory)
            .with_factory(browser.clone())
            .with_factory(WorkerModuleFactory);
        let plan = self
            .resolved_plan()
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: error.to_string(),
            })?;
        let app = Kernel::start_native(plan, TokioDriver::new(), registry).await?;
        if self.web_enabled {
            browser.wait_until_launched().await?;
        }
        Ok(RunningWebUi { app, browser })
    }
}

#[derive(Clone, Debug)]
pub struct RunningWebUi {
    app: NativeApp,
    browser: BrowserAdapterFactory,
}

impl RunningWebUi {
    pub const fn app(&self) -> &NativeApp {
        &self.app
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.browser.local_addr()
    }

    pub fn launched_url(&self) -> Option<String> {
        self.browser.launched_url()
    }

    pub async fn invoke_worker(&self, token: &str, name: &str) -> Result<String, RuntimeFailure> {
        let auth = self
            .app
            .invoke::<Auth>(
                "worker",
                AUTHENTICATE_OPERATION,
                authenticate_request(Some(CredentialEvidence::new("bearer", token))),
            )
            .await?
            .map_err(|error| RuntimeFailure::ModuleFailure {
                detail: format!("worker authentication rejected: {error:?}"),
            })?;
        let AuthOutcome::Authenticated(assertion) =
            decode_auth_response(auth).map_err(|error| RuntimeFailure::ModuleFailure {
                detail: format!("worker Auth response was invalid: {error:?}"),
            })?
        else {
            return Err(RuntimeFailure::ModuleFailure {
                detail: "worker authentication was absent".to_owned(),
            });
        };
        let context = assertion
            .attach(self.app.invocation_context(None, CancellationToken::new()))
            .map_err(|error| RuntimeFailure::ModuleFailure {
                detail: format!("worker assertion could not attach: {error}"),
            })?;
        let response = self
            .app
            .invoke_with_context::<SecureGreeting>(
                "worker",
                GREET_OPERATION,
                context,
                GreetRequest {
                    name: name.to_owned(),
                },
            )
            .await?
            .map_err(|error| RuntimeFailure::ModuleFailure {
                detail: format!("worker greeting rejected: {error:?}"),
            })?;
        Ok(response.message)
    }

    pub async fn shutdown(&self, timeout: Duration) -> ShutdownOutcome {
        self.app.shutdown(timeout).await
    }
}
