use std::rc::Rc;

use lenso_capability_agent_tool::{ExecuteError, ExecuteRequest, Tool, ToolEndpoint};
use lenso_kernel::{InvocationContext, NativeRequestFuture, RuntimeFailure};
use lenso_native_adapter::{NativePluginFactory, NativePluginFactoryContext, NativePluginInstance};

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
    ) -> NativeRequestFuture<Tool> {
        let result = if request.name != TOOL_NAME {
            Ok(Err(ExecuteError::UnknownTool))
        } else if request.input.is_empty() {
            Ok(Err(ExecuteError::InvalidInput))
        } else if request.input == "tool-domain-error" {
            Ok(Err(ExecuteError::Denied))
        } else if request.input == "tool-runtime-failure" {
            Err(RuntimeFailure::PluginFailure {
                detail: "selected tool provider failed while executing the tool".to_owned(),
            })
        } else {
            let output = match self.style {
                ToolStyle::Echo => format!("echo tool: {}", request.input),
                ToolStyle::Uppercase => {
                    format!("uppercase tool: {}", request.input.to_uppercase())
                }
            };
            Ok(Ok(lenso_capability_agent_tool::ExecuteResponse { output }))
        };
        Box::pin(async move { result })
    }
}

#[derive(Debug)]
pub(crate) struct ToolFactory {
    package_id: &'static str,
    style: ToolStyle,
}

impl NativePluginFactory for ToolFactory {
    fn package_id(&self) -> &'static str {
        self.package_id
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::new(vec![Rc::new(ToolEndpoint::new(
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
