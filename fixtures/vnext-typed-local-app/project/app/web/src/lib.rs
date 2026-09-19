use lenso_capability_http_endpoint::{prelude::*, response};
use local_text_contract::{self as text, UppercaseRequest};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Uppercase {
    text: String,
}

#[lenso::plugin]
#[derive(Clone, Debug)]
struct Web {
    #[dependency(id = "uppercase")]
    uppercase: text::TextClient,
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
        let response = self
            .uppercase
            .uppercase(UppercaseRequest { text: input.text })
            .await
            .map_err(|error| {
                Problem::new(
                    StatusCode::BAD_GATEWAY,
                    "uppercase_failed",
                    format!("{error:?}"),
                )
            })?;
        Ok(Json(serde_json::json!({"text":response.text})))
    }
}
