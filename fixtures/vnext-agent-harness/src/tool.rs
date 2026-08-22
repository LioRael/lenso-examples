use std::rc::Rc;

use futures::future::LocalBoxFuture;
use lenso_capability_agent_tool::{
    ExecuteError, ExecuteRequest, ToolEndpoint, ToolInvocationError,
};
use lenso_kernel::{InvocationContext, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};

use crate::{TOOL_NAME, UPPERCASE_TOOL_PACKAGE_ID};

#[derive(Clone, Copy, Debug)]
enum ToolStyle {
    Echo,
    Uppercase,
}

#[derive(Clone, Copy, Debug)]
struct ToolProviderImpl {
    style: ToolStyle,
}

impl lenso_capability_agent_tool::ToolProvider for ToolProviderImpl {
    fn execute(
        &self,
        _context: InvocationContext,
        request: ExecuteRequest,
    ) -> LocalBoxFuture<
        'static,
        Result<lenso_capability_agent_tool::ExecuteResponse, ToolInvocationError>,
    > {
        let result = if request.name != TOOL_NAME {
            Err(ToolInvocationError::Domain(ExecuteError::UnknownTool))
        } else if request.input.is_empty() {
            Err(ToolInvocationError::Domain(ExecuteError::InvalidInput))
        } else if request.input == "tool-domain-error" {
            Err(ToolInvocationError::Domain(ExecuteError::Denied))
        } else if request.input == "tool-runtime-failure" {
            Err(ToolInvocationError::Runtime(
                RuntimeFailure::ModuleFailure {
                    detail: "selected tool provider failed while executing the tool".to_owned(),
                },
            ))
        } else {
            let output = match self.style {
                ToolStyle::Echo => format!("echo tool: {}", request.input),
                ToolStyle::Uppercase => {
                    format!("uppercase tool: {}", request.input.to_uppercase())
                }
            };
            Ok(lenso_capability_agent_tool::ExecuteResponse { output })
        };
        Box::pin(async move { result })
    }
}

#[derive(Debug)]
pub(crate) struct ToolFactory {
    package_id: &'static str,
    style: ToolStyle,
}

impl NativeModuleFactory for ToolFactory {
    fn package_id(&self) -> &'static str {
        self.package_id
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(vec![Rc::new(ToolEndpoint::new(
            ToolProviderImpl { style: self.style },
        ))]))
    }
}

pub(crate) fn tool_factory(package_id: &'static str) -> ToolFactory {
    let style = if package_id == UPPERCASE_TOOL_PACKAGE_ID {
        ToolStyle::Uppercase
    } else {
        ToolStyle::Echo
    };
    ToolFactory { package_id, style }
}
