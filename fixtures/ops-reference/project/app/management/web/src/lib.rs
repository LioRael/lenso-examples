//! Optional authenticated HTTP projection; management and security authority remain at their ports.
mod delegation;
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_auth as auth;
use lenso_capability_auth_delegation as scoped;
use lenso_capability_credential_issuer as issuer;
use lenso_capability_http_endpoint::response::{HeaderValue, header};
use lenso_capability_http_endpoint::{
    ExtractorFuture, ExtractorRejection, FromRequest, prelude::*,
};
use lenso_capability_human_api_token as pat;
use lenso_capability_management as management;
use lenso_capability_management_human as human;
use lenso_capability_password_auth as password;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct Config {
    credential_scheme: String,
    #[serde(default)]
    human_login: bool,
    #[serde(default)]
    public_origin: Option<String>,
    #[serde(default)]
    delegated_session_json: Option<String>,
    #[serde(default)]
    delegated_management_path: Option<String>,
    #[serde(default)]
    scoped_issuance_json: Option<String>,
}
fn validate(config: &Config) -> Result<(), lenso::RuntimeFailure> {
    if !matches!(
        config.credential_scheme.as_str(),
        "bearer" | "session" | "delegated_session"
    ) {
        return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
            detail: "Management HTTP requires an explicit bearer or session profile".into(),
        });
    }
    if config.human_login {
        let origin = config
            .public_origin
            .as_ref()
            .and_then(|value| url::Url::parse(value).ok())
            .ok_or_else(|| lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Browser login requires a frozen public origin".into(),
            })?;
        if config.credential_scheme != "session"
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || config.public_origin.as_deref()
                != Some(origin.origin().ascii_serialization().as_str())
            || !(origin.scheme() == "https"
                || (origin.scheme() == "http"
                    && matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
        {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail: "Browser session profile requires HTTPS or explicit loopback origin".into(),
            });
        }
    }
    delegation::validate(config)
}
#[lenso::plugin(validate = validate)]
#[derive(Clone, Debug)]
struct ManagementWeb {
    #[config]
    config: Config,
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "management")]
    management: management::ManagementClient,
    #[dependency(id = "human")]
    human: human::ManagementHumanClient,
    #[dependency(id = "password")]
    password: Option<password::PasswordClient>,
    #[dependency(id = "account_issuer")]
    credential_issuer: Option<issuer::CredentialIssuerClient>,
    #[dependency(id = "human_tokens")]
    human_tokens: Option<pat::HumanApiTokenClient>,
    #[dependency(id = "delegation")]
    delegation: Option<scoped::DelegationClient>,
}
#[lenso::plugin_impl]
impl ManagementWeb {
    #[create]
    #[allow(
        clippy::too_many_arguments,
        reason = "Named construction inputs must match the Source Plugin fields"
    )]
    fn create(
        config: Config,
        auth: auth::AuthClient,
        management: management::ManagementClient,
        human: human::ManagementHumanClient,
        password: Option<password::PasswordClient>,
        credential_issuer: Option<issuer::CredentialIssuerClient>,
        human_tokens: Option<pat::HumanApiTokenClient>,
        delegation: Option<scoped::DelegationClient>,
    ) -> Result<Self, &'static str> {
        let owner = Self {
            config,
            auth,
            management,
            human,
            password,
            credential_issuer,
            human_tokens,
            delegation,
        };
        owner
            .validate_bindings()
            .map_err(|_| "Management HTTP profile has incompatible owner bindings")?;
        Ok(owner)
    }
}
impl ManagementWeb {
    fn validate_bindings(&self) -> Result<(), lenso::RuntimeFailure> {
        let expected = usize::from(self.config.human_login);
        if [
            self.password.iter().count(),
            self.credential_issuer.iter().count(),
            self.human_tokens.iter().count(),
        ]
        .iter()
        .any(|count| *count != expected)
        {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan { detail: "Browser profile requires exactly one selected Password, Account issuer and guarded human token port".into() });
        }
        if self.delegation.is_some() != self.config.scoped_issuance_json.is_some() {
            return Err(lenso::RuntimeFailure::InvalidResolvedPlan {
                detail:
                    "Scoped issuance requires its separately bound Account delegation capability"
                        .into(),
            });
        }
        Ok(())
    }
}
struct Authenticated(lenso::Ctx, String);
struct PrivateResponse<T>(T);
impl<T: IntoResponse> IntoResponse for PrivateResponse<T> {
    fn into_response(
        self,
    ) -> Result<HandleResponse, lenso_capability_http_endpoint::response::ResponseBuildError> {
        self.0.into_response()?.with_header(
            &lenso_capability_http_endpoint::response::header::CACHE_CONTROL,
            &lenso_capability_http_endpoint::response::HeaderValue::from_static("no-store"),
        )
    }
}
fn unavailable() -> PrivateResponse<Problem> {
    PrivateResponse(Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "owner_unavailable",
        "The owner reply or audit delivery requires receipt recovery",
    ))
}
fn domain_problem(status: StatusCode, code: &'static str) -> PrivateResponse<Problem> {
    PrivateResponse(Problem::new(
        status,
        code,
        "The current management request was rejected",
    ))
}
fn denied() -> PrivateResponse<Problem> {
    PrivateResponse(Problem::new(
        StatusCode::FORBIDDEN,
        "operators_required",
        "The current operators request was rejected",
    ))
}
impl Authenticated {
    async fn extract(
        provider: &ManagementWeb,
        context: &mut lenso::Ctx,
        request: &HandleRequest,
        delegated: bool,
    ) -> Result<Self, ExtractorRejection> {
        let reject = || {
            ExtractorRejection::Response(
                PrivateResponse(Problem::new(
                    StatusCode::UNAUTHORIZED,
                    "session_required",
                    "A current operators session is required",
                ))
                .into_response()
                .expect("static problem"),
            )
        };
        let credential = request
            .credential
            .as_ref()
            .filter(|value| {
                value.scheme
                    == if delegated {
                        "bearer"
                    } else {
                        &provider.config.credential_scheme
                    }
                    && value.value.len() <= 8192
            })
            .ok_or_else(reject)?;
        let response = provider
            .auth
            .authenticate_with_context(
                context.clone(),
                authenticate_request(Some(CredentialEvidence::new(
                    if delegated {
                        "session"
                    } else {
                        &provider.config.credential_scheme
                    },
                    credential.value.clone(),
                ))),
            )
            .await
            .map_err(|error| match error {
                auth::AuthInvocationError::Domain(auth::AuthenticateError::Unknown(_))
                | auth::AuthInvocationError::Runtime(_) => ExtractorRejection::Response(
                    unavailable().into_response().expect("static problem"),
                ),
                auth::AuthInvocationError::Domain(_) => reject(),
            })?;
        let AuthOutcome::Authenticated(assertion) =
            decode_auth_response(response).map_err(|_| {
                ExtractorRejection::Response(unavailable().into_response().expect("static problem"))
            })?
        else {
            return Err(reject());
        };
        if !delegated
            && assertion.to_wire().claims.as_ref().is_some_and(|claims| {
                claims.contains_key(lenso_auth_sdk::delegation::SCOPED_DELEGATION_CLAIM)
            })
        {
            return Err(reject());
        }
        let mut expected = request
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case("x-lenso-expected-subject"));
        if let Some(header) = expected.next()
            && (expected.next().is_some() || header.value != assertion.subject())
        {
            return Err(ExtractorRejection::Response(
                domain_problem(StatusCode::PRECONDITION_FAILED, "session_changed")
                    .into_response()
                    .expect("static problem"),
            ));
        }
        *context = assertion.attach(context.clone()).map_err(|_| {
            ExtractorRejection::Response(unavailable().into_response().expect("static problem"))
        })?;
        if delegated {
            delegation::verify_session(&provider.config, context, request).map_err(|problem| {
                ExtractorRejection::Response(problem.into_response().expect("static problem"))
            })?;
        }
        Ok(Self(context.clone(), assertion.subject().to_owned()))
    }
}
impl FromRequest<ManagementWeb> for Authenticated {
    fn from_request<'a>(
        provider: &'a ManagementWeb,
        context: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(Self::extract(
            provider,
            context,
            request,
            provider.config.credential_scheme == "delegated_session",
        ))
    }
}
struct DelegatedAuthenticated(Authenticated);
impl FromRequest<ManagementWeb> for DelegatedAuthenticated {
    fn from_request<'a>(
        provider: &'a ManagementWeb,
        context: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            if provider.config.delegated_management_path.as_deref() != Some("/agent") {
                return Err(ExtractorRejection::Response(
                    domain_problem(StatusCode::NOT_FOUND, "not_found")
                        .into_response()
                        .expect("static problem"),
                ));
            }
            Authenticated::extract(provider, context, request, true)
                .await
                .map(Self)
        })
    }
}
struct DelegationContext(lenso::Ctx);
impl FromRequest<ManagementWeb> for DelegationContext {
    fn from_request<'a>(
        provider: &'a ManagementWeb,
        context: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let Authenticated(context, subject) =
                Authenticated::from_request(provider, context, request).await?;
            delegation::require_browser(provider, request, &subject).map_err(|problem| {
                ExtractorRejection::Response(problem.into_response().expect("static problem"))
            })?;
            Ok(Self(context))
        })
    }
}
struct LoginContext(lenso::Ctx);
impl FromRequest<ManagementWeb> for LoginContext {
    fn from_request<'a>(
        provider: &'a ManagementWeb,
        context: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let reject =
                || ExtractorRejection::Response(denied().into_response().expect("static problem"));
            if !provider.config.human_login
                || request
                    .headers
                    .iter()
                    .filter(|value| value.name.eq_ignore_ascii_case("origin"))
                    .count()
                    != 1
                || !request.headers.iter().any(|value| {
                    value.name.eq_ignore_ascii_case("origin")
                        && Some(value.value.as_str()) == provider.config.public_origin.as_deref()
                })
            {
                return Err(reject());
            }
            Ok(Self(context.clone()))
        })
    }
}
struct SessionCredential(String);
impl FromRequest<ManagementWeb> for SessionCredential {
    fn from_request<'a>(
        provider: &'a ManagementWeb,
        _: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let reject =
                || ExtractorRejection::Response(denied().into_response().expect("static problem"));
            if !provider.config.human_login {
                return Err(reject());
            }
            let credential = request
                .credential
                .as_ref()
                .filter(|value| value.scheme == "session" && value.value.len() <= 8192)
                .ok_or_else(reject)?;
            Ok(Self(credential.value.clone()))
        })
    }
}
fn cookie_response(
    session: Option<(&str, i64)>,
) -> Result<PrivateResponse<HandleResponse>, PrivateResponse<Problem>> {
    let mut response = Json(serde_json::json!({"authenticated":session.is_some()}))
        .into_response()
        .map_err(|_| unavailable())?;
    let (credential, ttl, csrf) = session.map_or_else(
        || ("".to_owned(), 0, String::new()),
        |(credential, ttl)| {
            (
                credential.to_owned(),
                ttl,
                uuid::Uuid::new_v4().simple().to_string(),
            )
        },
    );
    if !credential
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(unavailable());
    }
    for cookie in [
        format!(
            "__Host-lenso-session={credential}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={ttl}"
        ),
        format!("__Host-lenso-csrf={csrf}; Path=/; Secure; SameSite=Strict; Max-Age={ttl}"),
    ] {
        response = response
            .with_header(
                &header::SET_COOKIE,
                &HeaderValue::from_str(&cookie).map_err(|_| unavailable())?,
            )
            .map_err(|_| unavailable())?;
    }
    Ok(PrivateResponse(response))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationPath {
    operation_id: String,
}
#[endpoint]
impl ManagementWeb {
    #[get("ops.management.auth-methods", "/auth/methods")]
    async fn methods(
        &self,
    ) -> Result<PrivateResponse<Json<serde_json::Value>>, PrivateResponse<Problem>> {
        if !self.config.human_login {
            return Err(denied());
        }
        Ok(PrivateResponse(Json(
            serde_json::json!({"methods":[{"id":"password","kind":"password","label":"Password","action":"/auth/login"}],"csrf":{"cookie_name":"__Host-lenso-csrf","header_name":"x-csrf-token"}}),
        )))
    }
    #[post("ops.management.login", "/auth/login")]
    async fn login(
        &self,
        LoginContext(context): LoginContext,
        Json(request): Json<password::LoginRequest>,
    ) -> Result<PrivateResponse<HandleResponse>, PrivateResponse<Problem>> {
        let owner = self.password.iter().next().ok_or_else(unavailable)?;
        let reply =
            owner
                .login_with_context(context, request)
                .await
                .map_err(|error| match error {
                    password::PasswordLoginInvocationError::Runtime(_)
                    | password::PasswordLoginInvocationError::Domain(
                        password::LoginError::Unknown(_),
                    ) => unavailable(),
                    password::PasswordLoginInvocationError::Domain(
                        password::LoginError::RateLimited,
                    ) => domain_problem(StatusCode::TOO_MANY_REQUESTS, "login_rate_limited"),
                    password::PasswordLoginInvocationError::Domain(_) => denied(),
                })?;
        let expiry = time::OffsetDateTime::parse(
            &reply.expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| unavailable())?;
        let ttl = (expiry - time::OffsetDateTime::now_utc())
            .whole_seconds()
            .clamp(0, 3600);
        if ttl == 0 {
            return Err(unavailable());
        }
        cookie_response(Some((&reply.credential, ttl)))
    }
    #[post("ops.management.scoped-grant", "/auth/delegations/scoped")]
    async fn grant_scoped(
        &self,
        DelegationContext(context): DelegationContext,
        Json(input): Json<delegation::GrantInput>,
    ) -> Result<PrivateResponse<Json<scoped::GrantScopedResponse>>, PrivateResponse<Problem>> {
        delegation::grant(self, context, input).await
    }
    #[post("ops.management.scoped-receipt", "/auth/delegations/scoped/receipt")]
    async fn scoped_receipt(
        &self,
        DelegationContext(context): DelegationContext,
        Json(input): Json<delegation::ReceiptInput>,
    ) -> Result<PrivateResponse<Json<scoped::ScopedReceiptResponse>>, PrivateResponse<Problem>>
    {
        delegation::receipt(self, context, input).await
    }
    #[post("ops.management.logout", "/auth/logout")]
    async fn logout(
        &self,
        Authenticated(context, _): Authenticated,
        SessionCredential(credential): SessionCredential,
    ) -> Result<PrivateResponse<HandleResponse>, PrivateResponse<Problem>> {
        self.credential_issuer
            .iter()
            .next()
            .ok_or_else(unavailable)?
            .revoke_credential_with_context(
                context,
                issuer::RevokeCredentialRequest {
                    credential,
                    scheme: "session".into(),
                },
            )
            .await
            .map_err(|error| match error {
                issuer::CredentialIssuerRevokeCredentialInvocationError::Runtime(_)
                | issuer::CredentialIssuerRevokeCredentialInvocationError::Domain(
                    issuer::RevokeCredentialError::Unknown(_),
                ) => unavailable(),
                issuer::CredentialIssuerRevokeCredentialInvocationError::Domain(_) => denied(),
            })?;
        cookie_response(None)
    }
    #[get("ops.management.session", "/api/console/v1/session")]
    async fn session(
        &self,
        Authenticated(context, subject): Authenticated,
    ) -> Result<PrivateResponse<Json<serde_json::Value>>, PrivateResponse<Problem>> {
        if !self.config.human_login {
            return Err(denied());
        }
        let admitted = self
            .management
            .catalog_with_context(context, management::CatalogRequest {})
            .await
            .map_err(|error| match error {
                management::ManagementCatalogInvocationError::Runtime(_)
                | management::ManagementCatalogInvocationError::Domain(
                    management::CatalogError::Unavailable,
                ) => unavailable(),
                management::ManagementCatalogInvocationError::Domain(_) => denied(),
            })?;
        if admitted.entries.is_empty() {
            return Err(denied());
        }
        Ok(PrivateResponse(Json(
            serde_json::json!({"mode":"required","authenticated":true,"subject":subject,"administrator":false,"management_enabled":true,"human_management_enabled":true,"workspace_ids":[]}),
        )))
    }
    #[post("ops.management.pat-issue", "/api/console/v1/human-tokens/issue")]
    async fn pat_issue(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<pat::IssueRequest>,
    ) -> Result<PrivateResponse<Json<pat::IssueResponse>>, PrivateResponse<Problem>> {
        self.human_tokens
            .iter()
            .next()
            .ok_or_else(denied)?
            .issue_with_context(context, request)
            .await
            .map(|value| PrivateResponse(Json(value)))
            .map_err(|error| match error {
                pat::HumanApiTokenIssueInvocationError::Domain(
                    pat::IssueError::PermissionDenied,
                ) => denied(),
                pat::HumanApiTokenIssueInvocationError::Domain(pat::IssueError::Conflict) => {
                    domain_problem(StatusCode::CONFLICT, "changed_intent")
                }
                pat::HumanApiTokenIssueInvocationError::Domain(pat::IssueError::InvalidRequest) => {
                    domain_problem(StatusCode::BAD_REQUEST, "invalid_input")
                }
                _ => unavailable(),
            })
    }
    #[post("ops.management.pat-list", "/api/console/v1/human-tokens/list")]
    async fn pat_list(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<pat::ListRequest>,
    ) -> Result<PrivateResponse<Json<pat::ListResponse>>, PrivateResponse<Problem>> {
        self.human_tokens
            .iter()
            .next()
            .ok_or_else(denied)?
            .list_with_context(context, request)
            .await
            .map(|value| PrivateResponse(Json(value)))
            .map_err(|error| match error {
                pat::HumanApiTokenListInvocationError::Domain(pat::ListError::PermissionDenied) => {
                    denied()
                }
                pat::HumanApiTokenListInvocationError::Domain(pat::ListError::InvalidRequest) => {
                    domain_problem(StatusCode::BAD_REQUEST, "invalid_input")
                }
                _ => unavailable(),
            })
    }
    #[post("ops.management.pat-receipt", "/api/console/v1/human-tokens/receipt")]
    async fn pat_receipt(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<pat::ReceiptRequest>,
    ) -> Result<PrivateResponse<Json<pat::ReceiptResponse>>, PrivateResponse<Problem>> {
        self.human_tokens
            .iter()
            .next()
            .ok_or_else(denied)?
            .receipt_with_context(context, request)
            .await
            .map(|value| PrivateResponse(Json(value)))
            .map_err(|error| match error {
                pat::HumanApiTokenReceiptInvocationError::Domain(
                    pat::ReceiptError::PermissionDenied,
                ) => denied(),
                pat::HumanApiTokenReceiptInvocationError::Domain(
                    pat::ReceiptError::InvalidRequest,
                ) => domain_problem(StatusCode::BAD_REQUEST, "invalid_input"),
                _ => unavailable(),
            })
    }
    #[post("ops.management.pat-revoke", "/api/console/v1/human-tokens/revoke")]
    async fn pat_revoke(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<pat::RevokeRequest>,
    ) -> Result<PrivateResponse<Json<pat::RevokeResponse>>, PrivateResponse<Problem>> {
        self.human_tokens
            .iter()
            .next()
            .ok_or_else(denied)?
            .revoke_with_context(context, request)
            .await
            .map(|value| PrivateResponse(Json(value)))
            .map_err(|error| match error {
                pat::HumanApiTokenRevokeInvocationError::Domain(
                    pat::RevokeError::PermissionDenied,
                ) => denied(),
                pat::HumanApiTokenRevokeInvocationError::Domain(
                    pat::RevokeError::InvalidRequest,
                ) => domain_problem(StatusCode::BAD_REQUEST, "invalid_input"),
                pat::HumanApiTokenRevokeInvocationError::Domain(pat::RevokeError::NotFound) => {
                    domain_problem(StatusCode::NOT_FOUND, "not_found")
                }
                _ => unavailable(),
            })
    }
    #[get("ops.management.catalog", "/management/catalog")]
    async fn catalog(
        &self,
        Authenticated(context, _): Authenticated,
    ) -> Result<PrivateResponse<Json<management::CatalogResponse>>, PrivateResponse<Problem>> {
        self.management
            .catalog_with_context(context, management::CatalogRequest {})
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(|error| match error {
                management::ManagementCatalogInvocationError::Domain(
                    management::CatalogError::PermissionDenied,
                ) => denied(),
                _ => unavailable(),
            })
    }
    #[get("ops.management.agent-catalog", "/agent/management/catalog")]
    async fn agent_catalog(
        &self,
        DelegatedAuthenticated(authenticated): DelegatedAuthenticated,
    ) -> Result<PrivateResponse<Json<management::CatalogResponse>>, PrivateResponse<Problem>> {
        self.catalog(authenticated).await
    }
    #[post("ops.management.agent-invoke", "/agent/management/invoke")]
    async fn agent_invoke(
        &self,
        DelegatedAuthenticated(authenticated): DelegatedAuthenticated,
        request: Json<management::InvokeRequest>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.invoke(authenticated, request).await
    }
    #[get(
        "ops.management.agent-status",
        "/agent/management/operations/{operation_id}"
    )]
    async fn agent_status(
        &self,
        DelegatedAuthenticated(authenticated): DelegatedAuthenticated,
        path: Path<OperationPath>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.status(authenticated, path).await
    }
    #[post("ops.management.invoke", "/management/invoke")]
    async fn invoke(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<management::InvokeRequest>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.management
            .invoke_with_context(context, request)
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(|error| match error {
                management::ManagementInvokeInvocationError::Domain(
                    management::InvokeError::PermissionDenied,
                ) => denied(),
                management::ManagementInvokeInvocationError::Domain(
                    management::InvokeError::Conflict,
                ) => domain_problem(StatusCode::CONFLICT, "changed_intent"),
                management::ManagementInvokeInvocationError::Domain(
                    management::InvokeError::InvalidInput,
                ) => domain_problem(StatusCode::UNPROCESSABLE_ENTITY, "invalid_input"),
                management::ManagementInvokeInvocationError::Domain(
                    management::InvokeError::NotFound,
                ) => domain_problem(StatusCode::NOT_FOUND, "not_found"),
                management::ManagementInvokeInvocationError::Domain(
                    management::InvokeError::DeadlineExceeded,
                ) => domain_problem(StatusCode::CONFLICT, "expired"),
                _ => unavailable(),
            })
    }
    #[get("ops.management.status", "/management/operations/{operation_id}")]
    async fn status(
        &self,
        Authenticated(context, _): Authenticated,
        Path(path): Path<OperationPath>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.management
            .status_with_context(
                context,
                management::StatusRequest {
                    operation_id: path.operation_id,
                },
            )
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(|error| match error {
                management::ManagementStatusInvocationError::Domain(
                    management::StatusError::PermissionDenied,
                ) => denied(),
                management::ManagementStatusInvocationError::Domain(
                    management::StatusError::NotFound,
                ) => domain_problem(StatusCode::NOT_FOUND, "not_found"),
                management::ManagementStatusInvocationError::Domain(
                    management::StatusError::Conflict,
                ) => domain_problem(StatusCode::CONFLICT, "changed_intent"),
                _ => unavailable(),
            })
    }
    #[get("ops.management.intent", "/human-management/intents/{operation_id}")]
    async fn intent(
        &self,
        Authenticated(context, _): Authenticated,
        Path(path): Path<OperationPath>,
    ) -> Result<PrivateResponse<Json<human::ReadIntentResponse>>, PrivateResponse<Problem>> {
        self.human
            .read_intent_with_context(
                context,
                human::ReadIntentRequest {
                    operation_id: path.operation_id,
                },
            )
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(|error| match error {
                human::ManagementHumanReadIntentInvocationError::Domain(
                    human::ReadIntentError::PermissionDenied,
                ) => denied(),
                human::ManagementHumanReadIntentInvocationError::Domain(
                    human::ReadIntentError::NotFound,
                ) => domain_problem(StatusCode::NOT_FOUND, "not_found"),
                human::ManagementHumanReadIntentInvocationError::Domain(
                    human::ReadIntentError::Conflict,
                ) => domain_problem(StatusCode::CONFLICT, "changed_intent"),
                _ => unavailable(),
            })
    }
    #[post("ops.management.decide", "/human-management/decide")]
    async fn decide(
        &self,
        Authenticated(context, _): Authenticated,
        Json(request): Json<human::DecideRequest>,
    ) -> Result<PrivateResponse<Json<human::DecideResponse>>, PrivateResponse<Problem>> {
        self.human
            .decide_with_context(context, request)
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(|error| match error {
                human::ManagementHumanDecideInvocationError::Domain(
                    human::DecideError::PermissionDenied,
                ) => denied(),
                human::ManagementHumanDecideInvocationError::Domain(
                    human::DecideError::NotFound,
                ) => domain_problem(StatusCode::NOT_FOUND, "not_found"),
                human::ManagementHumanDecideInvocationError::Domain(
                    human::DecideError::Conflict,
                ) => domain_problem(StatusCode::CONFLICT, "changed_intent"),
                _ => unavailable(),
            })
    }
    #[get("ops.management.ui-catalog", "/api/console/v1/management/catalog")]
    async fn ui_catalog(
        &self,
        authenticated: Authenticated,
    ) -> Result<PrivateResponse<Json<management::CatalogResponse>>, PrivateResponse<Problem>> {
        self.catalog(authenticated).await
    }

    #[post("ops.management.ui-invoke", "/api/console/v1/management/invoke")]
    async fn ui_invoke(
        &self,
        authenticated: Authenticated,
        Json(request): Json<management::InvokeRequest>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.invoke(authenticated, Json(request)).await
    }

    #[get(
        "ops.management.ui-status",
        "/api/console/v1/management/operations/{operation_id}"
    )]
    async fn ui_status(
        &self,
        authenticated: Authenticated,
        Path(path): Path<OperationPath>,
    ) -> Result<PrivateResponse<Json<management::InvokeResponse>>, PrivateResponse<Problem>> {
        self.status(authenticated, Path(path)).await
    }

    #[get(
        "ops.management.ui-intent",
        "/api/console/v1/human-management/intents/{operation_id}"
    )]
    async fn ui_intent(
        &self,
        authenticated: Authenticated,
        Path(path): Path<OperationPath>,
    ) -> Result<PrivateResponse<Json<human::ReadIntentResponse>>, PrivateResponse<Problem>> {
        self.intent(authenticated, Path(path)).await
    }

    #[post("ops.management.ui-decide", "/api/console/v1/human-management/decide")]
    async fn ui_decide(
        &self,
        authenticated: Authenticated,
        Json(request): Json<human::DecideRequest>,
    ) -> Result<PrivateResponse<Json<human::DecideResponse>>, PrivateResponse<Problem>> {
        self.decide(authenticated, Json(request)).await
    }
}
