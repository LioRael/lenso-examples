use bytes::Bytes;
use http::Request;
use lenso_kernel::RuntimeFailure;
use serde::{Deserialize, Serialize};

use crate::failure;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub method: String,
    #[serde(default = "default_uri")]
    pub uri: String,
    #[serde(default = "default_content_type")]
    pub content_type: String,
    pub principal: String,
    pub body: String,
}

fn default_uri() -> String {
    "/settings".into()
}
fn default_content_type() -> String {
    "application/json".into()
}

impl Input {
    pub fn request(self) -> Result<Request<Bytes>, RuntimeFailure> {
        if self.body.len() > 4096 || self.principal.len() > 128 {
            return Err(failure("fixture request exceeds bound"));
        }
        Request::builder()
            .method(self.method.as_str())
            .uri(self.uri)
            .header("content-type", self.content_type)
            .header("x-local-test-principal", self.principal)
            .body(Bytes::from(self.body))
            .map_err(failure)
    }
}

#[derive(Serialize)]
pub struct Output {
    pub status: u16,
    pub body: serde_json::Value,
}

impl Output {
    pub fn from_response(response: http::Response<Bytes>) -> Result<Self, RuntimeFailure> {
        Ok(Self {
            status: response.status().as_u16(),
            body: serde_json::from_slice(response.body()).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(response.body()).into_owned())
            }),
        })
    }
}
