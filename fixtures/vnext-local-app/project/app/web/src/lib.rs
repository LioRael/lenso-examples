use lenso_capability_agent_tool_provider::{self as tools, ExecuteRequest};
use lenso_capability_http_endpoint::{prelude::*, response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Uppercase {
    text: String,
}

#[lenso::plugin]
#[derive(Clone, Debug)]
struct Web {
    #[dependency(id = "uppercase")]
    uppercase: tools::ToolProviderClient,
}

#[endpoint]
impl Web {
    #[get("example.home", "/")]
    async fn home(&self) -> Result<HandleResponse, Problem> {
        let mut response = response::text(StatusCode::OK, include_str!("../public/index.html"));
        response.headers[0].value = "text/html; charset=utf-8".into();
        Ok(response)
    }

    #[post("example.uppercase", "/uppercase")]
    async fn uppercase(
        &self,
        Json(input): Json<Uppercase>,
    ) -> Result<Json<serde_json::Value>, Problem> {
        let arguments = serde_json::to_string(&input)
            .map_err(|e| Problem::new(StatusCode::BAD_REQUEST, "invalid_input", e.to_string()))?;
        let response = self
            .uppercase
            .execute(ExecuteRequest {
                name: "example.uppercase".into(),
                arguments_json: arguments.try_into().map_err(|e| {
                    Problem::new(StatusCode::BAD_REQUEST, "invalid_input", format!("{e:?}"))
                })?,
            })
            .await
            .map_err(|e| {
                Problem::new(
                    StatusCode::BAD_GATEWAY,
                    "uppercase_failed",
                    format!("{e:?}"),
                )
            })?;
        Ok(Json(
            serde_json::json!({"text":serde_json::from_str::<String>(&response.content).unwrap_or(response.content)}),
        ))
    }
}
