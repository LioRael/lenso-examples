use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{HeaderMap, header},
    routing::post,
};
use lenso_knowledge_base_reference_app::{
    settings_core::{SettingsCommand, UpdateBusinessSettings, prepare_settings_command},
    settings_store::{SettingsStoreOutcome, compare_and_set_settings, get_or_create_settings},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use subtle::ConstantTimeEq;

const AUTH_POLICY_ENV: &str = "LENSO_KNOWLEDGE_BRIDGE_AUTH_POLICY";
const DATABASE_URL_ENV: &str = "LENSO_KNOWLEDGE_DATABASE_URL";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthPolicy {
    schema: String,
    expires_unix: u64,
    actors: Vec<Actor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Actor {
    token_sha256: String,
    owner_id: String,
}

struct AuthorizedActor {
    digest: [u8; 32],
    owner_id: String,
}

struct BridgeState {
    database: PgPool,
    actors: Vec<AuthorizedActor>,
    expires_unix: u64,
    expected_host: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    schema: String,
    request_id: String,
}

#[derive(Serialize)]
struct BridgeResult {
    schema: &'static str,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<lenso_knowledge_base_reference_app::settings_core::BusinessSettings>,
}

impl BridgeResult {
    fn new(kind: &'static str) -> Self {
        Self {
            schema: "lenso.knowledge-settings-result.v1",
            kind,
            settings: None,
        }
    }

    fn ok(settings: lenso_knowledge_base_reference_app::settings_core::BusinessSettings) -> Self {
        Self {
            schema: "lenso.knowledge-settings-result.v1",
            kind: "ok",
            settings: Some(settings),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auth_policy = env::var(AUTH_POLICY_ENV)?;
    let database_url = env::var(DATABASE_URL_ENV)?;
    let policy = read_policy(&auth_policy)?;
    let database = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let state = Arc::new(BridgeState {
        database,
        actors: policy.actors,
        expires_unix: policy.expires_unix,
        expected_host: format!("127.0.0.1:{}", address.port()),
    });
    let app = Router::new()
        .route("/v1/knowledge-settings/read", post(read))
        .route(
            "/v1/knowledge-settings/compare-and-set",
            post(compare_and_set),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();
    println!("Listening on http://127.0.0.1:{}", address.port());
    axum::serve(listener, app).await?;
    Ok(())
}

struct LoadedPolicy {
    actors: Vec<AuthorizedActor>,
    expires_unix: u64,
}

fn read_policy(path: &str) -> Result<LoadedPolicy, Box<dyn std::error::Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > 16_384 {
        return Err("bridge authorization policy must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err("bridge authorization policy must have mode 0600".into());
        }
    }
    let policy: AuthPolicy = serde_json::from_slice(&fs::read(path)?)?;
    if policy.schema != "lenso.knowledge-settings-auth-policy.v1" || policy.actors.len() != 2 {
        return Err("bridge authorization policy must contain exactly two local actors".into());
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if policy.expires_unix <= now || policy.expires_unix > now + 3600 {
        return Err("bridge authorization policy expiry must be within one hour".into());
    }
    let mut actors = Vec::with_capacity(2);
    for actor in policy.actors {
        if actor.owner_id.is_empty() || actor.owner_id.len() > 128 {
            return Err("bridge actor owner ID is invalid".into());
        }
        let digest = actor
            .token_sha256
            .strip_prefix("sha256:")
            .ok_or("bridge token digest must be SHA-256")?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("bridge token digest is invalid".into());
        }
        let digest = hex::decode(digest)?;
        actors.push(AuthorizedActor {
            digest: digest
                .try_into()
                .map_err(|_| "bridge token digest length changed")?,
            owner_id: actor.owner_id,
        });
    }
    if actors[0].owner_id == actors[1].owner_id || actors[0].digest == actors[1].digest {
        return Err("bridge authorization policy contains duplicate actors or tokens".into());
    }
    let mut owners = [actors[0].owner_id.as_str(), actors[1].owner_id.as_str()];
    owners.sort_unstable();
    if owners != ["user-a", "user-b"] {
        return Err("bridge authorization policy is limited to the two acceptance users".into());
    }
    Ok(LoadedPolicy {
        actors,
        expires_unix: policy.expires_unix,
    })
}

fn actor<'a>(state: &'a BridgeState, peer: SocketAddr, headers: &HeaderMap) -> Option<&'a str> {
    if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST)
        || headers.contains_key(header::ORIGIN)
        || headers.get(header::HOST)?.to_str().ok()? != state.expected_host
    {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now >= state.expires_unix {
        return None;
    }
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty()
        || token.len() > 4096
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b',')
    {
        return None;
    }
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    state
        .actors
        .iter()
        .find(|entry| bool::from(entry.digest.ct_eq(&digest)))
        .map(|entry| entry.owner_id.as_str())
}

fn valid_request_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        })
}

