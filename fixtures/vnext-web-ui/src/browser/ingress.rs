use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri,
        header::{AUTHORIZATION, CONTENT_TYPE, COOKIE},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use lenso_kernel::CancellationToken;
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot, watch},
};
use tower::{ServiceBuilder, limit::GlobalConcurrencyLimitLayer};
use tower_http::{
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestId, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    set_header::SetResponseHeaderLayer,
};

const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024;
const MAX_REQUEST_HEAD_BYTES: usize = 16 * 1024;
const MAX_CONCURRENT_REQUESTS: usize = 32;
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const NOSNIFF_HEADER: HeaderName = HeaderName::from_static("x-content-type-options");

#[derive(Debug)]
pub(super) struct IngressRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) token: Option<String>,
    pub(super) body: String,
}

#[derive(Debug)]
pub(super) struct IngressResponse {
    status: u16,
    content_type: String,
    body: String,
}

impl IngressResponse {
    pub(super) fn text(
        status: u16,
        content_type: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
        }
    }

    pub(super) fn json(status: u16, body: impl Into<String>) -> Self {
        Self::text(status, "application/json; charset=utf-8", body)
    }

    pub(super) fn capability(status: u16, result: &serde_json::Value) -> Self {
        Self::json(
            status,
            serde_json::to_string(result).expect("Capability result serializes"),
        )
    }

    fn into_http(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let content_type = HeaderValue::from_str(&self.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
        (status, [(CONTENT_TYPE, content_type)], self.body).into_response()
    }
}

#[derive(Debug)]
struct IngressCall {
    request: IngressRequest,
    response: oneshot::Sender<IngressResponse>,
}

#[derive(Clone, Debug)]
struct IngressBridge {
    sender: mpsc::Sender<IngressCall>,
}

#[derive(Clone, Debug, Default)]
struct MakeIngressRequestId {
    next: Arc<AtomicU64>,
}

impl MakeRequestId for MakeIngressRequestId {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let value = self.next.fetch_add(1, Ordering::Relaxed);
        let value = HeaderValue::from_str(&format!("lenso-{value}"))
            .expect("generated request id is a valid header value");
        Some(RequestId::new(value))
    }
}

pub(super) async fn serve<H, F>(listener: TcpListener, handler: H, cancellation: CancellationToken)
where
    H: Fn(IngressRequest) -> F,
    F: Future<Output = IngressResponse>,
{
    let (sender, receiver) = mpsc::channel(MAX_CONCURRENT_REQUESTS);
    let middleware = ServiceBuilder::new()
        .layer(SetSensitiveRequestHeadersLayer::new([
            AUTHORIZATION,
            COOKIE,
        ]))
        .layer(SetRequestIdLayer::new(
            REQUEST_ID_HEADER.clone(),
            MakeIngressRequestId::default(),
        ))
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        .layer(SetResponseHeaderLayer::overriding(
            NOSNIFF_HEADER,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
        .layer(GlobalConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS));
    let router = Router::new()
        .fallback(forward)
        .with_state(IngressBridge { sender })
        .layer(middleware::from_fn(enforce_head_limit))
        .layer(middleware);
    let (shutdown, mut shutdown_signal) = watch::channel(false);
    let cancellation_bridge = async move {
        cancellation.cancelled().await;
        let _ = shutdown.send(true);
    };
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        while !*shutdown_signal.borrow_and_update() {
            if shutdown_signal.changed().await.is_err() {
                return;
            }
        }
    });
    let dispatcher = dispatch_requests(receiver, handler);
    let (server, (), ()) = tokio::join!(server, dispatcher, cancellation_bridge);
    let _ = server;
}

async fn forward(
    State(bridge): State<IngressBridge>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let token = match bearer_token(&headers) {
        Ok(token) => token,
        Err(response) => return response.into_http(),
    };
    let Ok(body) = String::from_utf8(body.to_vec()) else {
        return bad_request().into_http();
    };
    let (response, receive) = oneshot::channel();
    let call = IngressCall {
        request: IngressRequest {
            method: method.as_str().to_owned(),
            path: uri.path().to_owned(),
            token,
            body,
        },
        response,
    };
    if bridge.sender.send(call).await.is_err() {
        return unavailable().into_http();
    }
    receive.await.unwrap_or_else(|_| unavailable()).into_http()
}

async fn dispatch_requests<H, F>(mut receiver: mpsc::Receiver<IngressCall>, handler: H)
where
    H: Fn(IngressRequest) -> F,
    F: Future<Output = IngressResponse>,
{
    loop {
        let call = receiver.recv().await;
        let Some(call) = call else {
            return;
        };
        let response = handler(call.request).await;
        let _ = call.response.send(response);
    }
}

async fn enforce_head_limit(request: Request<Body>, next: Next) -> Response {
    let size = request.method().as_str().len()
        + request.uri().to_string().len()
        + request
            .headers()
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>();
    if size > MAX_REQUEST_HEAD_BYTES {
        return IngressResponse::json(431, r#"{"error":"request_header_fields_too_large"}"#)
            .into_http();
    }
    next.run(request).await
}

fn bearer_token(headers: &HeaderMap) -> Result<Option<String>, IngressResponse> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(bad_request());
    }
    let value = value.to_str().map_err(|_| bad_request())?;
    let token = value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .ok_or_else(bad_request)?;
    Ok(Some(token.to_owned()))
}

fn bad_request() -> IngressResponse {
    IngressResponse::json(400, r#"{"error":"bad_request"}"#)
}

fn unavailable() -> IngressResponse {
    IngressResponse::json(503, r#"{"error":"ingress_unavailable"}"#)
}
