use super::{Config, ManagementWeb, PrivateResponse, denied, domain_problem, unavailable};
use lenso_auth_sdk::{
    ActorAssertion, ActorProjectionError, FixedClock, TypedActor,
    delegation::ScopedDelegationBinding, realm::RealmAssertionVerifier,
};
use lenso_capability_auth_delegation as owner;
use lenso_capability_http_endpoint::{HandleRequest, prelude::*};
use lenso_capability_management as management;
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SessionProfile {
    realm: String,
    issuer: String,
    public_key: String,
    max_assertion_ttl_seconds: u32,
    task_id: String,
    agent_session_id: String,
    delegate_caller: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GrantEntry {
    permission: String,
    entry_id: String,
    scope_kind: String,
    scope_id: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IssuanceProfile {
    deployment: String,
    task_id: String,
    agent_session_id: String,
    delegate_caller: String,
    entries: Vec<GrantEntry>,
    audience: Vec<String>,
    max_ttl_seconds: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GrantInput {
    idempotency_key: String,
    expires_at: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptInput {
    idempotency_key: String,
}
fn binding(task: &str, session: &str, caller: &str) -> ScopedDelegationBinding {
    ScopedDelegationBinding {
        task_id: task.into(),
        agent_session_id: session.into(),
        delegate_caller: caller.into(),
    }
}
fn session_profile(config: &Config) -> Result<Option<SessionProfile>, lenso::RuntimeFailure> {
    config
        .delegated_session_json
        .as_deref()
        .map(parse_policy)
        .transpose()
}
fn issuance_profile(config: &Config) -> Result<Option<IssuanceProfile>, lenso::RuntimeFailure> {
    config
        .scoped_issuance_json
        .as_deref()
        .map(parse_policy)
        .transpose()
}
fn parse_policy<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, lenso::RuntimeFailure> {
    if value.len() > 8192 {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "Scoped policy exceeds its Host configuration bound".into(),
        });
    }
    serde_json::from_str(value).map_err(|_| lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "Scoped policy must be a complete object with known fields".into(),
    })
}
pub(super) fn validate(config: &Config) -> Result<(), lenso::RuntimeFailure> {
    let invalid = || {
        lenso::RuntimeFailure::InvalidResolvedPlan { detail: "Scoped delegation requires a fixed Native Account profile and admitted Management entries".into() }
    };
    let combined = config.delegated_management_path.as_deref() == Some("/agent");
    if config.delegated_management_path.is_some() && !combined
        || combined && (config.credential_scheme != "session" || !config.human_login)
        || ((config.credential_scheme == "delegated_session") || combined)
            != config.delegated_session_json.is_some()
    {
        return Err(invalid());
    }
    if let Some(profile) = session_profile(config)?
        && (config.human_login && !combined
            || profile.realm != "operators"
            || binding(
                &profile.task_id,
                &profile.agent_session_id,
                &profile.delegate_caller,
            )
            .validate()
            .is_err()
            || RealmAssertionVerifier::new(
                &profile.realm,
                &profile.issuer,
                &profile.public_key,
                profile.max_assertion_ttl_seconds,
                None,
            )
            .is_err())
    {
        return Err(invalid());
    }
    if let Some(profile) = issuance_profile(config)? {
        let audience = ["catalog", "invoke", "status"]
            .map(|operation| lenso_auth_sdk::audience(management::CAPABILITY_ID, operation));
        if !config.human_login
            || config.credential_scheme != "session"
            || binding(
                &profile.task_id,
                &profile.agent_session_id,
                &profile.delegate_caller,
            )
            .validate()
            .is_err()
            || profile.deployment.is_empty()
            || profile.entries.is_empty()
            || profile.entries.len() > 32
            || profile.max_ttl_seconds == 0
            || profile.max_ttl_seconds > 900
            || profile
                .entries
                .iter()
                .map(|entry| &entry.permission)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != profile.entries.len()
            || profile
                .audience
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != profile.audience.len()
            || profile.audience.is_empty()
            || profile
                .audience
                .iter()
                .any(|entry| !audience.contains(entry))
            || profile.entries.iter().any(|entry| {
                [
                    &entry.permission,
                    &entry.entry_id,
                    &entry.scope_kind,
                    &entry.scope_id,
                ]
                .iter()
                .any(|value| value.is_empty() || value.contains('*'))
            })
        {
            return Err(invalid());
        }
    }
    Ok(())
}
struct ScopedActor(ScopedDelegationBinding);
impl TypedActor for ScopedActor {
    fn from_assertion(assertion: &ActorAssertion) -> Result<Self, ActorProjectionError> {
        if assertion.actor_kind() != "user" {
            return Err(ActorProjectionError::UnexpectedActorKind {
                expected: "user".into(),
                actual: assertion.actor_kind().into(),
            });
        }
        ScopedDelegationBinding::from_assertion(assertion)
            .map(Self)
            .map_err(|_| lenso_auth_sdk::AssertionValidationError::InvalidProof.into())
    }
}
fn header<'a>(request: &'a HandleRequest, name: &str) -> Result<&'a str, PrivateResponse<Problem>> {
    let mut values = request
        .headers
        .iter()
        .filter(|value| value.name.eq_ignore_ascii_case(name));
    let first = values.next().ok_or_else(denied)?;
    if values.next().is_some() {
        return Err(denied());
    }
    Ok(&first.value)
}
pub(super) fn require_browser(
    provider: &ManagementWeb,
    request: &HandleRequest,
    subject: &str,
) -> Result<(), PrivateResponse<Problem>> {
    if provider.config.scoped_issuance_json.is_none()
        || !provider.config.human_login
        || provider.config.credential_scheme != "session"
        || Some(header(request, "origin")?) != provider.config.public_origin.as_deref()
        || header(request, "x-lenso-expected-subject")? != subject
    {
        return Err(denied());
    }
    Ok(())
}
pub(super) fn verify_session(
    config: &Config,
    context: &lenso::Ctx,
    request: &HandleRequest,
) -> Result<(), PrivateResponse<Problem>> {
    let profile = session_profile(config)
        .map_err(|_| unavailable())?
        .ok_or_else(denied)?;
    let path = if config.delegated_management_path.as_deref() == Some("/agent") {
        request.path.strip_prefix("/agent").ok_or_else(denied)?
    } else {
        request.path.as_str()
    };
    let operation = match (request.method.as_str(), path) {
        ("GET", "/management/catalog") => "catalog",
        ("POST", "/management/invoke") => "invoke",
        ("GET", path)
            if path
                .strip_prefix("/management/operations/")
                .is_some_and(|id| !id.is_empty() && id.len() <= 128 && !id.contains('/')) =>
        {
            "status"
        }
        _ => return Err(denied()),
    };
    let verifier = RealmAssertionVerifier::new(
        &profile.realm,
        &profile.issuer,
        &profile.public_key,
        profile.max_assertion_ttl_seconds,
        None,
    )
    .map_err(|_| unavailable())?;
    let ScopedActor(actual) = verifier
        .project_context::<ScopedActor>(
            context,
            management::CAPABILITY_ID,
            operation,
            &FixedClock::new(OffsetDateTime::now_utc()),
        )
        .map_err(|_| denied())?;
    let expected = binding(
        &profile.task_id,
        &profile.agent_session_id,
        &profile.delegate_caller,
    );
    if actual != expected
        || actual.task_id != header(request, "x-lenso-task-id")?
        || actual.agent_session_id != header(request, "x-lenso-agent-session-id")?
        || actual.delegate_caller != header(request, "x-lenso-delegate-caller")?
    {
        return Err(denied());
    }
    Ok(())
}
async fn current_entries(
    provider: &ManagementWeb,
    context: &lenso::Ctx,
    profile: &IssuanceProfile,
) -> Result<(), PrivateResponse<Problem>> {
    let catalog = provider
        .management
        .catalog_with_context(context.clone(), management::CatalogRequest {})
        .await
        .map_err(|error| match error {
            management::ManagementCatalogInvocationError::Domain(
                management::CatalogError::PermissionDenied,
            ) => denied(),
            _ => unavailable(),
        })?;
    if profile.entries.iter().any(|required| {
        !catalog
            .entries
            .iter()
            .any(|entry| entry.id == required.entry_id)
    }) {
        return Err(denied());
    }
    Ok(())
}
pub(super) async fn grant(
    provider: &ManagementWeb,
    context: lenso::Ctx,
    input: GrantInput,
) -> Result<PrivateResponse<Json<owner::GrantScopedResponse>>, PrivateResponse<Problem>> {
    let profile = issuance_profile(&provider.config)
        .map_err(|_| unavailable())?
        .ok_or_else(denied)?;
    let now = OffsetDateTime::now_utc();
    let expiry = OffsetDateTime::parse(&input.expires_at, &Rfc3339)
        .map_err(|_| domain_problem(StatusCode::BAD_REQUEST, "invalid_input"))?;
    if expiry <= now || expiry > now + time::Duration::seconds(i64::from(profile.max_ttl_seconds)) {
        return Err(domain_problem(StatusCode::BAD_REQUEST, "invalid_input"));
    }
    current_entries(provider, &context, &profile).await?;
    let mut scopes: Vec<owner::ResourceScope> = vec![];
    for entry in &profile.entries {
        if !scopes
            .iter()
            .any(|scope| scope.kind == entry.scope_kind && scope.id == entry.scope_id)
        {
            scopes.push(owner::ResourceScope {
                kind: entry.scope_kind.clone(),
                id: entry.scope_id.clone(),
            });
        }
    }
    let reply = provider
        .delegation
        .as_ref()
        .ok_or_else(denied)?
        .grant_scoped_with_context(
            context.clone(),
            owner::GrantScopedRequest {
                idempotency_key: input.idempotency_key,
                task_id: profile.task_id.clone(),
                agent_session_id: profile.agent_session_id.clone(),
                delegate_caller: profile.delegate_caller.clone(),
                deployment: profile.deployment.clone(),
                permissions: profile
                    .entries
                    .iter()
                    .map(|entry| entry.permission.clone())
                    .collect(),
                resource_scopes: scopes,
                audience: profile.audience.clone(),
                expires_at: input.expires_at,
            },
        )
        .await
        .map_err(|error| match error {
            owner::DelegationGrantScopedInvocationError::Domain(
                owner::GrantScopedError::PermissionDenied,
            ) => denied(),
            owner::DelegationGrantScopedInvocationError::Domain(
                owner::GrantScopedError::Conflict,
            ) => domain_problem(StatusCode::CONFLICT, "changed_intent"),
            owner::DelegationGrantScopedInvocationError::Domain(
                owner::GrantScopedError::InvalidRequest,
            ) => domain_problem(StatusCode::BAD_REQUEST, "invalid_input"),
            _ => unavailable(),
        })?;
    if context.is_cancelled() {
        return Err(unavailable());
    }
    current_entries(provider, &context, &profile)
        .await
        .map_err(|_| unavailable())?;
    Ok(PrivateResponse(Json(reply)))
}
pub(super) async fn receipt(
    provider: &ManagementWeb,
    context: lenso::Ctx,
    input: ReceiptInput,
) -> Result<PrivateResponse<Json<owner::ScopedReceiptResponse>>, PrivateResponse<Problem>> {
    let profile = issuance_profile(&provider.config)
        .map_err(|_| unavailable())?
        .ok_or_else(denied)?;
    current_entries(provider, &context, &profile).await?;
    provider
        .delegation
        .as_ref()
        .ok_or_else(denied)?
        .scoped_receipt_with_context(
            context,
            owner::ScopedReceiptRequest {
                idempotency_key: input.idempotency_key,
                task_id: profile.task_id.clone(),
                agent_session_id: profile.agent_session_id.clone(),
            },
        )
        .await
        .map(|reply| PrivateResponse(Json(reply)))
        .map_err(|error| match error {
            owner::DelegationScopedReceiptInvocationError::Domain(
                owner::ScopedReceiptError::PermissionDenied,
            ) => denied(),
            owner::DelegationScopedReceiptInvocationError::Domain(
                owner::ScopedReceiptError::InvalidRequest,
            ) => domain_problem(StatusCode::BAD_REQUEST, "invalid_input"),
            _ => unavailable(),
        })
}
