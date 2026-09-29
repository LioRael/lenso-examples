#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::{convert::Infallible, rc::Rc, time::Duration};

    use bytes::Bytes;
    use http_body_util::{BodyExt, Full, Limited};
    use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use portable_settings_fixture::{
        contract::{ChangeRequest, ReadRequest},
        postgres::Postgres,
        storage::Persistence,
    };

    fn reply(status: u16, body: String) -> Response<Full<Bytes>> {
        let mut response = Response::new(Full::new(Bytes::from(body)));
        *response.status_mut() = http::StatusCode::from_u16(status).unwrap();
        response
    }

    async fn handle(
        request: Request<Incoming>,
        store: Rc<Postgres>,
        token: Rc<String>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        let authorization = format!("Bearer {token}");
        if request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            != Some(&authorization)
        {
            return Ok(reply(401, "{}".into()));
        }
        if request.method() != http::Method::POST
            || !matches!(request.uri().path(), "/read" | "/change")
            || request.uri().query().is_some()
        {
            return Ok(reply(404, "{}".into()));
        }
        let path = request.uri().path().to_owned();
        let body = match Limited::new(request.into_body(), 4096).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => return Ok(reply(413, "{}".into())),
        };
        let result = match path.as_str() {
            "/read" => match serde_json::from_slice::<ReadRequest>(&body) {
                Ok(request) => store.read(request).await.map(|r| serde_json::to_string(&r)),
                Err(_) => return Ok(reply(400, "{}".into())),
            },
            "/change" => match serde_json::from_slice::<ChangeRequest>(&body) {
                Ok(request) => store
                    .change(request)
                    .await
                    .map(|r| serde_json::to_string(&r)),
                Err(_) => return Ok(reply(400, "{}".into())),
            },
            _ => unreachable!(),
        };
        Ok(match result {
            Ok(Ok(body)) => reply(200, body),
            _ => reply(503, "{}".into()),
        })
    }

    pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
        let store = Rc::new(Postgres::new(std::env::var("SETTINGS_DATABASE_URL")?));
        let token = Rc::new(std::env::var("SETTINGS_BRIDGE_TOKEN")?);
        if token.len() < 32 {
            return Err("bridge token must contain at least 32 bytes".into());
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        println!("http://{}", listener.local_addr()?);
        loop {
            let (socket, _) = listener.accept().await?;
            let store = store.clone();
            let token = token.clone();
            tokio::task::spawn_local(async move {
                let service =
                    service_fn(move |request| handle(request, store.clone(), token.clone()));
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    http1::Builder::new()
                        .max_buf_size(8192)
                        .serve_connection(TokioIo::new(socket), service),
                )
                .await;
            });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::LocalSet::new().run_until(native::run()).await
}

#[cfg(target_arch = "wasm32")]
fn main() {}
