use std::{net::SocketAddr, time::Duration};

use lenso_authoring::{
    CapabilityEndpoint, Module, ProjectAuthoring, ResolutionOptions, WebProfile,
};
use lenso_capability_secure_greeting::{CAPABILITY_ID, DESCRIPTOR_VERSION, GREET_OPERATION};
use lenso_vnext_web_ui::{MetadataScenario, ORDERS_PACKAGE_ID, WebUiFixture};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    task::LocalSet,
};

#[test]
fn web_profile_materializes_explicit_modules_entrypoints_and_bindings_before_boot() {
    let fixture = WebUiFixture::orders();
    let project = fixture.project_file();
    let project_without_web = fixture.clone().without_web().project_file();
    assert_ne!(
        serde_json::to_vec_pretty(&project).unwrap(),
        serde_json::to_vec_pretty(&project_without_web).unwrap()
    );
    let plan = fixture
        .resolved_plan()
        .expect("the target-owned Web profile should resolve");

    let instances = plan.module_instances();
    let orders_backend = instances
        .iter()
        .find(|instance| instance.instance_key() == "orders")
        .expect("business Module should be selected");
    let orders_ui = instances
        .iter()
        .find(|instance| instance.instance_key() == "orders-ui")
        .expect("UI Contribution Module should be selected");

    assert_eq!(orders_backend.package_id(), ORDERS_PACKAGE_ID);
    assert_eq!(orders_backend.entrypoint(), "backend");
    assert_eq!(orders_ui.package_id(), ORDERS_PACKAGE_ID);
    assert_eq!(orders_ui.entrypoint(), "ui");
    assert!(
        instances
            .iter()
            .any(|instance| instance.instance_key() == "web-shell")
    );
    assert!(
        instances
            .iter()
            .any(|instance| instance.instance_key() == "browser-adapter")
    );
    assert!(plan.capability_bindings().iter().any(|binding| {
        binding.consumer_instance() == "web-shell"
            && binding.provider_instance() == "orders-ui"
            && binding.capability_id() == "lenso.ui.contribution@1"
    }));
}

