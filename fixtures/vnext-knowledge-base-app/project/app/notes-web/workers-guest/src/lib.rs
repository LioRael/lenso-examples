use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;

#[path = "../../src/settings_core.rs"]
mod settings_core;

use settings_core::{
    BusinessSettings, SettingsCommandError, SettingsFailure, UpdateBusinessSettings,
    prepare_settings_command, problem_spec,
};

wit_bindgen::generate!({
    path: "wit",
    world: "plugin",
});

const DESCRIPTOR: &str = r#"{"abi":"lenso.json-request@1","capabilities":[{"capability_id":"lenso.http.endpoint@1","descriptor_digest":"sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe","descriptor_version":"1.1.0","request_operations":["describe","handle"]}]}"#;

#[cfg(target_arch = "wasm32")]
#[used]
#[unsafe(link_section = "lenso.plugin-descriptor.v1")]
static DESCRIPTOR_SECTION: [u8; DESCRIPTOR.len()] = descriptor_bytes(DESCRIPTOR);

#[cfg(target_arch = "wasm32")]
const fn descriptor_bytes<const N: usize>(value: &str) -> [u8; N] {
    let source = value.as_bytes();
    let mut bytes = [0; N];
    let mut index = 0;
    while index < N {
        bytes[index] = source[index];
        index += 1;
    }
    bytes
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareRequest {
    schema: String,
    route_id: String,
    body: Option<UpdateBusinessSettings>,
    idempotency_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteRequest {
    schema: String,
    route_id: String,
    result: serde_json::Value,
}

enum StoreOutcome {
    Ok(BusinessSettings),
    StaleRevision,
    IdempotencyConflict,
    Unauthorized,
    StorageUnavailable,
}

struct GuestComponent;

impl Guest for GuestComponent {
    fn describe() -> String {
        DESCRIPTOR.to_owned()
    }

    fn invoke(
        capability: String,
        operation: String,
        _request_json: String,
    ) -> Result<String, String> {
        if capability != "lenso.http.endpoint@1" {
            return Err("unknown_capability".to_owned());
        }
        match operation.as_str() {
            "describe" => Ok(json!({"routes": [
                {"route_id":"knowledge-base.settings.read","method":"GET","path":"/settings"},
                {"route_id":"knowledge-base.settings.update","method":"PUT","path":"/settings"}
            ]})
            .to_string()),
            "handle" => Err("settings_host_binding_required".to_owned()),
            _ => Err("unknown_operation".to_owned()),
        }
    }

    fn prepare_settings(request_json: String) -> Result<String, String> {
        let request: PrepareRequest = serde_json::from_str(&request_json)
            .map_err(|_| problem_response(SettingsFailure::InvalidLimit))?;
        if request.schema != "lenso.knowledge-settings-prepare.v1" {
            return Err(problem_response(SettingsFailure::InvalidLimit));
        }
        match request.route_id.as_str() {
            "knowledge-base.settings.read"
                if request.body.is_none() && request.idempotency_key.is_none() =>
            {
                Ok(
                    json!({"schema":"lenso.knowledge-settings-command.v1","kind":"read"})
                        .to_string(),
                )
            }
            "knowledge-base.settings.update" => {
                let body = request
                    .body
                    .ok_or_else(|| problem_response(SettingsFailure::InvalidLimit))?;
                let command = prepare_settings_command(body, request.idempotency_key.as_deref())
                    .map_err(command_error_response)?;
                let mut response = json!({
                    "schema":"lenso.knowledge-settings-command.v1",
                    "kind":"cas",
                    "excerpt_limit":command.excerpt_limit,
                    "predecessor_revision":command.predecessor_revision,
                    "payload_sha256":command.payload_sha256,
                });
                if let Some(key) = command.idempotency_key {
                    response["idempotency_key"] = json!(key);
                }
                Ok(response.to_string())
            }
            _ => Err(problem_response(SettingsFailure::InvalidLimit)),
        }
    }

    fn complete_settings(result_json: String) -> Result<String, String> {
        let request: CompleteRequest =
            serde_json::from_str(&result_json).map_err(|_| "invalid_host_result".to_owned())?;
        if request.schema != "lenso.knowledge-settings-complete.v1"
            || !matches!(
                request.route_id.as_str(),
                "knowledge-base.settings.read" | "knowledge-base.settings.update"
            )
        {
            return Err("invalid_host_result".to_owned());
        }
        let outcome =
            parse_store_result(&request.result).ok_or_else(|| "invalid_host_result".to_owned())?;
        if request.route_id == "knowledge-base.settings.read"
            && matches!(
                outcome,
                StoreOutcome::StaleRevision | StoreOutcome::IdempotencyConflict
            )
        {
            return Err("invalid_host_result".to_owned());
        }
        match outcome {
            StoreOutcome::Ok(settings)
                if (16..=512).contains(&settings.excerpt_limit) && settings.revision > 0 =>
            {
                Ok(endpoint_response(
                    200,
                    "application/json; charset=utf-8",
                    serde_json::to_string(&settings).expect("settings serialize"),
                ))
            }
            StoreOutcome::Ok(..) => Err("invalid_host_result".to_owned()),
            StoreOutcome::StaleRevision => Ok(problem_response(SettingsFailure::StaleRevision)),
            StoreOutcome::IdempotencyConflict => {
                Ok(problem_response(SettingsFailure::IdempotencyConflict))
            }
            StoreOutcome::Unauthorized => Ok(problem_response(SettingsFailure::Unauthorized)),
            StoreOutcome::StorageUnavailable => {
                Ok(problem_response(SettingsFailure::StorageUnavailable))
            }
        }
    }
}

fn parse_store_result(value: &serde_json::Value) -> Option<StoreOutcome> {
    let fields = value.as_object()?;
    if fields.get("schema")?.as_str()? != "lenso.knowledge-settings-result.v1" {
        return None;
    }
    match fields.get("kind")?.as_str()? {
        "ok" if fields.len() == 3 => Some(StoreOutcome::Ok(
            serde_json::from_value(fields.get("settings")?.clone()).ok()?,
        )),
        "stale_revision" if fields.len() == 2 => Some(StoreOutcome::StaleRevision),
        "idempotency_conflict" if fields.len() == 2 => Some(StoreOutcome::IdempotencyConflict),
        "unauthorized" if fields.len() == 2 => Some(StoreOutcome::Unauthorized),
        "storage_unavailable" if fields.len() == 2 => Some(StoreOutcome::StorageUnavailable),
        _ => None,
    }
}

fn command_error_response(error: SettingsCommandError) -> String {
    problem_response(error.into())
}

fn problem_response(failure: SettingsFailure) -> String {
    let spec = problem_spec(failure);
    endpoint_response(
        spec.status,
        "application/problem+json; charset=utf-8",
        json!({
            "type":"about:blank",
            "title":spec.title,
            "status":spec.status,
            "detail":spec.detail,
            "code":spec.code,
        })
        .to_string(),
    )
}

fn endpoint_response(status: u16, content_type: &str, body: String) -> String {
    json!({
        "status":status,
        "headers":[{"name":"content-type","value":content_type}],
        "body":STANDARD.encode(body.as_bytes()),
    })
    .to_string()
}

export!(GuestComponent);

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: String) -> serde_json::Value {
        serde_json::from_str(&value).unwrap()
    }

    #[test]
    fn descriptor_is_valid_trusted_v2_json() {
        let descriptor = parse(GuestComponent::describe());
        assert_eq!(descriptor["abi"], "lenso.json-request@1");
        assert_eq!(descriptor["capabilities"][0]["capability_id"], "lenso.http.endpoint@1");
        assert_eq!(descriptor["capabilities"][0]["descriptor_version"], "1.1.0");
        assert_eq!(descriptor["capabilities"][0]["descriptor_digest"], "sha256:701deedf705cb1a3b2f35fcae72f20ae85d46c6da6a008405a519018bbcdd3fe");
    }

    #[test]
    fn prepare_read_and_cas_preserve_optional_key_wire_shape() {
        let read = GuestComponent::prepare_settings(
            json!({"schema":"lenso.knowledge-settings-prepare.v1", "route_id":"knowledge-base.settings.read"})
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            parse(read),
            json!({"schema":"lenso.knowledge-settings-command.v1", "kind":"read"})
        );

        let request = json!({
            "schema":"lenso.knowledge-settings-prepare.v1",
            "route_id":"knowledge-base.settings.update",
            "body":{"excerpt_limit":48,"predecessor_revision":0}
        });
        let without_key = parse(GuestComponent::prepare_settings(request.to_string()).unwrap());
        assert_eq!(without_key["kind"], "cas");
        assert_eq!(without_key["predecessor_revision"], 0);
        assert!(without_key.get("idempotency_key").is_none());
        let mut with_key = request;
        with_key["idempotency_key"] = json!("request-001");
        let with_key = parse(GuestComponent::prepare_settings(with_key.to_string()).unwrap());
        assert_eq!(with_key["idempotency_key"], "request-001");
        assert_eq!(with_key["payload_sha256"], without_key["payload_sha256"]);
    }

    #[test]
    fn invalid_key_and_limit_are_guest_owned_400_responses() {
        for (field, value, code) in [
            (
                "idempotency_key",
                json!("two,keys"),
                "invalid_idempotency_key",
            ),
            (
                "body",
                json!({"excerpt_limit":15,"predecessor_revision":1}),
                "invalid_excerpt_limit",
            ),
        ] {
            let mut request = json!({
                "schema":"lenso.knowledge-settings-prepare.v1",
                "route_id":"knowledge-base.settings.update",
                "body":{"excerpt_limit":48,"predecessor_revision":1}
            });
            request[field] = value;
            let rejection =
                parse(GuestComponent::prepare_settings(request.to_string()).unwrap_err());
            assert_eq!(rejection["status"], 400);
            let body = STANDARD
                .decode(rejection["body"].as_str().unwrap())
                .unwrap();
            let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(problem["code"], code);
        }
    }

    #[test]
    fn complete_maps_typed_results_and_rejects_impossible_read_conflict() {
        let ok = parse(GuestComponent::complete_settings(
            json!({
                "schema":"lenso.knowledge-settings-complete.v1",
                "route_id":"knowledge-base.settings.read",
                "result":{"schema":"lenso.knowledge-settings-result.v1","kind":"ok", "settings":{"excerpt_limit":48,"revision":2}}
            })
            .to_string(),
        )
        .unwrap());
        assert_eq!(ok["status"], 200);
        let body = STANDARD.decode(ok["body"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            json!({"excerpt_limit":48,"revision":2})
        );

        let conflict = parse(GuestComponent::complete_settings(
            json!({
                "schema":"lenso.knowledge-settings-complete.v1",
                "route_id":"knowledge-base.settings.update",
                "result":{"schema":"lenso.knowledge-settings-result.v1","kind":"idempotency_conflict"}
            })
            .to_string(),
        )
        .unwrap());
        assert_eq!(conflict["status"], 409);

        assert_eq!(
            GuestComponent::complete_settings(
                json!({
                    "schema":"lenso.knowledge-settings-complete.v1",
                    "route_id":"knowledge-base.settings.read",
                    "result":{"schema":"lenso.knowledge-settings-result.v1","kind":"stale_revision"}
                })
                .to_string()
            ),
            Err("invalid_host_result".to_owned())
        );
        assert_eq!(
            GuestComponent::invoke(
                "lenso.http.endpoint@1".to_owned(),
                "handle".to_owned(),
                "{}".to_owned()
            ),
            Err("settings_host_binding_required".to_owned())
        );
    }
}
