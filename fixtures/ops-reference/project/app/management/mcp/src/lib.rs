use lenso_capability_auth as auth;
use lenso_capability_management as management;
use lenso_management_mcp::{NativeBridge, Profile, router};
use serde::Deserialize;
use std::{cell::RefCell, rc::Rc, time::Duration};
pub mod host_facilities;
use host_facilities::TransportHandle;

#[derive(Clone, Debug, Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct Config {
    allow_state_update: bool,
}
#[derive(Debug)]
struct Running {
    bridge: NativeBridge,
    stop: tokio_util::sync::CancellationToken,
    server: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
#[lenso::plugin(consumer)]
#[derive(Clone, Debug)]
struct OpsMcp {
    #[config]
    config: Config,
    #[facility(id = "transport")]
    host: TransportHandle,
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "management")]
    management: management::ManagementClient,
    running: Rc<RefCell<Option<Running>>>,
}
fn failure(_: std::io::Error) -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {
        detail: "Selected MCP transport failed".into(),
    }
}
#[lenso::plugin_impl]
impl OpsMcp {
    #[create]
    async fn create(
        config: Config,
        host: TransportHandle,
        auth: auth::AuthClient,
        management: management::ManagementClient,
    ) -> Result<Self, &'static str> {
        let owner = Self {
            config,
            host,
            auth,
            management,
            running: Rc::new(RefCell::new(None)),
        };
        owner
            .start()
            .await
            .map_err(|_| "Selected MCP complete-object construction failed")?;
        Ok(owner)
    }
    #[stop]
    async fn stop(&self) -> Result<(), &'static str> {
        let running = self.running.borrow_mut().take();
        if let Some(running) = running {
            running.stop.cancel();
            running.bridge.shutdown().await;
            running
                .server
                .await
                .map_err(|_| "MCP shutdown failed")?
                .map_err(|_| "MCP shutdown failed")?;
        }
        Ok(())
    }
}
impl OpsMcp {
    async fn start(&self) -> Result<(), lenso::RuntimeFailure> {
        let clock = self.host.clone();
        let bridge = NativeBridge::spawn(
            self.auth.clone(),
            self.management.clone(),
            self.host.resource_uri.clone(),
            Rc::new(move || clock.now()),
            Duration::from_secs(30),
        )
        .map_err(failure)?;
        let mut profile = Profile::read_only(self.host.resource_uri.clone());
        if self.config.allow_state_update {
            profile.allowed_write_entries.insert("state.update".into());
        }
        let application = router(bridge.bridge.clone(), profile).map_err(failure)?;
        let listener = tokio::net::TcpListener::bind(self.host.listen)
            .await
            .map_err(failure)?;
        let stop = tokio_util::sync::CancellationToken::new();
        let shutdown = stop.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, application)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        *self.running.borrow_mut() = Some(Running {
            bridge,
            stop,
            server,
        });
        println!("Management MCP listening on {}", self.host.resource_uri);
        Ok(())
    }
}
