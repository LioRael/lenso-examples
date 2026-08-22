use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use futures::future::LocalBoxFuture;
use lenso_capability_agent_model::{
    CompleteError, CompleteRequest, ModelEndpoint, ModelInvocationError,
};
use lenso_kernel::{
    InvocationContext, NativeStreamItem, NativeStreamSession, NoopModuleLifecycle, RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};

#[derive(Clone, Copy, Debug)]
enum ModelStyle {
    Echo,
    Friendly,
}

#[derive(Debug)]
struct ScriptedModelSession {
    messages: RefCell<VecDeque<lenso_capability_agent_model::CompleteResponse>>,
    terminal_seen: Cell<bool>,
    cancelled: Cell<bool>,
    request_id: u64,
}

impl ScriptedModelSession {
    fn new(messages: Vec<String>, request_id: u64) -> Self {
        Self {
            messages: RefCell::new(
                messages
                    .into_iter()
                    .map(|delta| lenso_capability_agent_model::CompleteResponse { delta })
                    .collect(),
            ),
            terminal_seen: Cell::new(false),
            cancelled: Cell::new(false),
            request_id,
        }
    }

    fn cancelled(&self) -> Result<(), RuntimeFailure> {
        if self.cancelled.get() {
            Err(RuntimeFailure::Cancelled {
                request_id: self.request_id,
            })
        } else {
            Ok(())
        }
    }
}

impl NativeStreamSession for ScriptedModelSession {
    fn send(
        &self,
        message: Box<dyn std::any::Any>,
    ) -> LocalBoxFuture<'static, Result<(), RuntimeFailure>> {
        let result = self.cancelled().and_then(|()| {
            message
                .downcast::<lenso_capability_agent_model::CompleteResponse>()
                .map(|_| ())
                .map_err(|_| RuntimeFailure::ProtocolViolation {
                    capability: lenso_capability_agent_model::CAPABILITY_ID,
                })
        });
        Box::pin(async move { result })
    }

    fn receive(&self) -> LocalBoxFuture<'static, Result<NativeStreamItem, RuntimeFailure>> {
        let result = self.cancelled().and_then(|()| {
            if let Some(message) = self.messages.borrow_mut().pop_front() {
                Ok(NativeStreamItem::Message(Box::new(message)))
            } else if !self.terminal_seen.replace(true) {
                Ok(NativeStreamItem::Terminal(Ok(())))
            } else {
                Err(RuntimeFailure::ProtocolViolation {
                    capability: lenso_capability_agent_model::CAPABILITY_ID,
                })
            }
        });
        Box::pin(async move { result })
    }

    fn close_send(&self) -> LocalBoxFuture<'static, Result<(), RuntimeFailure>> {
        let result = self.cancelled();
        Box::pin(async move { result })
    }

    fn cancel(&self) {
        self.cancelled.set(true);
    }
}

#[derive(Clone, Copy, Debug)]
struct ModelProvider {
    style: ModelStyle,
}

impl lenso_capability_agent_model::ModelProvider for ModelProvider {
    fn complete(
        &self,
        context: InvocationContext,
        request: CompleteRequest,
    ) -> LocalBoxFuture<'static, Result<Box<dyn NativeStreamSession>, ModelInvocationError>> {
        if request.prompt.trim().is_empty() {
            return Box::pin(async {
                Err(ModelInvocationError::Domain(CompleteError::InvalidPrompt))
            });
        }
        if request.prompt == "model-domain-error" {
            return Box::pin(async {
                Err(ModelInvocationError::Domain(CompleteError::ContextTooLarge))
            });
        }
        if request.prompt == "model-runtime-failure" {
            return Box::pin(async {
                Err(ModelInvocationError::Runtime(RuntimeFailure::Unavailable {
                    capability: lenso_capability_agent_model::CAPABILITY_ID,
                }))
            });
        }
        if request.prompt == "model-delay" {
            return Box::pin(futures::future::pending());
        }

        let prefix = match self.style {
            ModelStyle::Echo => "echo model: ".to_owned(),
            ModelStyle::Friendly => "friendly model: ".to_owned(),
        };
        let memory = if request.memory.is_empty() {
            "no prior memory".to_owned()
        } else {
            format!("{} prior memories", request.memory.len())
        };
        let messages = if request.prompt == "model-output-overflow" {
            vec!["x".repeat(65 * 1024)]
        } else {
            vec![
                prefix,
                format!(
                    "{} | tool result: {} | {}",
                    request.prompt, request.tool_output, memory
                ),
            ]
        };
        let request_id = context.request_id();
        Box::pin(async move {
            Ok(Box::new(ScriptedModelSession::new(messages, request_id))
                as Box<dyn NativeStreamSession>)
        })
    }
}

#[derive(Debug)]
pub(crate) struct ModelFactory {
    package_id: &'static str,
    style: ModelStyle,
}

impl NativeModuleFactory for ModelFactory {
    fn package_id(&self) -> &'static str {
        self.package_id
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::with_stream_endpoints(
            vec![Rc::new(ModelEndpoint::new(ModelProvider {
                style: self.style,
            }))],
            NoopModuleLifecycle,
        ))
    }
}

pub(crate) fn model_factory(package_id: &'static str) -> ModelFactory {
    let style = if package_id == crate::FRIENDLY_MODEL_PACKAGE_ID {
        ModelStyle::Friendly
    } else {
        ModelStyle::Echo
    };
    ModelFactory { package_id, style }
}
