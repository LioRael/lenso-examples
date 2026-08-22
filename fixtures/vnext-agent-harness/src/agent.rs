use std::{cell::RefCell, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_capability_agent::{
    AgentEndpoint, AgentInvocationError, AgentProvider, RunError, RunRequest, RunResponse,
};
use lenso_capability_agent_memory::{AppendRequest, MemoryAppendInvocationError, MemoryClient};
use lenso_capability_agent_model::{
    CompleteRequest, ModelClient, ModelEvent, ModelInvocationError,
};
use lenso_capability_agent_progress::{ProgressClient, UpdateRequest};
use lenso_capability_agent_tool::{ExecuteRequest, ToolInvocationError};
use lenso_kernel::{
    ActivateContext, DeactivateContext, InvocationContext, ModuleFuture, ModuleLifecycle,
    NativeRequestEndpoint, RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use serde::Deserialize;

use crate::AGENT_PACKAGE_ID;

#[derive(Clone, Debug)]
struct AgentDependencies {
    model: Rc<ModelClient>,
    tool: Rc<lenso_capability_agent_tool::ToolClient>,
    memory: Rc<MemoryClient>,
    progress: Rc<ProgressClient>,
}

#[derive(Debug, Default)]
struct AgentRuntime {
    dependencies: RefCell<Option<AgentDependencies>>,
}

impl AgentRuntime {
    fn dependencies(&self) -> Result<AgentDependencies, RuntimeFailure> {
        self.dependencies
            .borrow()
            .clone()
            .ok_or_else(|| RuntimeFailure::ModuleFailure {
                detail: "agent harness dependencies are not active".to_owned(),
            })
    }
}

#[derive(Debug)]
struct AgentLifecycle {
    runtime: Rc<AgentRuntime>,
}

impl ModuleLifecycle for AgentLifecycle {
    fn prepare(&self, _context: lenso_kernel::PrepareContext) -> ModuleFuture {
        Box::pin(async { Ok(()) })
    }

    fn activate(&self, context: ActivateContext) -> ModuleFuture {
        let runtime = self.runtime.clone();
        let dependencies = context.dependencies().clone();
        Box::pin(async move {
            let resolved = AgentDependencies {
                model: Rc::new(ModelClient::from_dependencies(&dependencies)?),
                tool: Rc::new(lenso_capability_agent_tool::ToolClient::from_dependencies(
                    &dependencies,
                )?),
                memory: Rc::new(MemoryClient::from_dependencies(&dependencies)?),
                progress: Rc::new(ProgressClient::from_dependencies(&dependencies)?),
            };
            runtime.dependencies.replace(Some(resolved));
            Ok(())
        })
    }

    fn deactivate(&self, _context: DeactivateContext) -> ModuleFuture {
        let runtime = self.runtime.clone();
        Box::pin(async move {
            runtime.dependencies.borrow_mut().take();
            Ok(())
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct AgentConfiguration {
    memory_key: String,
    max_model_output_bytes: usize,
    tool_name: String,
}

#[derive(Debug)]
struct AgentProviderImpl {
    runtime: Rc<AgentRuntime>,
    configuration: AgentConfiguration,
}

impl AgentProvider for AgentProviderImpl {
    fn run(
        &self,
        context: InvocationContext,
        request: RunRequest,
    ) -> LocalBoxFuture<'static, Result<RunResponse, AgentInvocationError>> {
        let runtime = self.runtime.clone();
        let configuration = self.configuration.clone();
        Box::pin(async move { run_agent(runtime, configuration, context, request).await })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the linear harness flow keeps stage ordering and failure propagation visible"
)]
async fn run_agent(
    runtime: Rc<AgentRuntime>,
    configuration: AgentConfiguration,
    context: InvocationContext,
    request: RunRequest,
) -> Result<RunResponse, AgentInvocationError> {
    if request.prompt.trim().is_empty() {
        return Err(AgentInvocationError::Domain(RunError::InvalidPrompt));
    }
    let RunRequest { prompt, run_id } = request;
    let dependencies = runtime
        .dependencies()
        .map_err(AgentInvocationError::Runtime)?;
    let progress = dependencies.progress;
    let progress_event = |stage: &str, detail: &str| UpdateRequest {
        detail: detail.to_owned(),
        run_id: run_id.clone(),
        stage: stage.to_owned(),
    };
    let _ = progress
        .update_with_context(
            context.clone(),
            progress_event("started", "agent run started"),
        )
        .await;

    let memory = dependencies.memory;
    let memory_entries = match memory
        .read_with_context(
            context.clone(),
            lenso_capability_agent_memory::ReadRequest {
                key: configuration.memory_key.clone(),
            },
        )
        .await
    {
        Ok(response) => response.entries,
        Err(lenso_capability_agent_memory::MemoryReadInvocationError::Domain(
            lenso_capability_agent_memory::ReadError::MissingKey,
        )) => Vec::new(),
        Err(lenso_capability_agent_memory::MemoryReadInvocationError::Domain(_)) => {
            return Err(AgentInvocationError::Domain(RunError::MemoryRejected));
        }
        Err(lenso_capability_agent_memory::MemoryReadInvocationError::Runtime(error)) => {
            return Err(AgentInvocationError::Runtime(error));
        }
    };
    let _ = progress
        .update_with_context(
            context.clone(),
            progress_event("memory", "durable memory loaded"),
        )
        .await;

    let tool = dependencies.tool;
    let tool_response = match tool
        .execute_with_context(
            context.clone(),
            ExecuteRequest {
                input: prompt.clone(),
                name: configuration.tool_name,
            },
        )
        .await
    {
        Ok(response) => response.output,
        Err(ToolInvocationError::Domain(_)) => {
            return Err(AgentInvocationError::Domain(RunError::ToolRejected));
        }
        Err(ToolInvocationError::Runtime(error)) => {
            return Err(AgentInvocationError::Runtime(error));
        }
    };
    let _ = progress
        .update_with_context(
            context.clone(),
            progress_event("tool", "tool invocation completed"),
        )
        .await;

    let model = dependencies.model;
    let stream = match model
        .complete_with_context(
            context.clone(),
            CompleteRequest {
                memory: memory_entries,
                prompt,
                tool_output: tool_response,
            },
        )
        .await
    {
        Ok(stream) => stream,
        Err(ModelInvocationError::Domain(_)) => {
            return Err(AgentInvocationError::Domain(RunError::ModelRejected));
        }
        Err(ModelInvocationError::Runtime(error)) => {
            return Err(AgentInvocationError::Runtime(error));
        }
    };
    let mut text = String::new();
    loop {
        match stream.receive().await {
            Ok(ModelEvent::Message(message)) => {
                let within_limit = text
                    .len()
                    .checked_add(message.delta.len())
                    .is_some_and(|length| length <= configuration.max_model_output_bytes);
                if !within_limit {
                    stream.cancel();
                    return Err(AgentInvocationError::Runtime(
                        RuntimeFailure::ResourceExhausted {
                            capability: lenso_capability_agent_model::CAPABILITY_ID,
                            operation: lenso_capability_agent_model::COMPLETE_OPERATION.to_owned(),
                        },
                    ));
                }
                text.push_str(&message.delta);
                let _ = progress
                    .update_with_context(
                        context.clone(),
                        progress_event("model", "streamed model output"),
                    )
                    .await;
            }
            Ok(ModelEvent::PeerHalfClosed) => {
                let _ = progress
                    .update_with_context(
                        context.clone(),
                        progress_event("model_half_closed", "model send side closed"),
                    )
                    .await;
            }
            Ok(ModelEvent::Terminal(Ok(()))) => break,
            Ok(ModelEvent::Terminal(Err(_))) => {
                return Err(AgentInvocationError::Domain(RunError::ModelRejected));
            }
            Err(error) => return Err(AgentInvocationError::Runtime(error)),
        }
    }

    let memory_response = match memory
        .append_with_context(
            context.clone(),
            AppendRequest {
                entry: format!("{run_id} => {text}"),
                key: configuration.memory_key,
            },
        )
        .await
    {
        Ok(response) => response,
        Err(MemoryAppendInvocationError::Domain(_)) => {
            return Err(AgentInvocationError::Domain(RunError::MemoryRejected));
        }
        Err(MemoryAppendInvocationError::Runtime(error)) => {
            return Err(AgentInvocationError::Runtime(error));
        }
    };
    let _ = progress
        .update_with_context(context, progress_event("completed", "agent run completed"))
        .await;
    Ok(RunResponse {
        revision: memory_response.revision,
        text,
    })
}

#[derive(Debug)]
pub(crate) struct AgentFactory;

impl NativeModuleFactory for AgentFactory {
    fn package_id(&self) -> &'static str {
        AGENT_PACKAGE_ID
    }

    fn instantiate(
        &self,
        context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let configuration: AgentConfiguration = serde_json::from_str(context.configuration())
            .map_err(|error| RuntimeFailure::InvalidResolvedPlan {
                detail: format!("agent harness configuration is invalid: {error}"),
            })?;
        if configuration.memory_key.is_empty()
            || configuration.max_model_output_bytes == 0
            || configuration.tool_name.is_empty()
        {
            return Err(RuntimeFailure::InvalidResolvedPlan {
                detail: "agent harness requires memory_key, max_model_output_bytes, and tool_name"
                    .to_owned(),
            });
        }
        let runtime = Rc::new(AgentRuntime::default());
        Ok(NativeModuleInstance::with_lifecycle(
            vec![Rc::new(AgentEndpoint::new(AgentProviderImpl {
                runtime: runtime.clone(),
                configuration,
            })) as Rc<dyn NativeRequestEndpoint>],
            AgentLifecycle { runtime },
        ))
    }
}
