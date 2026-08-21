use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    process::Command,
    rc::Rc,
    time::Duration,
};

use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_auth::{AuthClient, AuthInvocationError, AuthenticateError};
use lenso_capability_secure_greeting::{
    CAPABILITY_ID as GREETING_CAPABILITY_ID, DESCRIPTOR_VERSION as GREETING_DESCRIPTOR_VERSION,
    GREET_OPERATION, GreetError, GreetRequest, SecureGreetingClient, SecureGreetingInvocationError,
};
use lenso_capability_web_shell::{
    ReadAssetRequest, RenderRouteRequest, RenderRouteResponse, ShellClient,
    ShellReadAssetInvocationError, ShellRenderRouteInvocationError,
};
use lenso_kernel::{
    ActivateContext, CancellationToken, ModuleDependencies, ModuleLifecycle, PrepareContext,
    RuntimeFailure,
};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const BROWSER_PACKAGE_ID: &str = "lenso.browser-adapter";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const GREETING_ROUTE: &str = "/api/capabilities/example.secure-greeting@1/greet";

#[derive(Debug, Default)]
struct BrowserObserver {
    local_addr: Cell<Option<SocketAddr>>,
    launched_url: RefCell<Option<String>>,
    launch_error: RefCell<Option<String>>,
}

#[derive(Debug, Default)]
struct BrowserGeneration {
    listener: RefCell<Option<TcpListener>>,
}

#[derive(Clone, Debug)]
pub struct BrowserAdapterFactory {
    observer: Rc<BrowserObserver>,
    launcher: Rc<dyn BrowserLauncher>,
}

trait BrowserLauncher: std::fmt::Debug {
    fn open(&self, url: &str) -> Result<(), String>;
}

#[derive(Debug)]
struct RecordingBrowserLauncher;

impl BrowserLauncher for RecordingBrowserLauncher {
    fn open(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug)]
struct SystemBrowserLauncher;

impl BrowserLauncher for SystemBrowserLauncher {
    fn open(&self, url: &str) -> Result<(), String> {
        system_browser_command(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("system browser launch failed: {error}"))
    }
}

#[cfg(target_os = "macos")]
fn system_browser_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_browser_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(windows)]
fn system_browser_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(not(any(unix, windows)))]
fn system_browser_command(_url: &str) -> Command {
    Command::new("unsupported-system-browser")
}

impl BrowserAdapterFactory {
    pub fn new() -> Self {
        Self {
            observer: Rc::new(BrowserObserver::default()),
            launcher: Rc::new(RecordingBrowserLauncher),
        }
    }

    pub fn with_system_browser() -> Self {
        Self {
            observer: Rc::new(BrowserObserver::default()),
            launcher: Rc::new(SystemBrowserLauncher),
        }
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.observer.local_addr.get()
    }

    pub fn launched_url(&self) -> Option<String> {
        self.observer.launched_url.borrow().clone()
    }

    pub async fn wait_until_launched(&self) -> Result<(), RuntimeFailure> {
        for _ in 0..64 {
            if self.launched_url().is_some() {
                return Ok(());
            }
            if let Some(detail) = self.observer.launch_error.borrow().clone() {
                return Err(RuntimeFailure::ModuleFailure { detail });
            }
            tokio::task::yield_now().await;
        }
        Err(RuntimeFailure::ModuleFailure {
            detail: "Browser Adapter did not publish its ready URL".to_owned(),
        })
    }
}

impl Default for BrowserAdapterFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeModuleFactory for BrowserAdapterFactory {
    fn package_id(&self) -> &'static str {
        BROWSER_PACKAGE_ID
    }

    fn package_version(&self) -> &'static str {
        "0.1.0"
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::with_lifecycle(
            Vec::new(),
            BrowserLifecycle {
                observer: self.observer.clone(),
                launcher: self.launcher.clone(),
                generation: Rc::new(BrowserGeneration::default()),
            },
        ))
    }
}

