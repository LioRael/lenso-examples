//! A native stateful Module fixture with owned storage and explicit Secrets.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use lenso_capability_counter::{
    CounterEndpoint, CounterIncrementInvocationError, CounterProvider, CounterReadInvocationError,
    IncrementError, IncrementRequest, IncrementResponse, ReadError, ReadRequest, ReadResponse,
};
use lenso_capability_secrets::{ResolveRequest, SecretsClient};
use lenso_kernel::{
    ActivateContext, DeactivateContext, InvocationContext, ModuleFuture, ModuleLifecycle,
    NativeRequestEndpoint, PrepareContext, RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::Deserialize;

mod storage;
use storage::FileStateAdapter;

pub use storage::{RecoveryOutcome, SetupOutcome, StateStorageError, UpgradeOutcome};

/// Runs the owned Module setup workflow without exposing its persistence Adapter.
pub fn setup_owned_state(path: impl Into<PathBuf>) -> Result<SetupOutcome, StateStorageError> {
    storage::setup_owned_state(path)
}

/// Runs the owned Module upgrade workflow without exposing its persistence Adapter.
pub fn upgrade_owned_state(path: impl Into<PathBuf>) -> Result<UpgradeOutcome, StateStorageError> {
    storage::upgrade_owned_state(path)
}

/// Runs the explicit recovery workflow for an interrupted owned transaction.
pub fn recover_owned_state(path: impl Into<PathBuf>) -> Result<RecoveryOutcome, StateStorageError> {
    storage::recover_owned_state(path)
}

/// Package identity for the example state owner.
pub const COUNTER_PACKAGE_ID: &str = "example.owned-counter";
/// The only schema version currently accepted by the counter Module.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
/// The owned migration artifact compiled into this Module package.
pub const INITIAL_MIGRATION: &str = include_str!("../migrations/001-counter-state-v1.json");

/// Opaque non-sensitive configuration owned by the counter Module.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterConfiguration {
    storage_path: PathBuf,
    secret_ref: String,
}

#[derive(Debug)]
struct CounterRuntime {
    storage: FileStateAdapter,
    secret_ready: Cell<bool>,
}

#[derive(Debug)]
struct CounterLifecycle {
    runtime: Rc<CounterRuntime>,
    secret_ref: String,
}

impl ModuleLifecycle for CounterLifecycle {
    fn prepare(&self, _context: PrepareContext) -> ModuleFuture {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            runtime
                .storage
                .verify_ready()
                .map_err(|error| storage_runtime_failure(&error))
        })
    }

    fn activate(&self, context: ActivateContext) -> ModuleFuture {
        let runtime = Rc::clone(&self.runtime);
        let secret_ref = self.secret_ref.clone();
        Box::pin(async move {
            let secrets = SecretsClient::from_dependencies(context.dependencies())?;
            let response = secrets
                .resolve(ResolveRequest {
                    reference: secret_ref.clone(),
                })
                .await
                .map_err(|error| match error {
                    lenso_capability_secrets::SecretsInvocationError::Runtime(error) => error,
                    lenso_capability_secrets::SecretsInvocationError::Domain(error) => {
                        RuntimeFailure::ModuleFailure {
                            detail: format!(
                                "required secret reference `{secret_ref}` failed: {error:?}"
                            ),
                        }
                    }
                })?;
            if response.value.is_empty() {
                return Err(RuntimeFailure::ModuleFailure {
                    detail: format!(
                        "required secret reference `{secret_ref}` resolved to an empty value"
                    ),
                });
            }
            runtime.secret_ready.set(true);
            Ok(())
        })
    }

    fn deactivate(&self, _context: DeactivateContext) -> ModuleFuture {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            runtime.secret_ready.set(false);
            Ok(())
        })
    }
}

#[derive(Debug)]
struct CounterProviderImpl {
    runtime: Rc<CounterRuntime>,
}

