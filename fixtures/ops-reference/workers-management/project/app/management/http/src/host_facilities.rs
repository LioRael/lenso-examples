use lenso_management_http::McpTransport;
use std::rc::Rc;

#[derive(Clone)]
pub struct McpHandle {
    #[cfg(target_arch = "wasm32")]
    object: wasm_bindgen::JsValue,
}
impl std::fmt::Debug for McpHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpHandle(event-scoped)")
    }
}
impl McpHandle {
    pub fn transport(&self, resource: String) -> Result<Rc<dyn McpTransport>, &'static str> {
        #[cfg(target_arch = "wasm32")]
        {
            let transport = lenso_management_http::workers::WorkersMcp::from_binding(
                &self.object, resource,
            )?;
            Ok(Rc::new(transport))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = resource;
            Err("This Source MCP facility requires the Workers profile")
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn mcp(binding: &wasm_bindgen::JsValue) -> Result<McpHandle, lenso::RuntimeFailure> {
    if !binding.is_object() || binding.is_null() {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "MCP requires its explicitly selected event-scoped transport".into(),
        });
    }
    Ok(McpHandle {object: binding.clone()})
}
