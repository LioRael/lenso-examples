use std::{collections::BTreeMap, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    ModuleInstancePlan,
};
use lenso_auth_sdk::{
    ActorAssertion, ActorAssertionIssuer, ActorAssertionVerifier, ActorProjectionError,
    AuthOutcome, CredentialEvidence, FixedClock, TypedActor, Validity, audience,
    authenticate_request, authenticated_response, decode_auth_response,
};
use lenso_capability_auth::{
    AUTHENTICATE_OPERATION, Auth, AuthEndpoint, AuthError, AuthInvocationError, AuthProvider,
    AuthRequest, AuthResponse, CAPABILITY_ID as AUTH_ID, DESCRIPTOR_VERSION as AUTH_VERSION,
};
use lenso_capability_secure_greeting::{
    ActorBoundSecureGreetingEndpoint, CAPABILITY_ID, DESCRIPTOR_VERSION, GREET_OPERATION,
    GreetError, GreetRequest, GreetResponse, SecureGreeting, SecureGreetingHandler,
};
use lenso_kernel::{CancellationToken, DeterministicDriver, Kernel, RuntimeFailure};
use lenso_native_adapter::{
    NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance, NativeModuleRegistry,
};
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, Eq, PartialEq)]
struct UserActor(String);

impl TypedActor for UserActor {
    fn from_assertion(assertion: &ActorAssertion) -> Result<Self, ActorProjectionError> {
        if assertion.actor_kind() != "user" {
            return Err(ActorProjectionError::UnexpectedActorKind {
                expected: "user".to_owned(),
                actual: assertion.actor_kind().to_owned(),
            });
        }
        Ok(Self(assertion.subject().to_owned()))
    }
}

#[derive(Debug)]
struct GreetingHandler;

impl SecureGreetingHandler<UserActor> for GreetingHandler {
    fn greet(
        &self,
        actor: UserActor,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetError>> {
        Box::pin(async move {
            if request.name.is_empty() {
                return Err(GreetError::EmptyName);
            }
            if actor.0 == "forbidden" {
                return Err(GreetError::NotAllowed);
            }
            Ok(GreetResponse {
                message: format!("Hello, {} ({})!", request.name, actor.0),
            })
        })
    }
}

#[derive(Clone, Debug)]
struct FixtureAuthProvider {
    issuer: ActorAssertionIssuer,
    now: OffsetDateTime,
}

impl AuthProvider for FixtureAuthProvider {
    fn authenticate(
        &self,
        _context: lenso_kernel::InvocationContext,
        request: AuthRequest,
    ) -> LocalBoxFuture<'static, Result<AuthResponse, AuthInvocationError>> {
        let issuer = self.issuer.clone();
        let now = self.now;
        Box::pin(async move {
            let credential = request
                .credential
                .ok_or(AuthInvocationError::Domain(AuthError::Invalid))?;
            if credential.scheme != "bearer" {
                return Err(AuthInvocationError::Domain(AuthError::Unsupported));
            }
            let subject = match credential.value.as_str() {
                "good-token" => "user-123",
                "forbidden-token" => "forbidden",
                _ => return Err(AuthInvocationError::Domain(AuthError::Invalid)),
            };
            let assertion = issuer.issue(
                subject,
                "user",
                "fixture",
                [audience(CAPABILITY_ID, GREET_OPERATION)],
                Validity::new(now - Duration::seconds(1), now + Duration::minutes(1))
                    .expect("fixture interval is valid"),
                BTreeMap::new(),
            );
            Ok(authenticated_response(&assertion))
        })
    }
}

#[derive(Debug)]
struct FixtureFactory {
    package: &'static str,
    auth: FixtureAuthProvider,
    verifier: ActorAssertionVerifier,
    clock: FixedClock,
}

impl NativeModuleFactory for FixtureFactory {
    fn package_id(&self) -> &'static str {
        self.package
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let endpoints: Vec<Rc<dyn lenso_kernel::NativeRequestEndpoint>> = match self.package {
            "fixture.auth" => vec![Rc::new(AuthEndpoint::new(self.auth.clone()))],
            "fixture.secure-greeting" => vec![Rc::new(ActorBoundSecureGreetingEndpoint::new(
                GreetingHandler,
                self.verifier.clone(),
                self.clock,
            ))],
            "fixture.ingress" => Vec::new(),
            _ => unreachable!("factory package is fixed"),
        };
        Ok(NativeModuleInstance::new(endpoints))
    }
}

#[derive(Debug)]
struct FixtureIngressAdapter;

