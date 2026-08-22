use std::collections::BTreeMap;

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionIssuer, Validity, audience, authenticated_response};
use lenso_capability_auth::{
    AuthEndpoint, AuthError, AuthInvocationError, AuthProvider, AuthRequest, AuthResponse,
};
use lenso_capability_secure_greeting::{CAPABILITY_ID, GREET_OPERATION};
use lenso_kernel::{InvocationContext, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use time::{Duration, OffsetDateTime};

const AUTH_PACKAGE_ID: &str = "fixture.web-auth";
const FIXTURE_KEY: &[u8] = b"fixture-web-ui-key";

pub fn fixture_issuer() -> ActorAssertionIssuer {
    ActorAssertionIssuer::new("fixture.web-auth", FIXTURE_KEY)
}

fn fixture_now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("fixture timestamp is valid")
}

#[derive(Clone, Debug)]
struct WebAuthProvider {
    issuer: ActorAssertionIssuer,
}

impl AuthProvider for WebAuthProvider {
    fn authenticate(
        &self,
        _context: InvocationContext,
        request: AuthRequest,
    ) -> LocalBoxFuture<'static, Result<AuthResponse, AuthInvocationError>> {
        let issuer = self.issuer.clone();
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
            let now = fixture_now();
            let assertion = issuer.issue(
                subject,
                "user",
                "fixture",
                [audience(CAPABILITY_ID, GREET_OPERATION)],
                Validity::new(now - Duration::seconds(1), now + Duration::minutes(1))
                    .expect("fixture validity is ordered"),
                BTreeMap::new(),
            );
            Ok(authenticated_response(&assertion))
        })
    }
}

#[derive(Clone, Debug)]
pub struct AuthModuleFactory {
    issuer: ActorAssertionIssuer,
}

impl AuthModuleFactory {
    pub const fn new(issuer: ActorAssertionIssuer) -> Self {
        Self { issuer }
    }
}

impl NativeModuleFactory for AuthModuleFactory {
    fn package_id(&self) -> &'static str {
        AUTH_PACKAGE_ID
    }

    fn package_version(&self) -> &'static str {
        "0.1.0"
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(vec![std::rc::Rc::new(
            AuthEndpoint::new(WebAuthProvider {
                issuer: self.issuer.clone(),
            }),
        )]))
    }
}

pub fn fixed_clock() -> lenso_auth_sdk::FixedClock {
    lenso_auth_sdk::FixedClock::new(fixture_now())
}