async fn read(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<BridgeState>>,
    headers: HeaderMap,
    Json(request): Json<ReadRequest>,
) -> Json<BridgeResult> {
    let Some(owner_id) = actor(&state, peer, &headers) else {
        return Json(BridgeResult::new("unauthorized"));
    };
    if request.schema != "lenso.knowledge-settings-store.v1"
        || !valid_request_id(&request.request_id)
    {
        return Json(BridgeResult::new("storage_unavailable"));
    }
    match tokio::time::timeout(
        Duration::from_millis(900),
        get_or_create_settings(&state.database, owner_id),
    )
    .await
    {
        Ok(Ok(settings)) => Json(BridgeResult::ok(settings)),
        Ok(Err(_)) | Err(_) => Json(BridgeResult::new("storage_unavailable")),
    }
}

async fn compare_and_set(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<BridgeState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<BridgeResult> {
    let Some(owner_id) = actor(&state, peer, &headers) else {
        return Json(BridgeResult::new("unauthorized"));
    };
    let Some(command) = decode_cas_request(&request) else {
        return Json(BridgeResult::new("storage_unavailable"));
    };
    match tokio::time::timeout(
        Duration::from_millis(900),
        compare_and_set_settings(&state.database, owner_id, &command),
    )
    .await
    {
        Ok(Ok(SettingsStoreOutcome::Applied(settings))) => Json(BridgeResult::ok(settings)),
        Ok(Ok(SettingsStoreOutcome::StaleRevision)) => Json(BridgeResult::new("stale_revision")),
        Ok(Ok(SettingsStoreOutcome::IdempotencyConflict)) => {
            Json(BridgeResult::new("idempotency_conflict"))
        }
        Ok(Err(_)) | Err(_) => Json(BridgeResult::new("storage_unavailable")),
    }
}

fn decode_cas_request(value: &Value) -> Option<SettingsCommand> {
    let root = value.as_object()?;
    if root.len() != 3
        || root.get("schema")?.as_str()? != "lenso.knowledge-settings-store.v1"
        || !valid_request_id(root.get("request_id")?.as_str()?)
    {
        return None;
    }
    let command = root.get("command")?.as_object()?;
    if !(command.len() == 3 || command.len() == 4)
        || !command.keys().all(|key| {
            matches!(
                key.as_str(),
                "excerpt_limit" | "predecessor_revision" | "payload_sha256" | "idempotency_key"
            )
        })
    {
        return None;
    }
    let excerpt_limit = command.get("excerpt_limit")?.as_i64()?;
    let predecessor_revision = command.get("predecessor_revision")?.as_i64()?;
    let idempotency_key = match command.get("idempotency_key") {
        Some(value) => Some(value.as_str()?),
        None => None,
    };
    let payload_sha256 = command.get("payload_sha256")?.as_str()?;
    let expected = prepare_settings_command(
        UpdateBusinessSettings {
            excerpt_limit,
            predecessor_revision,
        },
        idempotency_key,
    )
    .ok()?;
    if expected.payload_sha256 != payload_sha256 {
        return None;
    }
    Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REQUEST_ID: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn store_request_accepts_only_the_canonical_guest_command() {
        let command = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 48,
                predecessor_revision: 0,
            },
            None,
        )
        .unwrap();
        let mut request = json!({
            "schema":"lenso.knowledge-settings-store.v1",
            "request_id":REQUEST_ID,
            "command":{
                "excerpt_limit":48,
                "predecessor_revision":0,
                "payload_sha256":command.payload_sha256,
            }
        });
        assert_eq!(decode_cas_request(&request), Some(command.clone()));
        request["command"]["idempotency_key"] = json!("request-001");
        let with_key = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 48,
                predecessor_revision: 0,
            },
            Some("request-001"),
        )
        .unwrap();
        assert_eq!(decode_cas_request(&request), Some(with_key));
        request["command"]["idempotency_key"] = Value::Null;
        assert!(decode_cas_request(&request).is_none());
        request["command"]
            .as_object_mut()
            .unwrap()
            .remove("idempotency_key");
        request["command"]["payload_sha256"] = json!("sha256:0000");
        assert!(decode_cas_request(&request).is_none());
    }

    #[test]
    fn request_id_and_extra_fields_are_rejected() {
        assert!(valid_request_id(REQUEST_ID));
        assert!(!valid_request_id("not-a-uuid"));
        let command = prepare_settings_command(
            UpdateBusinessSettings {
                excerpt_limit: 48,
                predecessor_revision: 1,
            },
            None,
        )
        .unwrap();
        let mut request = json!({
            "schema":"lenso.knowledge-settings-store.v1",
            "request_id":REQUEST_ID,
            "command":{
                "excerpt_limit":48,
                "predecessor_revision":1,
                "payload_sha256":command.payload_sha256,
                "owner_id":"user-a",
            }
        });
        assert!(decode_cas_request(&request).is_none());
        request["command"]
            .as_object_mut()
            .unwrap()
            .remove("owner_id");
        request["owner_id"] = json!("user-a");
        assert!(decode_cas_request(&request).is_none());
    }
}