#[derive(Debug)]
struct BrowserLifecycle {
    observer: Rc<BrowserObserver>,
    launcher: Rc<dyn BrowserLauncher>,
    generation: Rc<BrowserGeneration>,
}

impl ModuleLifecycle for BrowserLifecycle {
    fn prepare(&self, _context: PrepareContext) -> lenso_kernel::ModuleFuture {
        let observer = self.observer.clone();
        let generation = self.generation.clone();
        Box::pin(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|error| {
                RuntimeFailure::ModuleFailure {
                    detail: format!("Browser Adapter bind failed: {error}"),
                }
            })?;
            let address = listener
                .local_addr()
                .map_err(|error| RuntimeFailure::ModuleFailure {
                    detail: format!("Browser Adapter address failed: {error}"),
                })?;
            observer.local_addr.set(Some(address));
            generation.listener.borrow_mut().replace(listener);
            Ok(())
        })
    }

    fn activate(&self, context: ActivateContext) -> lenso_kernel::ModuleFuture {
        let shell = match ShellClient::from_dependencies(context.dependencies()) {
            Ok(client) => Rc::new(client),
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let auth = match AuthClient::from_dependencies(context.dependencies()) {
            Ok(client) => Rc::new(client),
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let greeting = match SecureGreetingClient::from_dependencies(context.dependencies()) {
            Ok(client) => Rc::new(client),
            Err(error) => return Box::pin(futures::future::ready(Err(error))),
        };
        let Some(listener) = self.generation.listener.borrow_mut().take() else {
            return Box::pin(futures::future::ready(Err(RuntimeFailure::Internal {
                detail: "Browser Adapter listener was not prepared".to_owned(),
            })));
        };
        let observer = self.observer.clone();
        let launcher = self.launcher.clone();
        let readiness = context.readiness();
        let cancellation = context.cancellation();
        let runtime = BrowserRuntime {
            shell,
            auth,
            greeting,
            dependencies: context.dependencies().clone(),
        };
        let spawn = context.tasks().spawn_local(Box::pin(async move {
            readiness.wait().await;
            let Some(address) = observer.local_addr.get() else {
                return;
            };
            let url = format!("http://{address}/orders");
            match launcher.open(&url) {
                Ok(()) => observer.launched_url.replace(Some(url)),
                Err(error) => observer.launch_error.replace(Some(error)),
            };
            accept(listener, runtime, cancellation).await;
        }));
        match spawn {
            Ok(_) => Box::pin(futures::future::ready(Ok(()))),
            Err(error) => Box::pin(futures::future::ready(Err(RuntimeFailure::Internal {
                detail: format!("Browser Adapter task could not start: {error:?}"),
            }))),
        }
    }
}

#[derive(Clone, Debug)]
struct BrowserRuntime {
    shell: Rc<ShellClient>,
    auth: Rc<AuthClient>,
    greeting: Rc<SecureGreetingClient>,
    dependencies: ModuleDependencies,
}

async fn accept(listener: TcpListener, runtime: BrowserRuntime, cancellation: CancellationToken) {
    loop {
        let accepted = tokio::select! {
            () = cancellation.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        let Ok((stream, _)) = accepted else {
            return;
        };
        serve(stream, &runtime).await;
    }
}

async fn serve(mut stream: TcpStream, runtime: &BrowserRuntime) {
    let response = match read_request(&mut stream).await {
        Ok(request) => dispatch(request, runtime).await,
        Err(response) => response,
    };
    let _ = write_response(&mut stream, response).await;
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    token: Option<String>,
    body: String,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: String,
    body: String,
}

impl HttpResponse {
    fn text(status: u16, content_type: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
        }
    }

    fn json(status: u16, body: impl Into<String>) -> Self {
        Self::text(status, "application/json; charset=utf-8", body)
    }

    fn capability(status: u16, result: &serde_json::Value) -> Self {
        Self::json(
            status,
            serde_json::to_string(result).expect("Capability result serializes"),
        )
    }
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, HttpResponse> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2_048];
    let header_end;
    loop {
        let read = stream.read(&mut buffer).await.map_err(|_| bad_request())?;
        if read == 0 || bytes.len() + read > MAX_REQUEST_BYTES {
            return Err(bad_request());
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = find_header_end(&bytes) {
            header_end = end;
            break;
        }
    }
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| bad_request())?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().ok_or_else(bad_request)?.split_whitespace();
    let method = request_line.next().ok_or_else(bad_request)?.to_owned();
    let path = request_line.next().ok_or_else(bad_request)?.to_owned();
    if request_line.next() != Some("HTTP/1.1") || request_line.next().is_some() {
        return Err(bad_request());
    }
    let mut content_length = None;
    let mut token = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(bad_request());
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(bad_request());
            }
            content_length = Some(value.parse().map_err(|_| bad_request())?);
        } else if name.eq_ignore_ascii_case("authorization") {
            if token.is_some() {
                return Err(bad_request());
            }
            token = Some(
                value
                    .strip_prefix("Bearer ")
                    .filter(|token| !token.is_empty())
                    .ok_or_else(bad_request)?
                    .to_owned(),
            );
        }
    }
    let content_length = content_length.unwrap_or(0);
    if content_length > MAX_REQUEST_BYTES.saturating_sub(header_end + 4) {
        return Err(bad_request());
    }
    while bytes.len() < header_end + 4 + content_length {
        let read = stream.read(&mut buffer).await.map_err(|_| bad_request())?;
        if read == 0 || bytes.len() + read > MAX_REQUEST_BYTES {
            return Err(bad_request());
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    let body = std::str::from_utf8(&bytes[header_end + 4..header_end + 4 + content_length])
        .map_err(|_| bad_request())?
        .to_owned();
    Ok(HttpRequest {
        method,
        path,
        token,
        body,
    })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn bad_request() -> HttpResponse {
    HttpResponse::json(400, r#"{"error":"bad_request"}"#)
}

async fn dispatch(request: HttpRequest, runtime: &BrowserRuntime) -> HttpResponse {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", path) if path.starts_with("/assets/") => read_asset(path, runtime).await,
        ("GET", path) => render_route(path, runtime).await,
        ("POST", GREETING_ROUTE) => greet(request, runtime).await,
        _ => HttpResponse::json(404, r#"{"error":"not_found"}"#),
    }
}

async fn render_route(path: &str, runtime: &BrowserRuntime) -> HttpResponse {
    match runtime
        .shell
        .render_route(RenderRouteRequest {
            path: path.to_owned(),
        })
        .await
    {
        Ok(route) => HttpResponse::text(200, "text/html; charset=utf-8", route.body),
        Err(ShellRenderRouteInvocationError::Domain(_)) => {
            HttpResponse::json(404, r#"{"error":"route_not_found"}"#)
        }
        Err(ShellRenderRouteInvocationError::Runtime(_)) => {
            HttpResponse::json(503, r#"{"error":"unavailable"}"#)
        }
    }
}

async fn read_asset(path: &str, runtime: &BrowserRuntime) -> HttpResponse {
    match runtime
        .shell
        .read_asset(ReadAssetRequest {
            path: path.to_owned(),
        })
        .await
    {
        Ok(asset) => HttpResponse::text(200, asset.content_type, asset.content),
        Err(ShellReadAssetInvocationError::Domain(_)) => {
            HttpResponse::json(404, r#"{"error":"asset_not_found"}"#)
        }
        Err(ShellReadAssetInvocationError::Runtime(_)) => {
            HttpResponse::json(503, r#"{"error":"unavailable"}"#)
        }
    }
}

async fn greet(request: HttpRequest, runtime: &BrowserRuntime) -> HttpResponse {
    let Ok(route) = runtime
        .shell
        .render_route(RenderRouteRequest {
            path: "/orders".to_owned(),
        })
        .await
    else {
        return capability_runtime(404, "unavailable", "route_not_found");
    };
    if !allows_greeting(&route) {
        return capability_runtime(403, "unavailable", "client_not_projected");
    }
    let Some(token) = request.token else {
        return capability_runtime(401, "module_failure", "credential_required");
    };
    let auth = match runtime
        .auth
        .authenticate(authenticate_request(Some(CredentialEvidence::new(
            "bearer", token,
        ))))
        .await
    {
        Ok(response) => response,
        Err(AuthInvocationError::Domain(error)) => return auth_rejection(&error),
        Err(AuthInvocationError::Runtime(_)) => {
            return capability_runtime(503, "unavailable", "auth_unavailable");
        }
    };
    let assertion = match decode_auth_response(auth) {
        Ok(AuthOutcome::Authenticated(assertion)) => assertion,
        Ok(AuthOutcome::Absent) => {
            return capability_runtime(401, "module_failure", "credential_required");
        }
        Err(_) => return capability_runtime(502, "protocol_violation", "invalid_auth_response"),
    };
    let greeting: GreetRequest = match serde_json::from_str(&request.body) {
        Ok(request) => request,
        Err(_) => return capability_runtime(400, "protocol_violation", "invalid_request"),
    };
    let Ok(context) = runtime
        .dependencies
        .invocation_context_after(Duration::from_secs(2), CancellationToken::new())
        .and_then(|context| {
            assertion
                .attach(context)
                .map_err(|error| RuntimeFailure::ModuleFailure {
                    detail: format!("Browser assertion could not attach: {error}"),
                })
        })
    else {
        return capability_runtime(503, "unavailable", "context_unavailable");
    };
    match runtime.greeting.greet_with_context(context, greeting).await {
        Ok(response) => {
            HttpResponse::capability(200, &serde_json::json!({ "ok": true, "value": response }))
        }
        Err(SecureGreetingInvocationError::Domain(GreetError::NotAllowed)) => {
            capability_domain(403, "not_allowed")
        }
        Err(SecureGreetingInvocationError::Domain(GreetError::ActorRequired)) => {
            capability_domain(401, "actor_required")
        }
        Err(SecureGreetingInvocationError::Domain(GreetError::EmptyName)) => {
            capability_domain(400, "empty_name")
        }
        Err(SecureGreetingInvocationError::Domain(error @ GreetError::Unknown(_))) => {
            capability_domain_value(
                422,
                &serde_json::to_value(error).expect("generated Domain Error serializes"),
            )
        }
        Err(SecureGreetingInvocationError::Runtime(_)) => {
            capability_runtime(503, "unavailable", "target_unavailable")
        }
    }
}

fn capability_domain(status: u16, error: &str) -> HttpResponse {
    capability_domain_value(status, &serde_json::Value::String(error.to_owned()))
}

fn capability_domain_value(status: u16, error: &serde_json::Value) -> HttpResponse {
    HttpResponse::capability(
        status,
        &serde_json::json!({
            "ok": false,
            "error": { "kind": "domain", "error": error },
        }),
    )
}

fn capability_runtime(status: u16, kind: &str, detail: &str) -> HttpResponse {
    HttpResponse::capability(
        status,
        &serde_json::json!({
            "ok": false,
            "error": { "kind": "runtime", "error": { "kind": kind, "detail": detail } },
        }),
    )
}

fn allows_greeting(route: &RenderRouteResponse) -> bool {
    route.requirements.iter().any(|requirement| {
        requirement.capability_id == GREETING_CAPABILITY_ID
            && requirement.descriptor_version == GREETING_DESCRIPTOR_VERSION
            && requirement
                .operations
                .iter()
                .any(|operation| operation == GREET_OPERATION)
    })
}

fn auth_rejection(error: &AuthenticateError) -> HttpResponse {
    let code = match error {
        AuthenticateError::Invalid => "invalid",
        AuthenticateError::Expired => "expired",
        AuthenticateError::Revoked => "revoked",
        AuthenticateError::Unsupported => "unsupported",
        AuthenticateError::Unknown(_) => "unknown_auth_error",
    };
    capability_runtime(401, "module_failure", code)
}

async fn write_response(stream: &mut TcpStream, response: HttpResponse) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Content",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await
}
