use std::rc::Rc;

use futures::future::LocalBoxFuture;
use lenso_capability_agent_memory::{AppendError, MemoryAppendInvocationError};
use lenso_kernel::{
    ActivateContext, DeactivateContext, InvocationContext, ModuleFuture, ModuleLifecycle,
    RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::Deserialize;

use crate::{MEMORY_PACKAGE_ID, storage};

#[derive(Debug)]
struct MemoryProvider {
    storage: Rc<storage::MemoryWorker>,
}

impl lenso_capability_agent_memory::MemoryProvider for MemoryProvider {
    fn append(
        &self,
        _context: InvocationContext,
        request: lenso_capability_agent_memory::AppendRequest,
    ) -> LocalBoxFuture<
        'static,
        Result<lenso_capability_agent_memory::AppendResponse, MemoryAppendInvocationError>,
    > {
        let storage = self.storage.clone();
        Box::pin(async move {
            if request.key.is_empty() {
                return Err(MemoryAppendInvocationError::Domain(AppendError::InvalidKey));
            }
            let revision = storage
                .append(request.key, request.entry)
                .await
                .map_err(|error| {
                    MemoryAppendInvocationError::Runtime(memory_runtime_failure(error))
                })?;
            Ok(lenso_capability_agent_memory::AppendResponse {
                revision: revision.to_string(),
            })
        })
    }

    fn read(
        &self,
        _context: InvocationContext,
        request: lenso_capability_agent_memory::ReadRequest,
    ) -> LocalBoxFuture<
        'static,
        Result<
            lenso_capability_agent_memory::ReadResponse,
            lenso_capability_agent_memory::MemoryReadInvocationError,
        >,
    > {
        let storage = self.storage.clone();
        Box::pin(async move {
            if request.key.is_empty() {
                return Err(
                    lenso_capability_agent_memory::MemoryReadInvocationError::Domain(
                        lenso_capability_agent_memory::ReadError::InvalidKey,
                    ),
                );
            }
            let Some((entries, revision)) = storage.read(request.key).await.map_err(|error| {
                lenso_capability_agent_memory::MemoryReadInvocationError::Runtime(
                    memory_runtime_failure(error),
                )
            })?
            else {
                return Err(
                    lenso_capability_agent_memory::MemoryReadInvocationError::Domain(
                        lenso_capability_agent_memory::ReadError::MissingKey,
                    ),
                );
            };
            Ok(lenso_capability_agent_memory::ReadResponse {
                entries,
                revision: revision.to_string(),
            })
        })
    }
}

#[derive(Debug)]
struct MemoryLifecycle {
    storage: Rc<storage::MemoryWorker>,
}

impl ModuleLifecycle for MemoryLifecycle {
    fn prepare(&self, _context: lenso_kernel::PrepareContext) -> ModuleFuture {
        let storage = self.storage.clone();
        Box::pin(async move { storage.verify_ready().await.map_err(memory_runtime_failure) })
    }

    fn activate(&self, _context: ActivateContext) -> ModuleFuture {
        Box::pin(async { Ok(()) })
    }

    fn deactivate(&self, _context: DeactivateContext) -> ModuleFuture {
        let storage = self.storage.clone();
        Box::pin(async move { storage.stop().await.map_err(memory_runtime_failure) })
    }
}

#[derive(Debug, Deserialize)]
struct MemoryConfiguration {
    storage_path: std::path::PathBuf,
}

#[derive(Debug)]
pub struct MemoryFactory;

impl NativeModuleFactory for MemoryFactory {
    fn package_id(&self) -> &'static str {
        MEMORY_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let configuration: MemoryConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("agent memory configuration is invalid: {error}"),
            })?;
        if configuration.storage_path.as_os_str().is_empty() {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "agent memory requires storage_path".to_owned(),
            });
        }
        let storage = Rc::new(
            storage::MemoryWorker::spawn(configuration.storage_path).map_err(|error| {
                RuntimeFailure::Internal {
                    detail: error.to_string(),
                }
            })?,
        );
        Ok(NativeModuleInstance::with_lifecycle(
            vec![Rc::new(lenso_capability_agent_memory::MemoryEndpoint::new(
                MemoryProvider {
                    storage: storage.clone(),
                },
            ))],
            MemoryLifecycle { storage },
        ))
    }
}

fn memory_runtime_failure(error: storage::MemoryWorkerError) -> RuntimeFailure {
    match error {
        storage::MemoryWorkerError::Busy
        | storage::MemoryWorkerError::Storage(storage::MemoryStorageError::ResourceLimit {
            ..
        }) => RuntimeFailure::ResourceExhausted {
            capability: lenso_capability_agent_memory::CAPABILITY_ID,
            operation: "storage".to_owned(),
        },
        storage::MemoryWorkerError::Stopped => RuntimeFailure::Unavailable {
            capability: lenso_capability_agent_memory::CAPABILITY_ID,
        },
        storage::MemoryWorkerError::Storage(error) => RuntimeFailure::Internal {
            detail: error.to_string(),
        },
    }
}