impl CounterProvider for CounterProviderImpl {
    fn read(
        &self,
        _context: InvocationContext,
        request: ReadRequest,
    ) -> futures::future::LocalBoxFuture<'static, Result<ReadResponse, CounterReadInvocationError>>
    {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            ensure_secret_ready(&runtime).map_err(CounterReadInvocationError::Runtime)?;
            if request.key.is_empty() {
                return Err(CounterReadInvocationError::Domain(ReadError::InvalidKey));
            }
            let Some((value, revision)) =
                runtime
                    .storage
                    .read_counter(&request.key)
                    .map_err(|error| {
                        CounterReadInvocationError::Runtime(storage_runtime_failure(&error))
                    })?
            else {
                return Err(CounterReadInvocationError::Domain(ReadError::MissingKey));
            };
            Ok(ReadResponse {
                value,
                revision: revision.to_string(),
            })
        })
    }

    fn increment(
        &self,
        _context: InvocationContext,
        request: IncrementRequest,
    ) -> futures::future::LocalBoxFuture<
        'static,
        Result<IncrementResponse, CounterIncrementInvocationError>,
    > {
        let runtime = Rc::clone(&self.runtime);
        Box::pin(async move {
            ensure_secret_ready(&runtime).map_err(CounterIncrementInvocationError::Runtime)?;
            if request.key.is_empty() {
                return Err(CounterIncrementInvocationError::Domain(
                    IncrementError::InvalidKey,
                ));
            }
            let (value, revision) = runtime
                .storage
                .increment_counter(&request.key, request.amount)
                .map_err(|error| {
                    CounterIncrementInvocationError::Runtime(storage_runtime_failure(&error))
                })?;
            Ok(IncrementResponse {
                value,
                revision: revision.to_string(),
            })
        })
    }
}

fn ensure_secret_ready(runtime: &CounterRuntime) -> Result<(), RuntimeFailure> {
    runtime
        .secret_ready
        .get()
        .then_some(())
        .ok_or_else(|| RuntimeFailure::ModuleFailure {
            detail: "counter Module has no resolved Secrets Capability".to_owned(),
        })
}

fn storage_runtime_failure(error: &StateStorageError) -> RuntimeFailure {
    RuntimeFailure::Internal {
        detail: error.to_string(),
    }
}

/// Statically linked factory for the owned counter Module.
#[derive(Debug)]
pub struct CounterFactory;

impl NativeModuleFactory for CounterFactory {
    fn package_id(&self) -> &'static str {
        COUNTER_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let configuration: CounterConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("counter Module configuration is invalid: {error}"),
            })?;
        if configuration.secret_ref.is_empty() || configuration.storage_path.as_os_str().is_empty()
        {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "counter Module requires storage_path and secret_ref".to_owned(),
            });
        }
        let runtime = Rc::new(CounterRuntime {
            storage: FileStateAdapter::new(configuration.storage_path),
            secret_ready: Cell::new(false),
        });
        let endpoint: Rc<dyn NativeRequestEndpoint> =
            Rc::new(CounterEndpoint::new(CounterProviderImpl {
                runtime: Rc::clone(&runtime),
            }));
        Ok(NativeModuleInstance::with_lifecycle(
            vec![endpoint],
            CounterLifecycle {
                runtime,
                secret_ref: configuration.secret_ref,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_documents_upgrade_only_when_the_explicit_workflow_runs() {
        let root = std::env::temp_dir().join(format!(
            "lenso-stateful-unit-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let path = root.join("counter.json");
        std::fs::create_dir_all(&root).expect("test storage directory should be created");
        std::fs::write(&path, r#"{"schema_version":0,"entries":{"legacy":4}}"#)
            .expect("legacy state should be written");

        let adapter = FileStateAdapter::new(&path);
        assert_eq!(
            adapter.upgrade().expect("explicit upgrade should work"),
            UpgradeOutcome::Applied { from: 0, to: 1 }
        );
        assert!(adapter.verify_ready().is_ok());

        std::fs::remove_dir_all(root).expect("test storage should be removed");
    }
}
