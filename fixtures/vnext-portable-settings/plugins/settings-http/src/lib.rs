use contract::*;
use example_settings_contract as contract;
use http_endpoint_contract::{EndpointHandleInvocationError, HandleResponse, Headers, Json};
use lenso::Port;
use lenso_capability_http_endpoint::{self as http_endpoint_contract, endpoint};
use lenso_kernel::InvocationContext;
use serde_json::json;

#[lenso::plugin]
#[derive(Clone, Debug)]
pub struct SettingsHttp {
    store: Port<contract::SettingsStoreClient>,
}

fn response(status: u16, body: serde_json::Value) -> HandleResponse {
    HandleResponse {
        status: status.into(),
        headers: vec![],
        body: body.to_string().into_bytes().into(),
    }
}

fn principal(headers: &Headers) -> String {
    // A local-test key, not authenticated identity.
    headers
        .get("x-local-test-principal")
        .unwrap_or_default()
        .to_owned()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    expected_revision: i64,
    idempotency_key: String,
    value: String,
}

#[endpoint]
impl SettingsHttp {
    #[get("read", "/settings")]
    async fn read(
        &self,
        context: InvocationContext,
        headers: Headers,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        match self
            .store
            .read_with_context(
                context,
                ReadRequest {
                    principal: principal(&headers),
                },
            )
            .await
        {
            Ok(value) => Ok(response(200, json!(value))),
            Err(SettingsStoreReadInvocationError::Domain(error)) => {
                Ok(response(400, json!({"error": error})))
            }
            Err(SettingsStoreReadInvocationError::Runtime(error)) => {
                Err(EndpointHandleInvocationError::Runtime(error))
            }
        }
    }

    #[put("change", "/settings")]
    async fn change(
        &self,
        context: InvocationContext,
        headers: Headers,
        Json(body): Json<ChangeBody>,
    ) -> Result<HandleResponse, EndpointHandleInvocationError> {
        match self
            .store
            .change_with_context(
                context,
                ChangeRequest {
                    principal: principal(&headers),
                    expected_revision: body.expected_revision,
                    idempotency_key: body.idempotency_key,
                    value: body.value,
                },
            )
            .await
        {
            Ok(value) => Ok(response(200, json!(value))),
            Err(SettingsStoreChangeInvocationError::Domain(error)) => {
                let status = if matches!(error, ChangeError::Invalid) {
                    400
                } else {
                    409
                };
                Ok(response(status, json!({"error": error})))
            }
            Err(SettingsStoreChangeInvocationError::Runtime(error)) => {
                Err(EndpointHandleInvocationError::Runtime(error))
            }
        }
    }
}
