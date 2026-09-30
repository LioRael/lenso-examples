use example_ops_state_contract::{self as state, ReadRequest, UpdateRequest};
use lenso_capability_http_endpoint::{
    prelude::*,
    response::{Problem, StatusCode},
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatePath {
    instance: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptPath {
    instance: String,
    key: String,
}

#[lenso::plugin(request_queue_capacity = 16, request_max_concurrency = 2)]
#[derive(Clone, Debug)]
struct OpsWeb {
    #[dependency(id = "primary")]
    primary: state::OpsStateClient,
    #[dependency(id = "secondary")]
    secondary: state::OpsStateClient,
    #[dependency(id = "cache")]
    cache: Option<state::OpsStateClient>,
}

impl OpsWeb {
    fn client(&self, instance: &str) -> Result<&state::OpsStateClient, Problem> {
        match instance {
            "primary" => Ok(&self.primary),
            "secondary" => Ok(&self.secondary),
            _ => Err(Problem::new(
                StatusCode::NOT_FOUND,
                "unknown_instance",
                "Select primary or secondary",
            )),
        }
    }
}

#[endpoint]
impl OpsWeb {
    #[get("ops.cache.selection", "/cache")]
    async fn cache_selection(&self) -> Result<Json<serde_json::Value>, Problem> {
        Ok(Json(serde_json::json!({"enabled":self.cache.is_some()})))
    }

    #[get("ops.state.receipt", "/receipts/{instance}/{key}")]
    async fn receipt(
        &self,
        Path(path): Path<ReceiptPath>,
    ) -> Result<Json<state::ReceiptResponse>, Problem> {
        self.client(&path.instance)?
            .receipt(state::ReceiptRequest {
                idempotency_key: path.key,
            })
            .await
            .map(Json)
            .map_err(|_| {
                Problem::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "receipt_unavailable",
                    "Receipt query is unavailable",
                )
            })
    }

    #[get("ops.state.read", "/state/{instance}")]
    async fn read(
        &self,
        Path(path): Path<StatePath>,
    ) -> Result<Json<state::ReadResponse>, Problem> {
        self.client(&path.instance)?
            .read(ReadRequest {})
            .await
            .map(Json)
            .map_err(|error| {
                Problem::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "state_unavailable",
                    format!("{error:?}"),
                )
            })
    }

    #[post("ops.state.update", "/state/{instance}")]
    async fn update(
        &self,
        Path(path): Path<StatePath>,
        Json(input): Json<UpdateRequest>,
    ) -> Result<Json<state::UpdateResponse>, Problem> {
        self.client(&path.instance)?
            .update(input)
            .await
            .map(Json)
            .map_err(|error| {
                let status = match &error {
                    state::OpsStateUpdateInvocationError::Domain(
                        state::UpdateError::StaleRevision | state::UpdateError::IdempotencyConflict,
                    ) => StatusCode::CONFLICT,
                    state::OpsStateUpdateInvocationError::Domain(
                        state::UpdateError::InvalidInput,
                    ) => StatusCode::BAD_REQUEST,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                };
                Problem::new(status, "state_update_failed", format!("{error:?}"))
            })
    }
}
