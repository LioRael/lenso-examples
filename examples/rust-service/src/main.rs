use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use lenso::{
    ModuleHttpMethod, ModuleHttpRoute, ModuleManifest, ModuleManifestLintSeverity, ModuleSource,
    RuntimeFunctionDeclaration, RuntimeSurface, ServiceOperationMetadata,
    ServiceOperationSafeProbe, lint_module_manifest,
};
use serde_json::{Value, json};

const DEFAULT_PORT: u16 = 4130;
const SERVICE_NAME: &str = "rust-audit-service";
const MODULE_NAME: &str = "rust-audit-log";
const READ_CAPABILITY: &str = "rust_audit_log.events.read";
const EVENTS_OPERATION_ID: &str = "rust-audit-log/http/GET:/events";
const SUMMARIZE_FUNCTION_NAME: &str = "rust-audit-log.summarize-events.v1";
const SUMMARIZE_OPERATION_ID: &str = "rust-audit-log/runtime/rust-audit-log.summarize-events.v1";

#[derive(Clone)]
struct AppState {
    module_manifest: Value,
    service_manifest: Value,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = service_port();
    let module = audit_log_module();
    ensure_manifest_has_no_errors(&module)?;
    let state = AppState {
        module_manifest: serde_json::to_value(module)?,
        service_manifest: service_manifest(port)?,
    };

    if std::env::args().any(|arg| arg == "--check") {
        println!("{}", serde_json::to_string_pretty(&state.service_manifest)?);
        return Ok(());
    }

