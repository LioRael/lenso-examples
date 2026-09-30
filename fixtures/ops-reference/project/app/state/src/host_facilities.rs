use example_ops_state_contract::{
    self as state, ReadResponse, ReceiptRequest, ReceiptResponse, UpdateRequest, UpdateResponse,
};
use std::fmt;

#[derive(Clone)]
pub struct StateHandle(Backend);

#[derive(Clone)]
enum Backend {
    Simulated,
    #[cfg(not(target_arch = "wasm32"))]
    Postgres(lenso_ops_reference_pg::PgStateHandle),
    #[cfg(target_arch = "wasm32")]
    Worker(wasm_bindgen::JsValue),
}

impl fmt::Debug for StateHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StateHandle")
            .field("profile", &self.profile())
            .finish()
    }
}

fn invalid() -> lenso::RuntimeFailure {
    lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "invalid Ops State Host facility".into(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn state(binding: &serde_json::Value) -> Result<StateHandle, lenso::RuntimeFailure> {
    #[derive(serde::Deserialize)]
    #[serde(tag = "profile", deny_unknown_fields)]
    enum Binding {
        #[serde(rename = "simulated")]
        Simulated,
        #[serde(rename = "native-pg")]
        Postgres {
            connection_uri_file: std::path::PathBuf,
            schema: String,
        },
    }
    match serde_json::from_value(binding.clone()).map_err(|_| invalid())? {
        Binding::Simulated => Ok(StateHandle(Backend::Simulated)),
        Binding::Postgres {
            connection_uri_file,
            schema,
        } => {
            let handle =
                lenso_ops_reference_pg::PgStateHandle::open(lenso_ops_reference_pg::PgBinding {
                    connection_uri_file,
                    schema,
                })
                .map_err(|_| invalid())?;
            Ok(StateHandle(Backend::Postgres(handle)))
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn state(binding: &wasm_bindgen::JsValue) -> Result<StateHandle, lenso::RuntimeFailure> {
    let profile = js_sys::Reflect::get(binding, &"profile".into())
        .map_err(|_| invalid())?
        .as_string()
        .ok_or_else(invalid)?;
    match profile.as_str() {
        "simulated" => Ok(StateHandle(Backend::Simulated)),
        "workers-d1" => Ok(StateHandle(Backend::Worker(binding.clone()))),
        _ => Err(invalid()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn map_failure(error: lenso_ops_reference_pg::Failure) -> state::UpdateError {
    use lenso_ops_reference_pg::Failure;
    match error {
        Failure::InvalidInput => state::UpdateError::InvalidInput,
        Failure::StaleRevision => state::UpdateError::StaleRevision,
        Failure::IdempotencyConflict => state::UpdateError::IdempotencyConflict,
        Failure::UnknownCommit => state::UpdateError::UnknownCommit,
        _ => state::UpdateError::Unavailable,
    }
}

impl StateHandle {
    pub async fn close(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Backend::Postgres(handle) = &self.0 {
            handle.close().await;
        }
    }

    pub fn profile(&self) -> String {
        match &self.0 {
            Backend::Simulated => "simulated".into(),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Postgres(_) => "native-pg".into(),
            #[cfg(target_arch = "wasm32")]
            Backend::Worker(binding) => js_sys::Reflect::get(binding, &"profile".into())
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_default(),
        }
    }

    pub async fn readiness(&self) -> Result<(), lenso::RuntimeFailure> {
        match &self.0 {
            Backend::Simulated => Ok(()),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Postgres(handle) => {
                handle
                    .readiness()
                    .await
                    .map_err(|error| lenso::RuntimeFailure::PluginFailure {
                        detail: format!("Ops State readiness: {error:?}"),
                    })
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Worker(_) => {
                self.read()
                    .await
                    .map(|_| ())
                    .map_err(|_| lenso::RuntimeFailure::PluginFailure {
                        detail: "Ops State readiness failed".into(),
                    })
            }
        }
    }

    pub async fn read(&self) -> Result<ReadResponse, state::ReadError> {
        match &self.0 {
            Backend::Simulated => Err(state::ReadError::Unavailable),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Postgres(handle) => {
                let response = handle
                    .read()
                    .await
                    .map_err(|_| state::ReadError::Unavailable)?;
                Ok(ReadResponse {
                    label: response.label,
                    value: response.value,
                    revision: response.revision,
                })
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Worker(binding) => worker_call(binding, "read", &serde_json::json!({}))
                .await
                .map_err(|_| state::ReadError::Unavailable),
        }
    }

    pub async fn update(
        &self,
        request: UpdateRequest,
    ) -> Result<UpdateResponse, state::UpdateError> {
        match &self.0 {
            Backend::Simulated => Err(state::UpdateError::Unavailable),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Postgres(handle) => {
                let response = handle
                    .update(
                        request.value,
                        request.expected_revision,
                        &request.idempotency_key,
                    )
                    .await
                    .map_err(map_failure)?;
                Ok(UpdateResponse {
                    label: response.snapshot.label,
                    value: response.snapshot.value,
                    revision: response.snapshot.revision,
                    receipt_id: response.receipt_id,
                })
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Worker(binding) => worker_call(binding, "update", &request).await,
        }
    }
    pub async fn receipt(
        &self,
        request: ReceiptRequest,
    ) -> Result<ReceiptResponse, state::ReceiptError> {
        match &self.0 {
            Backend::Simulated => Err(state::ReceiptError::Unavailable),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Postgres(handle) => {
                let response = handle
                    .receipt(&request.idempotency_key)
                    .await
                    .map_err(|error| {
                        if error == lenso_ops_reference_pg::Failure::InvalidInput {
                            state::ReceiptError::InvalidInput
                        } else {
                            state::ReceiptError::Unavailable
                        }
                    })?;
                Ok(match response {
                    Some(response) => ReceiptResponse {
                        found: true,
                        label: Some(Some(response.snapshot.label)),
                        value: Some(Some(response.snapshot.value)),
                        revision: Some(Some(response.snapshot.revision)),
                        receipt_id: Some(Some(response.receipt_id)),
                    },
                    None => ReceiptResponse {
                        found: false,
                        label: None,
                        value: None,
                        revision: None,
                        receipt_id: None,
                    },
                })
            }
            #[cfg(target_arch = "wasm32")]
            Backend::Worker(binding) => {
                worker_call(binding, "receipt", &request)
                    .await
                    .map_err(|error| match error {
                        state::UpdateError::InvalidInput => state::ReceiptError::InvalidInput,
                        _ => state::ReceiptError::Unavailable,
                    })
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
async fn worker_call<T: serde::de::DeserializeOwned>(
    binding: &wasm_bindgen::JsValue,
    method: &str,
    request: &impl serde::Serialize,
) -> Result<T, state::UpdateError> {
    use wasm_bindgen::JsCast;
    let call = js_sys::Reflect::get(binding, &method.into())
        .ok()
        .and_then(|value| value.dyn_into::<js_sys::Function>().ok())
        .ok_or(state::UpdateError::Unavailable)?;
    let input =
        serde_wasm_bindgen::to_value(request).map_err(|_| state::UpdateError::InvalidInput)?;
    let result = call
        .call1(binding, &input)
        .map_err(|_| state::UpdateError::Unavailable)?;
    let result = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&result))
        .await
        .map_err(|_| state::UpdateError::UnknownCommit)?;
    let result: serde_json::Value = serde_wasm_bindgen::from_value(result).map_err(|_| {
        if method == "update" {
            state::UpdateError::UnknownCommit
        } else {
            state::UpdateError::Unavailable
        }
    })?;
    if let Some(payload) = result.get("ok") {
        return serde_json::from_value(payload.clone()).map_err(|_| {
            if method == "update" {
                state::UpdateError::UnknownCommit
            } else {
                state::UpdateError::Unavailable
            }
        });
    }
    Err(match result.get("error").and_then(|value| value.as_str()) {
        Some("invalid_input") => state::UpdateError::InvalidInput,
        Some("stale_revision") => state::UpdateError::StaleRevision,
        Some("idempotency_conflict") => state::UpdateError::IdempotencyConflict,
        Some("unknown_commit") => state::UpdateError::UnknownCommit,
        _ => state::UpdateError::Unavailable,
    })
}
