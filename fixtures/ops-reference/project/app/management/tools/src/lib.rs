//! A real HTTP consumer of the optional ToolProvider; no model or coding tool starts.
use lenso_auth_sdk::{AuthOutcome, CredentialEvidence, authenticate_request, decode_auth_response};
use lenso_capability_agent_tool_provider as tools;
use lenso_capability_auth as auth;
use lenso_capability_http_endpoint::{
    ExtractorFuture, ExtractorRejection, FromRequest, prelude::*,
};

#[lenso::plugin]
#[derive(Clone, Debug)]
struct ToolHttp {
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "tools")]
    tools: tools::ToolProviderClient,
}
struct Authenticated(lenso::Ctx);
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
fn denied() -> PrivateResponse<Problem> {
    PrivateResponse(Problem::new(
        StatusCode::FORBIDDEN,
        "management_tool_denied",
        "The current management tool request was rejected",
    ))
}
fn unavailable() -> PrivateResponse<Problem> {
    PrivateResponse(Problem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "management_tool_unavailable",
        "The management tool result is unavailable; reconcile an accepted operation before retrying",
    ))
}
fn authentication_error(error: auth::AuthInvocationError) -> PrivateResponse<Problem> {
    match error {
        auth::AuthInvocationError::Runtime(_)
        | auth::AuthInvocationError::Domain(auth::AuthenticateError::Unknown(_)) => unavailable(),
        auth::AuthInvocationError::Domain(_) => denied(),
    }
}
fn catalog_error(error: tools::ToolProviderCatalogInvocationError) -> PrivateResponse<Problem> {
    match error {
        tools::ToolProviderCatalogInvocationError::Domain(tools::CatalogError::CatalogInvalid) => {
            denied()
        }
        _ => unavailable(),
    }
}
fn execute_error(error: tools::ToolProviderExecuteInvocationError) -> PrivateResponse<Problem> {
    match error {
        tools::ToolProviderExecuteInvocationError::Domain(
            tools::ExecuteError::PermissionDenied,
        ) => denied(),
        tools::ToolProviderExecuteInvocationError::Domain(
            tools::ExecuteError::InvalidArguments,
        ) => PrivateResponse(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_tool_arguments",
            "The tool arguments were rejected",
        )),
        tools::ToolProviderExecuteInvocationError::Domain(tools::ExecuteError::NotFound) => {
            PrivateResponse(Problem::new(
                StatusCode::NOT_FOUND,
                "tool_not_found",
                "The current tool is unavailable to this principal",
            ))
        }
        tools::ToolProviderExecuteInvocationError::Domain(
            tools::ExecuteError::ExecutionFailed { payload },
        ) if payload.reason_code == "conflict" => PrivateResponse(Problem::new(
            StatusCode::CONFLICT,
            "intent_changed",
            "Reload the current intent before continuing",
        )),
        _ => unavailable(),
    }
}
fn rejection(problem: PrivateResponse<Problem>) -> ExtractorRejection {
    ExtractorRejection::Response(problem.into_response().expect("static private problem"))
}
impl FromRequest<ToolHttp> for Authenticated {
    fn from_request<'a>(
        provider: &'a ToolHttp,
        context: &'a mut lenso::Ctx,
        request: &'a HandleRequest,
    ) -> ExtractorFuture<'a, Self> {
        Box::pin(async move {
            let reject = || rejection(denied());
            let credential = request
                .credential
                .as_ref()
                .filter(|value| value.scheme == "bearer" && value.value.len() <= 8192)
                .ok_or_else(reject)?;
            let response = provider
                .auth
                .authenticate_with_context(
                    context.clone(),
                    authenticate_request(Some(CredentialEvidence::new(
                        "bearer",
                        credential.value.clone(),
                    ))),
                )
                .await
                .map_err(|error| rejection(authentication_error(error)))?;
            let AuthOutcome::Authenticated(assertion) =
                decode_auth_response(response).map_err(|_| rejection(unavailable()))?
            else {
                return Err(reject());
            };
            *context = assertion
                .attach(context.clone())
                .map_err(|_| rejection(unavailable()))?;
            Ok(Self(context.clone()))
        })
    }
}
#[endpoint]
impl ToolHttp {
    #[get("ops.management.tools", "/management-tools/catalog")]
    async fn catalog(
        &self,
        Authenticated(context): Authenticated,
    ) -> Result<PrivateResponse<Json<tools::CatalogResponse>>, PrivateResponse<Problem>> {
        self.tools
            .catalog_with_context(context, tools::CatalogRequest {})
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(catalog_error)
    }
    #[post("ops.management.tool-execute", "/management-tools/execute")]
    async fn execute(
        &self,
        Authenticated(context): Authenticated,
        Json(request): Json<tools::ExecuteRequest>,
    ) -> Result<PrivateResponse<Json<tools::ExecuteResponse>>, PrivateResponse<Problem>> {
        self.tools
            .execute_with_context(context, request)
            .await
            .map(|response| PrivateResponse(Json(response)))
            .map_err(execute_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_status(problem: PrivateResponse<Problem>) -> i64 {
        let response = problem.into_response().unwrap();
        assert!(
            response
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("cache-control")
                    && header.value == "no-store")
        );
        response.status
    }

    #[test]
    fn unknown_owner_state_and_runtime_are_unavailable_instead_of_denied() {
        let future = auth::AuthenticateError::Unknown(auth::UnknownDomainError {
            code: "future_auth_state".into(),
            payload: None,
            extra: Default::default(),
        });
        assert_eq!(
            private_status(authentication_error(auth::AuthInvocationError::Domain(
                future
            ))),
            503
        );
        assert_eq!(
            private_status(authentication_error(auth::AuthInvocationError::Runtime(
                lenso::RuntimeFailure::Unavailable {
                    capability: auth::CAPABILITY_ID
                }
            ))),
            503
        );
        assert_eq!(
            private_status(authentication_error(auth::AuthInvocationError::Domain(
                auth::AuthenticateError::Revoked
            ))),
            403
        );
        let future = tools::CatalogError::Unknown(tools::UnknownDomainError {
            code: "future_catalog_state".into(),
            payload: None,
            extra: Default::default(),
        });
        assert_eq!(
            private_status(catalog_error(
                tools::ToolProviderCatalogInvocationError::Domain(future)
            )),
            503
        );
        assert_eq!(
            private_status(catalog_error(
                tools::ToolProviderCatalogInvocationError::Runtime(
                    lenso::RuntimeFailure::ProtocolViolation {
                        capability: tools::CAPABILITY_ID
                    }
                )
            )),
            503
        );
    }

    #[test]
    fn known_tool_denial_and_request_errors_keep_distinct_http_statuses() {
        for (error, expected) in [
            (tools::ExecuteError::PermissionDenied, 403),
            (tools::ExecuteError::InvalidArguments, 400),
            (tools::ExecuteError::NotFound, 404),
            (tools::ExecuteError::OutputLimitExceeded, 503),
        ] {
            assert_eq!(
                private_status(execute_error(
                    tools::ToolProviderExecuteInvocationError::Domain(error)
                )),
                expected
            );
        }
        let future = tools::ExecuteError::Unknown(tools::UnknownDomainError {
            code: "future_execution_state".into(),
            payload: None,
            extra: Default::default(),
        });
        assert_eq!(
            private_status(execute_error(
                tools::ToolProviderExecuteInvocationError::Domain(future)
            )),
            503
        );
    }
}
