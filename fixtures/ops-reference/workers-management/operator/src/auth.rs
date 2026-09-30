use crate::{label, unavailable};
use lenso_auth_api_token_plugin::{ApiTokenAuthOperator, IssueApiToken, workers::D1Binding};
use lenso_auth_sdk::credential::{
    CredentialBinding, MANAGEMENT_CEILING_CLAIM, ManagementCredentialCeiling,
    ManagementResourceScope,
};
use serde::Deserialize;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FixtureActorKind {
    #[default]
    User,
    ServiceAccount,
}

impl FixtureActorKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ServiceAccount => "service-account",
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Issue {
        subject: String,
        #[serde(default)]
        actor_kind: FixtureActorKind,
        deployment: String,
        permissions: Vec<String>,
        resource_scopes: Vec<ManagementResourceScope>,
        audience: Vec<String>,
        expires_at: String,
    },
    Metadata {
        subject: String,
        deployment: String,
    },
    RevokeToken {
        credential_id: String,
    },
    RevokeSession {
        session_id: String,
    },
    Attenuate {
        binding: CredentialBinding,
        ceiling: ManagementCredentialCeiling,
    },
}

#[wasm_bindgen]
pub fn auth_public_key(signing_reference_value: String) -> Result<String, JsValue> {
    let secret = Zeroizing::new(signing_reference_value);
    if !(32..=8192).contains(&secret.len()) {
        return Err(unavailable());
    }
    Ok(lenso_auth_api_token_plugin::assertion_public_key(
        secret.as_bytes(),
    ))
}

#[wasm_bindgen]
pub async fn setup_auth(batch: js_sys::Function) -> Result<(), JsValue> {
    lenso_auth_api_token_plugin::migration::setup(&D1Binding::new("AUTH_DB", batch))
        .await
        .map_err(|_| unavailable())
}

#[wasm_bindgen]
pub async fn verify_auth(batch: js_sys::Function) -> Result<(), JsValue> {
    lenso_auth_api_token_plugin::migration::verify(&D1Binding::new("AUTH_DB", batch))
        .await
        .map_err(|_| unavailable())
}

#[wasm_bindgen]
pub async fn operate_auth(
    batch: js_sys::Function,
    request: String,
    pepper: String,
) -> Result<String, JsValue> {
    let request: Request = serde_json::from_str(&request).map_err(|_| unavailable())?;
    let owner = ApiTokenAuthOperator::connect_workers(D1Binding::new("AUTH_DB", batch))
        .await
        .map_err(|_| unavailable())?;
    let pepper = Zeroizing::new(pepper);
    let result = match request {
        Request::Issue {
            subject,
            actor_kind,
            deployment,
            permissions,
            resource_scopes,
            audience,
            expires_at,
        } => {
            if !label(&subject) {
                return Err(unavailable());
            }
            let ceiling = ManagementCredentialCeiling {
                deployment,
                permissions,
                resource_scopes,
            };
            ceiling.validate().map_err(|_| unavailable())?;
            let expires_at =
                OffsetDateTime::parse(&expires_at, &Rfc3339).map_err(|_| unavailable())?;
            // The private fixture selects existing Actor kinds for admission negatives.
            let issued = owner
                .issue(
                    pepper.as_bytes(),
                    IssueApiToken {
                        subject,
                        actor_kind: actor_kind.as_str().into(),
                        assurance: "operator-issued".into(),
                        audience,
                        claims: [(MANAGEMENT_CEILING_CLAIM.into(), json!(ceiling))]
                            .into_iter()
                            .collect(),
                        expires_at,
                    },
                )
                .await
                .map_err(|_| unavailable())?;
            json!({"credential_id":issued.token_id(),"session_id":issued.session_id(),"secret":issued.expose_secret()})
        }
        Request::Metadata {
            subject,
            deployment,
        } => {
            if !label(&subject) || !label(&deployment) {
                return Err(unavailable());
            }
            let rows = owner
                .list_management_credentials(&subject, &deployment, 100, None)
                .await
                .map_err(|_| unavailable())?;
            let rows = rows
                .into_iter()
                .map(|row| {
                    Ok(json!({
                        "credential_id":row.credential_id,"session_id":row.session_id,
                        "subject":row.subject,"actor_kind":row.actor_kind,"active":row.active,
                        "expires_at":row.expires_at.format(&Rfc3339).map_err(|_|unavailable())?,
                        "created_at":row.created_at.format(&Rfc3339).map_err(|_|unavailable())?,
                        "last_used_at":row.last_used_at.map(|time|time.format(&Rfc3339)).transpose().map_err(|_|unavailable())?,
                        "revoked_at":row.revoked_at.map(|time|time.format(&Rfc3339)).transpose().map_err(|_|unavailable())?,
                    }))
                })
                .collect::<Result<Vec<_>, JsValue>>()?;
            json!({"credentials":rows})
        }
        Request::RevokeToken { credential_id } => {
            if !label(&credential_id) {
                return Err(unavailable());
            }
            json!({"changed":owner.revoke_token(&credential_id).await.map_err(|_|unavailable())?})
        }
        Request::RevokeSession { session_id } => {
            if !label(&session_id) {
                return Err(unavailable());
            }
            json!({"changed":owner.revoke_session(&session_id).await.map_err(|_|unavailable())?})
        }
        Request::Attenuate { binding, ceiling } => {
            json!({"changed":owner.attenuate_management_credential(&binding,&ceiling).await.map_err(|_|unavailable())?})
        }
    };
    serde_json::to_string(&result).map_err(|_| unavailable())
}