    let app = Router::new()
        .route("/lenso/service/v1/manifest", get(manifest))
        .route("/lenso/service/v1/ready", get(ready))
        .route("/lenso/service/v1/status", get(status))
        .route(
            "/lenso/service/v1/modules/rust-audit-log/manifest",
            get(module_manifest_handler),
        )
        .route(
            "/lenso/service/v1/modules/rust-audit-log/events",
            get(audit_events),
        )
        .route(
            "/lenso/service/v1/modules/rust-audit-log/runtime/functions/rust-audit-log.summarize-events.v1/invoke",
            post(summarize_events),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;

    println!("Rust Audit service manifest: http://127.0.0.1:{port}/lenso/service/v1/manifest");
    println!("Rust Audit service status: http://127.0.0.1:{port}/lenso/service/v1/status");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn manifest(State(state): State<AppState>) -> Json<Value> {
    Json(state.service_manifest)
}

async fn module_manifest_handler(State(state): State<AppState>) -> Json<Value> {
    Json(state.module_manifest)
}

async fn ready() -> Json<Value> {
    Json(json!({ "ready": true }))
}

async fn status() -> Json<Value> {
    Json(json!({
        "checks": [
            { "name": MODULE_NAME, "status": "ok" },
        ],
        "manifestUrl": format!("http://127.0.0.1:{}/lenso/service/v1/manifest", service_port()),
        "modules": [
            { "name": MODULE_NAME, "version": "0.1.0" },
        ],
        "protocolVersion": "1",
        "serviceName": SERVICE_NAME,
        "state": "ready",
        "transports": ["http"],
        "version": "0.1.0",
    }))
}

async fn audit_events() -> Json<Value> {
    Json(json!({
        "next_cursor": null,
        "records": [
            {
                "actor": "system",
                "id": "evt_1001",
                "occurred_at": "2026-06-27T00:00:00Z",
                "summary": "Rust service provider started",
                "target": SERVICE_NAME,
            },
            {
                "actor": "support-lead",
                "id": "evt_1002",
                "occurred_at": "2026-06-27T00:05:00Z",
                "summary": "Support ticket priority reviewed",
                "target": "ticket_1",
            },
        ],
    }))
}

async fn summarize_events() -> Json<Value> {
    Json(json!({
        "output": {
            "count": 2,
            "summary": "2 audit events available",
        },
    }))
}

fn audit_log_module() -> ModuleManifest {
    ModuleManifest::builder(MODULE_NAME)
        .capabilities(vec![READ_CAPABILITY.to_owned()])
        .http_routes(vec![ModuleHttpRoute {
            method: ModuleHttpMethod::Get,
            path: "/events".to_owned(),
            capability: Some(READ_CAPABILITY.to_owned()),
            display_name: Some("List audit events".to_owned()),
            operation: Some(ServiceOperationMetadata {
                operation_id: Some(EVENTS_OPERATION_ID.to_owned()),
                safe_probe: Some(ServiceOperationSafeProbe {
                    method: Some("GET".to_owned()),
                    path: Some("/events".to_owned()),
                    input: None,
                    expect_status: Some(200),
                }),
                summary: Some("List audit events".to_owned()),
                ..ServiceOperationMetadata::default()
            }),
            story_title: Some("Audit events listed".to_owned()),
        }])
        .runtime(RuntimeSurface {
            functions: vec![RuntimeFunctionDeclaration {
                name: SUMMARIZE_FUNCTION_NAME.to_owned(),
                version: 1,
                queue: MODULE_NAME.to_owned(),
                input_schema: None,
                retry_policy: None,
                operation: Some(ServiceOperationMetadata {
                    operation_id: Some(SUMMARIZE_OPERATION_ID.to_owned()),
                    summary: Some("Summarize audit events".to_owned()),
                    ..ServiceOperationMetadata::default()
                }),
            }],
            schedules: vec![],
        })
        .build()
}

fn service_manifest(port: u16) -> Result<Value, serde_json::Error> {
    let module_manifest = serde_json::to_value(audit_log_module())?;
    Ok(json!({
        "compatibility": {
            "remoteProtocolVersion": "1",
            "requiredHostFeatures": ["service.status"],
            "sdkLanguage": "rust",
            "sdkVersion": env!("CARGO_PKG_VERSION"),
        },
        "deployment": {
            "commands": ["cargo run --manifest-path examples/rust-service/Cargo.toml"],
            "target": "cargo-binary",
        },
        "health": {
            "manifestUrl": format!("http://127.0.0.1:{port}/lenso/service/v1/manifest"),
            "readyUrl": format!("http://127.0.0.1:{port}/lenso/service/v1/status"),
            "statusUrl": format!("http://127.0.0.1:{port}/lenso/service/v1/status"),
        },
        "install": {
            "services": [
                {
                    "autoStart": true,
                    "command": "cargo run --manifest-path examples/rust-service/Cargo.toml",
                    "name": SERVICE_NAME,
                    "readyTimeoutMs": 30000,
                    "readyUrl": format!("http://127.0.0.1:{port}/lenso/service/v1/status"),
                },
            ],
        },
        "modules": [module_manifest],
        "name": SERVICE_NAME,
        "protocol": "lenso.service.v1",
        "provider": {
            "name": SERVICE_NAME,
            "summary": "Rust audit log service provider",
            "vendor": "Lenso",
        },
        "requiredEnv": [],
        "statusPath": "/lenso/service/v1/status",
        "transports": ["http"],
        "version": "0.1.0",
    }))
}

fn ensure_manifest_has_no_errors(
    manifest: &ModuleManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let lints = lint_module_manifest(ModuleSource::Remote, manifest);
    let errors = lints
        .iter()
        .filter(|lint| matches!(lint.severity, ModuleManifestLintSeverity::Error))
        .collect::<Vec<_>>();
    if errors.is_empty() {
        return Ok(());
    }
    for lint in errors {
        eprintln!(
            "{:?}: {} - {} ({})",
            lint.severity, lint.subject, lint.message, lint.suggestion
        );
    }
    Err("rust service module manifest failed lint".into())
}

fn service_port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_manifest_declares_rust_provider_and_probeable_route() {
        let manifest = service_manifest(4130).unwrap();

        assert_eq!(manifest["name"], SERVICE_NAME);
        assert_eq!(manifest["compatibility"]["sdkLanguage"], "rust");
        assert_eq!(
            manifest["health"]["readyUrl"],
            "http://127.0.0.1:4130/lenso/service/v1/status"
        );
        assert_eq!(manifest["modules"][0]["name"], MODULE_NAME);
        assert_eq!(manifest["modules"][0]["http_routes"][0]["method"], "GET");
        assert_eq!(manifest["modules"][0]["http_routes"][0]["path"], "/events");
        assert_eq!(
            manifest["modules"][0]["http_routes"][0]["operation"]["operationId"],
            EVENTS_OPERATION_ID
        );
        assert_eq!(
            manifest["modules"][0]["http_routes"][0]["operation"]["safeProbe"]["path"],
            "/events"
        );
        assert_eq!(
            manifest["modules"][0]["runtime"]["functions"][0]["name"],
            SUMMARIZE_FUNCTION_NAME
        );
        assert_eq!(
            manifest["modules"][0]["runtime"]["functions"][0]["operation"]["operationId"],
            SUMMARIZE_OPERATION_ID
        );
    }

    #[test]
    fn module_manifest_lints_without_errors() {
        ensure_manifest_has_no_errors(&audit_log_module()).unwrap();
    }
}