impl FixtureIngressAdapter {
    fn select_bearer<'a>(
        &self,
        headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Option<CredentialEvidence>, &'static str> {
        let mut selected = None;
        for (name, value) in headers {
            if !name.eq_ignore_ascii_case("authorization") {
                continue;
            }
            let Some(value) = value.strip_prefix("Bearer ") else {
                continue;
            };
            if selected.is_some() {
                return Err("multiple bearer credentials");
            }
            selected = Some(CredentialEvidence::new("bearer", value));
        }
        Ok(selected)
    }
}

fn plan() -> lenso_app_plan::ResolvedAppPlan {
    let ingress = ModuleInstancePlan::new("ingress", "fixture.ingress")
        .with_requirement(CapabilityRequirementPlan::one(AUTH_ID, AUTH_VERSION))
        .with_requirement(CapabilityRequirementPlan::one(
            CAPABILITY_ID,
            DESCRIPTOR_VERSION,
        ));
    let auth = ModuleInstancePlan::new("auth", "fixture.auth").with_capability(
        CapabilityEndpointPlan::new(AUTH_ID, AUTH_VERSION, [AUTHENTICATE_OPERATION]),
    );
    let target = ModuleInstancePlan::new("target", "fixture.secure-greeting").with_capability(
        CapabilityEndpointPlan::new(CAPABILITY_ID, DESCRIPTOR_VERSION, [GREET_OPERATION]),
    );
    AppComposition::new(
        vec![ingress, auth, target],
        vec![
            CapabilityBinding::new("ingress", AUTH_ID, AUTH_VERSION, "auth"),
            CapabilityBinding::new("ingress", CAPABILITY_ID, DESCRIPTOR_VERSION, "target"),
        ],
    )
    .resolve()
    .expect("fixture App Composition resolves")
}

#[test]
fn ingress_invokes_bound_auth_before_the_actor_bound_target() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("valid timestamp");
    let issuer = ActorAssertionIssuer::new("auth.users", b"shared-auth-key");
    let auth = FixtureAuthProvider {
        issuer: issuer.clone(),
        now,
    };
    let registry = ["fixture.ingress", "fixture.auth", "fixture.secure-greeting"]
        .into_iter()
        .fold(NativeModuleRegistry::new(), |registry, package| {
            registry.with_factory(FixtureFactory {
                package,
                auth: auth.clone(),
                verifier: issuer.verifier(),
                clock: FixedClock::new(now),
            })
        });
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(plan(), driver.clone(), registry))
        .expect("native fixture starts");
    let evidence = FixtureIngressAdapter
        .select_bearer([("Authorization", "Bearer good-token")])
        .expect("one credential is selected");
    let auth_response = driver
        .run(app.invoke::<Auth>(
            "ingress",
            AUTHENTICATE_OPERATION,
            authenticate_request(evidence),
        ))
        .expect("bound Auth invocation succeeds")
        .expect("credential is accepted");
    let AuthOutcome::Authenticated(assertion) =
        decode_auth_response(auth_response).expect("Auth response is consistent")
    else {
        panic!("credential should authenticate");
    };
    let context = assertion
        .attach(app.invocation_context(None, CancellationToken::new()))
        .expect("assertion attaches once");
    let response = driver
        .run(app.invoke_with_context::<SecureGreeting>(
            "ingress",
            GREET_OPERATION,
            context,
            GreetRequest {
                name: "Ada".to_owned(),
            },
        ))
        .expect("target invocation succeeds")
        .expect("verified actor is allowed");

    assert_eq!(response.message, "Hello, Ada (user-123)!");
    let unrelated = issuer.issue(
        "user-123",
        "user",
        "fixture",
        [audience("example.other@1", "read")],
        Validity::new(now - Duration::seconds(1), now + Duration::minutes(1))
            .expect("unrelated fixture interval is valid"),
        BTreeMap::new(),
    );
    let unrelated_context = unrelated
        .attach(app.invocation_context(None, CancellationToken::new()))
        .expect("unrelated assertion attaches");
    let unrelated_result = driver
        .run(app.invoke_with_context::<SecureGreeting>(
            "ingress",
            GREET_OPERATION,
            unrelated_context,
            GreetRequest {
                name: "Ada".to_owned(),
            },
        ))
        .expect("unrelated invocation reaches provider");
    assert_eq!(unrelated_result, Err(GreetError::ActorRequired));
    let anonymous = driver
        .run(app.invoke::<SecureGreeting>(
            "ingress",
            GREET_OPERATION,
            GreetRequest {
                name: "Ada".to_owned(),
            },
        ))
        .expect("target invocation reaches provider");
    assert_eq!(anonymous, Err(GreetError::ActorRequired));
}
