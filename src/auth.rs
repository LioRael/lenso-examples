use std::{collections::BTreeMap, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionIssuer, Validity, audience, authenticated_response};
use lenso_capability_auth::{
    AuthEndpoint, AuthError, AuthInvocationError, AuthProvider, AuthRequest, AuthResponse,
};
use lenso_capability_game_session::{CAPABILITY_ID as GAME_CAPABILITY_ID, PLAY_OPERATION};
use lenso_kernel::RuntimeFailure;
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use time::{Duration as TimeDuration, OffsetDateTime};

/// Native package identity for the example Auth Module.
pub const AUTH_PACKAGE_ID: &str = "fixture.game.auth";

/// Credential scheme selected by the game protocol Module.
pub const GAME_CREDENTIAL_SCHEME: &str = "game-bearer";

/// A small deterministic credential set used by the network fixture.
#[derive(Clone, Debug)]
pub struct AuthModuleFactory {
    issuer: ActorAssertionIssuer,
}

impl AuthModuleFactory {
    /// Creates an Auth Module using App-selected issuer material.
    pub fn new(issuer: ActorAssertionIssuer) -> Self {
        Self { issuer }
    }
}

impl NativeModuleFactory for AuthModuleFactory {
    fn package_id(&self) -> &'static str {
        AUTH_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(vec![Rc::new(AuthEndpoint::new(
            GameAuthProvider {
                issuer: self.issuer.clone(),
            },
        ))]))
    }
}

#[derive(Clone, Debug)]
struct GameAuthProvider {
    issuer: ActorAssertionIssuer,
}

impl AuthProvider for GameAuthProvider {
    fn authenticate(
        &self,
        _context: lenso_kernel::InvocationContext,
        request: AuthRequest,
    ) -> LocalBoxFuture<'static, Result<AuthResponse, AuthInvocationError>> {
        let issuer = self.issuer.clone();
        Box::pin(async move {
            let Some(credential) = request.credential else {
                return Ok(lenso_auth_sdk::absent_response());
            };
            if credential.scheme != GAME_CREDENTIAL_SCHEME {
                return Err(AuthInvocationError::Domain(AuthError::Unsupported));
            }
            let (subject, validity) = match credential.value.as_str() {
                "good-token" => ("player-123", valid_assertion()),
                "forbidden-token" => ("player-999", valid_assertion()),
                "expired-token" => {
                    return Err(AuthInvocationError::Domain(AuthError::Expired));
                }
                "provider-failure" => {
                    return Err(AuthInvocationError::Runtime(
                        RuntimeFailure::ModuleFailure {
                            detail: "fixture Auth provider failed".to_owned(),
                        },
                    ));
                }
                _ => return Err(AuthInvocationError::Domain(AuthError::Invalid)),
            };
            let assertion = issuer.issue(
                subject,
                "player",
                "fixture",
                [audience(GAME_CAPABILITY_ID, PLAY_OPERATION)],
                validity,
                BTreeMap::default(),
            );
            Ok(authenticated_response(&assertion))
        })
    }
}

fn valid_assertion() -> Validity {
    let now = OffsetDateTime::now_utc();
    Validity::new(
        now - TimeDuration::seconds(5),
        now + TimeDuration::minutes(5),
    )
    .expect("fixture assertion interval is ordered")
}
