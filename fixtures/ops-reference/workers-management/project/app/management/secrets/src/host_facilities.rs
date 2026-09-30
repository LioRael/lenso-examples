
#[derive(Clone)]
pub struct SecretHandle {
    #[cfg(target_arch = "wasm32")]
    object: wasm_bindgen::JsValue,
}
impl std::fmt::Debug for SecretHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretHandle(Host-selected)")
    }
}
impl SecretHandle {
    pub async fn resolve(&self, reference: &str, caller: &str) -> Result<Option<String>, lenso::RuntimeFailure> {
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast as _;
            let method = js_sys::Reflect::get(&self.object, &"resolve".into()).ok()
                .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
                .ok_or(unavailable())?;
            let reply = method.call2(&self.object, &reference.into(), &caller.into())
                .map_err(|_| unavailable())?;
            let value = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&reply))
                .await.map_err(|_| unavailable())?;
            Ok(value.as_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (reference, caller);
            Err(unavailable())
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn secrets(binding: &wasm_bindgen::JsValue) -> Result<SecretHandle, lenso::RuntimeFailure> {
    if !binding.is_object() || binding.is_null() {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "Secrets requires its explicitly selected private binding".into(),
        });
    }
    Ok(SecretHandle {object: binding.clone()})
}

fn unavailable() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::PluginFailure {detail: "Selected Secrets facility is unavailable".into()}
}
