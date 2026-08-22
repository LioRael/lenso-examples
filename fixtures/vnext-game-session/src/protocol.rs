use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    rc::Rc,
    str::FromStr,
    time::Duration,
};

use lenso_capability_auth::AuthClient;
use lenso_capability_game_session::GameSessionClient;
use lenso_kernel::{
    ActivateContext, CancellationToken, ModuleDependencies, ModuleLifecycle, RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::{
    connection::{send_frame, serve_connection},
    frame::ServerFrame,
};

/// Native package identity for the primary protocol Module.
pub const PROTOCOL_PACKAGE_ID: &str = "fixture.game.protocol";
/// Alternate native package identity proving protocol replacement by Composition.
pub const PROTOCOL_REPLACEMENT_PACKAGE_ID: &str = "fixture.game.protocol.replacement";

const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024;
const DEFAULT_MAX_CONNECTIONS: usize = 8;
const DEFAULT_IDLE_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_SESSION_TIMEOUT_MS: u64 = 30_000;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 1_024;

/// Protocol package selected by App Composition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProtocolVariant {
    /// The primary fixture protocol package.
    #[default]
    Primary,
    /// An alternate package with the same documented wire contract.
    Replacement,
}

impl ProtocolVariant {
    pub(crate) const fn package_id(self) -> &'static str {
        match self {
            Self::Primary => PROTOCOL_PACKAGE_ID,
            Self::Replacement => PROTOCOL_REPLACEMENT_PACKAGE_ID,
        }
    }
}

/// Configuration owned by the protocol Module and selected by Composition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolConfig {
    /// TCP listen address. Port zero asks the OS for an available fixture port.
    pub bind: String,
    /// Maximum encoded JSON payload in one length-prefixed frame.
    pub max_frame_bytes: usize,
    /// Maximum number of accepted connections in this Module generation.
    pub max_connections: usize,
    /// Maximum time without a frame while an established session is active.
    pub idle_timeout_ms: u64,
    /// Maximum lifetime of one authenticated session.
    pub session_timeout_ms: u64,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:0".to_owned(),
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            idle_timeout_ms: DEFAULT_IDLE_TIMEOUT_MS,
            session_timeout_ms: DEFAULT_SESSION_TIMEOUT_MS,
        }
    }
}

impl ProtocolConfig {
    /// Serializes this immutable Module configuration for an App Plan.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("protocol configuration must serialize")
    }

    pub(crate) fn from_json(value: &str) -> Result<Self, RuntimeFailure> {
        let config: Self =
            serde_json::from_str(value).map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("protocol configuration is invalid JSON: {error}"),
            })?;
        config.validate().map(|_| config)
    }

    pub(crate) fn validate(&self) -> Result<SocketAddr, RuntimeFailure> {
        let address = SocketAddr::from_str(&self.bind).map_err(|error| {
            RuntimeFailure::InvalidResolvedPlan {
                detail: format!("protocol bind address is invalid: {error}"),
            }
        })?;
        if self.max_frame_bytes < 256 || self.max_frame_bytes > MAX_FRAME_BYTES {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: format!(
                    "protocol max_frame_bytes must be between 256 and {MAX_FRAME_BYTES}"
                ),
            });
        }
        if self.max_connections == 0 || self.max_connections > MAX_CONNECTIONS {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: format!("protocol max_connections must be between 1 and {MAX_CONNECTIONS}"),
            });
        }
        if self.idle_timeout_ms == 0 {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "protocol idle_timeout_ms must be greater than zero".to_owned(),
            });
        }
        if self.session_timeout_ms < self.idle_timeout_ms {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "protocol session_timeout_ms must be at least idle_timeout_ms".to_owned(),
            });
        }
        Ok(address)
    }

    pub fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub(crate) fn idle_timeout(&self) -> Duration {
        Duration::from_millis(self.idle_timeout_ms)
    }

    pub(crate) fn session_timeout(&self) -> Duration {
        Duration::from_millis(self.session_timeout_ms)
    }
}

#[derive(Debug, Default)]
struct ProtocolObserver {
    local_addr: Cell<Option<SocketAddr>>,
}

#[derive(Debug, Default)]
struct ProtocolGenerationState {
    listener: RefCell<Option<TcpListener>>,
    active_connections: Cell<usize>,
}

/// Factory for a replaceable native protocol Module.
#[derive(Clone, Debug)]
pub struct GameProtocolFactory {
    variant: ProtocolVariant,
    observer: Rc<ProtocolObserver>,
}

impl GameProtocolFactory {
    /// Creates the primary protocol Module factory.
    pub fn new() -> Self {
        Self::with_variant(ProtocolVariant::Primary)
    }

    /// Creates the package selected by Composition.
    pub fn with_variant(variant: ProtocolVariant) -> Self {
        Self {
            variant,
            observer: Rc::new(ProtocolObserver::default()),
        }
    }