#[test]
fn web_profile_rejects_a_browser_projection_bound_to_another_provider() {
    let mut project = WebUiFixture::orders().project_file();
    project.composition_mut().add_module(
        Module::new("orders-alternate", ORDERS_PACKAGE_ID)
            .with_entrypoint("backend")
            .with_capability(CapabilityEndpoint::request(
                CAPABILITY_ID,
                DESCRIPTOR_VERSION,
                [GREET_OPERATION],
            )),
    );
    let binding = project
        .composition_mut()
        .bindings_mut()
        .iter_mut()
        .find(|binding| {
            binding.consumer() == "browser-adapter" && binding.capability_id() == CAPABILITY_ID
        })
        .unwrap();
    *binding = lenso_authoring::Binding::new(
        "browser-adapter",
        CAPABILITY_ID,
        DESCRIPTOR_VERSION,
        "orders-alternate",
    );
    project.profiles_mut().insert(
        "web".to_owned(),
        WebProfile::new("web-shell", "browser-adapter")
            .with_ui_contribution("orders-ui")
            .with_module("orders")
            .with_module("orders-alternate")
            .with_module("auth")
            .with_module("worker"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let error = project
        .resolve(&root, &ResolutionOptions::default().with_profile("web"))
        .expect_err("a browser projection cannot bypass the contribution binding");
    assert!(
        error
            .to_string()
            .contains("same cardinality and resolved provider")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn route_assets_and_allowlisted_generated_client_run_after_app_readiness() {
    LocalSet::new()
        .run_until(async {
            let running = WebUiFixture::orders()
                .start()
                .await
                .expect("target-owned Web App should start");
            assert!(running.app().is_ready());
            let expected_url = format!("http://{}/orders", running.local_addr().unwrap());
            assert_eq!(
                running.launched_url().as_deref(),
                Some(expected_url.as_str())
            );

            let page = request(running.local_addr().unwrap(), "GET", "/orders", None, None).await;
            assert_eq!(page.status, 200);
            assert!(page.body.contains("<h1>Orders</h1>"));
            assert!(page.body.contains("/assets/orders.js"));

            let asset = request(
                running.local_addr().unwrap(),
                "GET",
                "/assets/orders.js",
                None,
                None,
            )
            .await;
            assert_eq!(asset.status, 200);
            assert!(asset.body.contains("addEventListener"));

            let client = request(
                running.local_addr().unwrap(),
                "GET",
                "/assets/generated/secure-greeting.js",
                None,
                None,
            )
            .await;
            assert_eq!(client.status, 200);
            assert!(client.body.contains("@generated by lenso-contract-codegen"));
            assert!(client.body.contains("createSecureGreetingClient"));

            let allowed = request(
                running.local_addr().unwrap(),
                "POST",
                "/api/capabilities/example.secure-greeting@1/greet",
                Some("good-token"),
                Some(r#"{"name":"Ada"}"#),
            )
            .await;
            assert_eq!(allowed.status, 200);
            assert_eq!(
                allowed.body,
                r#"{"ok":true,"value":{"message":"Hello, Ada (user-123)!"}}"#
            );

            let denied = request(
                running.local_addr().unwrap(),
                "POST",
                "/api/capabilities/example.secure-greeting@1/greet",
                Some("forbidden-token"),
                Some(r#"{"name":"Ada"}"#),
            )
            .await;
            assert_eq!(denied.status, 403);
            assert_eq!(
                denied.body,
                r#"{"error":{"error":"not_allowed","kind":"domain"},"ok":false}"#
            );

            let ambient = request(
                running.local_addr().unwrap(),
                "POST",
                "/api/invoke",
                Some("good-token"),
                Some(r#"{"capability":"example.secure-greeting@1"}"#),
            )
            .await;
            assert_eq!(ambient.status, 404);

            running.shutdown(Duration::from_secs(1)).await;
        })
        .await;
}

#[ignore = "requires Bun; CI runs this exact generated-client test after installing Bun"]
#[tokio::test(flavor = "current_thread")]
async fn generated_browser_client_invokes_the_allowlisted_app_capability() {
    LocalSet::new()
        .run_until(async {
            let running = WebUiFixture::orders()
                .start()
                .await
                .expect("target-owned Web App should start");
            let browser_result = run_generated_client(running.local_addr().unwrap()).await;
            assert_eq!(
                browser_result,
                r#"{"ok":true,"value":{"message":"Hello, Ada (user-123)!"}}"#
            );
            running.shutdown(Duration::from_secs(1)).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn generated_client_transport_preserves_unknown_and_protocol_errors() {
    LocalSet::new()
        .run_until(async {
            let running = WebUiFixture::orders().start().await.unwrap();
            let address = running.local_addr().unwrap();
            let unknown = request(
                address,
                "POST",
                "/api/capabilities/example.secure-greeting@1/greet",
                Some("good-token"),
                Some(r#"{"name":"Future"}"#),
            )
            .await;
            assert_eq!(unknown.status, 422);
            assert_eq!(
                unknown.body,
                r#"{"error":{"error":{"code":"future_rule","payload":{"retry":false},"source":"orders"},"kind":"domain"},"ok":false}"#
            );

            let malformed = request(
                address,
                "POST",
                "/api/capabilities/example.secure-greeting@1/greet",
                Some("good-token"),
                Some("{"),
            )
            .await;
            assert_eq!(malformed.status, 400);
            assert!(malformed.body.contains(r#""kind":"runtime""#));
            assert!(malformed.body.contains(r#""kind":"protocol_violation""#));
            running.shutdown(Duration::from_secs(1)).await;
        })
        .await;
}

async fn run_generated_client(address: SocketAddr) -> String {
    let origin = format!("http://{address}");
    let script = r#"
const origin = process.argv[1];
const source = await fetch(`${origin}/assets/generated/secure-greeting.js`).then((response) => response.text());
const { unlink } = await import("node:fs/promises");
const { tmpdir } = await import("node:os");
const { join } = await import("node:path");
const { pathToFileURL } = await import("node:url");
const modulePath = join(tmpdir(), `lenso-generated-client-${process.pid}.mjs`);
await Bun.write(modulePath, source);
try {
  const { createSecureGreetingClient } = await import(pathToFileURL(modulePath).href);
  let rejectedMalformedEnvelope = false;
  try {
    const malformedTransport = async () => ({ json: async () => ({ ok: true }) });
    await createSecureGreetingClient(malformedTransport).greet({ name: "Ada" }, "good-token");
  } catch {
    rejectedMalformedEnvelope = true;
  }
  if (!rejectedMalformedEnvelope) throw new Error("generated client accepted a malformed result envelope");
  const transport = (path, init) => fetch(`${origin}${path}`, init);
  const result = await createSecureGreetingClient(transport).greet({ name: "Ada" }, "good-token");
  process.stdout.write(JSON.stringify(result));
} finally {
  await unlink(modulePath).catch(() => {});
}
"#;
    let output = Command::new("bun")
        .args(["--eval", script, &origin])
        .output()
        .await
        .expect("Bun should execute the generated browser client");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Bun output should be UTF-8")
}

#[tokio::test(flavor = "current_thread")]
async fn missing_or_colliding_contribution_metadata_fails_before_readiness() {
    LocalSet::new()
        .run_until(async {
            for (scenario, expected) in [
                (MetadataScenario::MissingRoute, "route must start with `/`"),
                (MetadataScenario::CollidingRoute, "route collision"),
            ] {
                let error = WebUiFixture::orders()
                    .with_metadata_scenario(scenario)
                    .start()
                    .await
                    .expect_err("invalid contribution metadata must fail startup");
                let error = format!("{error:?}");
                assert!(error.contains(expected), "{error}");
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn removing_web_modules_leaves_non_ui_business_invocation_unchanged() {
    LocalSet::new()
        .run_until(async {
            let running = WebUiFixture::orders()
                .without_web()
                .start()
                .await
                .expect("the non-Web App should still start");
            assert!(running.app().is_ready());
            assert!(running.local_addr().is_none());
            assert_eq!(
                running
                    .invoke_worker("good-token", "Ada")
                    .await
                    .expect("non-UI worker invocation should succeed"),
                "Hello, Ada (user-123)!"
            );
            running.shutdown(Duration::from_secs(1)).await;
        })
        .await;
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: String,
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Response {
    let mut stream = TcpStream::connect(address)
        .await
        .expect("connect to Browser Adapter");
    let body = body.unwrap_or_default();
    let authorization = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{authorization}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(wire.as_bytes())
        .await
        .expect("write HTTP request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read HTTP response");
    let response = String::from_utf8(response).expect("response should be UTF-8");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response framing");
    let status = head
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric HTTP status");
    Response {
        status,
        body: body.to_owned(),
    }
}