    /// Returns the concrete address after the Module has prepared its listener.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.observer.local_addr.get()
    }
}

impl Default for GameProtocolFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeModuleFactory for GameProtocolFactory {
    fn package_id(&self) -> &'static str {
        self.variant.package_id()
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let config = ProtocolConfig::from_json(context.configuration())?;
        Ok(NativeModuleInstance::with_lifecycle(
            Vec::new(),
            ProtocolLifecycle {
                config,
                observer: self.observer.clone(),
                state: Rc::new(ProtocolGenerationState::default()),
            },
        ))
    }
}

#[derive(Debug)]
struct ProtocolLifecycle {
    config: ProtocolConfig,
    observer: Rc<ProtocolObserver>,
    state: Rc<ProtocolGenerationState>,
}

impl ModuleLifecycle for ProtocolLifecycle {
    fn prepare(&self, _context: lenso_kernel::PrepareContext) -> lenso_kernel::ModuleFuture {
        let config = self.config.clone();
        let observer = self.observer.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let address = config.validate()?;
            let listener = TcpListener::bind(address).await.map_err(|error| {
                RuntimeFailure::ModuleFailure {
                    detail: format!("protocol listener bind failed: {error}"),
                }
            })?;
            let local_addr =
                listener
                    .local_addr()
                    .map_err(|error| RuntimeFailure::ModuleFailure {
                        detail: format!("protocol listener address failed: {error}"),
                    })?;
            observer.local_addr.set(Some(local_addr));
            state.listener.borrow_mut().replace(listener);
            Ok(())
        })
    }

    fn activate(&self, context: ActivateContext) -> lenso_kernel::ModuleFuture {
        let auth = match AuthClient::from_dependencies(context.dependencies()) {
            Ok(client) => Rc::new(client),
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let game = match GameSessionClient::from_dependencies(context.dependencies()) {
            Ok(client) => Rc::new(client),
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let Some(listener) = self.state.listener.borrow_mut().take() else {
            return Box::pin(futures::future::ready(Err(RuntimeFailure::Internal {
                detail: "protocol listener was not prepared".to_owned(),
            })));
        };
        let config = self.config.clone();
        let state = self.state.clone();
        let readiness = context.readiness();
        let cancellation = context.cancellation();
        let dependencies = context.dependencies().clone();
        let tasks = context.tasks().clone();
        let runtime = ProtocolRuntime {
            connection: ConnectionRuntime {
                config,
                auth,
                game,
                dependencies,
                module_cancellation: cancellation.clone(),
            },
            state,
            tasks: tasks.clone(),
            cancellation,
        };
        let spawn = tasks.spawn_local(Box::pin(async move {
            readiness.wait().await;
            accept_connections(listener, runtime).await;
        }));
        match spawn {
            Ok(_) => Box::pin(futures::future::ready(Ok(()))),
            Err(error) => Box::pin(futures::future::ready(Err(RuntimeFailure::Internal {
                detail: format!("protocol accept loop could not start: {error:?}"),
            }))),
        }
    }

    fn deactivate(&self, _context: lenso_kernel::DeactivateContext) -> lenso_kernel::ModuleFuture {
        self.state.listener.borrow_mut().take();
        self.observer.local_addr.set(None);
        Box::pin(futures::future::ready(Ok(())))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConnectionRuntime {
    pub(crate) config: ProtocolConfig,
    pub(crate) auth: Rc<AuthClient>,
    pub(crate) game: Rc<GameSessionClient>,
    pub(crate) dependencies: ModuleDependencies,
    pub(crate) module_cancellation: CancellationToken,
}

#[derive(Clone, Debug)]
struct ProtocolRuntime {
    connection: ConnectionRuntime,
    state: Rc<ProtocolGenerationState>,
    tasks: lenso_kernel::ManagedTaskScope,
    cancellation: CancellationToken,
}

async fn accept_connections(listener: TcpListener, runtime: ProtocolRuntime) {
    let ProtocolRuntime {
        connection,
        state,
        tasks,
        cancellation,
    } = runtime;
    let config = &connection.config;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else {
                    tokio::task::yield_now().await;
                    continue;
                };
                if state.active_connections.get() >= config.max_connections() {
                    let mut stream = stream;
                    let _ = send_frame(
                        &mut stream,
                        config,
                        &ServerFrame::Runtime {
                            code: "resource_exhausted".to_owned(),
                        },
                    ).await;
                    continue;
                }
                state.active_connections.set(state.active_connections.get() + 1);
                let guard = ActiveConnectionGuard { state: state.clone() };
                let connection = connection.clone();
                let _ = tasks.spawn_local(Box::pin(async move {
                    let _guard = guard;
                    serve_connection(stream, connection).await;
                }));
            }
        }
    }
}

#[derive(Debug)]
struct ActiveConnectionGuard {
    state: Rc<ProtocolGenerationState>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.state
            .active_connections
            .set(self.state.active_connections.get().saturating_sub(1));
    }
}
