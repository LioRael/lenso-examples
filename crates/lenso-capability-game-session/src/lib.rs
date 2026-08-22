//! Authenticated game-session Capability and its target-owned actor boundary.

use std::{fmt, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionVerifier, AssertionClock, TypedActor};
use lenso_kernel::{InvocationContext, NativeStreamEndpoint, NativeStreamSession, RuntimeFailure};

#[allow(dead_code)]
mod generated {
    include!("generated.rs");
}

pub use generated::{
    CAPABILITY_ID, DESCRIPTOR_VERSION, GameSession, GameSessionClient, GameSessionEndpoint,
    GameSessionInvocationError, GameSessionProvider, PLAY_OPERATION, PlayError, PlayRequest,
    PlayResponse, UnknownDomainError, decode_play_error, decode_play_request, decode_play_response,
    encode_play_error, encode_play_request, encode_play_response,
};

/// The typed actor owned by the game-session Module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerActor {
    subject: String,
}

impl PlayerActor {
    /// Returns the stable authenticated player subject.
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

impl TypedActor for PlayerActor {
    fn from_assertion(
        assertion: &lenso_auth_sdk::ActorAssertion,
    ) -> Result<Self, lenso_auth_sdk::ActorProjectionError> {
        if assertion.actor_kind() != "player" {
            return Err(lenso_auth_sdk::ActorProjectionError::UnexpectedActorKind {
                expected: "player".to_owned(),
                actual: assertion.actor_kind().to_owned(),
            });
        }
        Ok(Self {
            subject: assertion.subject().to_owned(),
        })
    }
}

/// Game behavior owned by a selected game-session Module.
pub trait GameSessionHandler: fmt::Debug + 'static {
    /// Opens one session after the target has projected and authorized a `PlayerActor`.
    fn play(
        &self,
        actor: PlayerActor,
        context: InvocationContext,
        request: PlayRequest,
    ) -> LocalBoxFuture<'static, Result<Box<dyn NativeStreamSession>, GameSessionInvocationError>>;
}

struct ActorBoundProvider<H> {
    handler: Rc<H>,
    verifier: ActorAssertionVerifier,
    clock: Rc<dyn AssertionClock>,
}

impl<H: fmt::Debug> fmt::Debug for ActorBoundProvider<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActorBoundProvider")
            .field("handler", &self.handler)
            .field("verifier", &self.verifier)
            .field("clock", &self.clock)
            .finish_non_exhaustive()
    }
}

impl<H: GameSessionHandler> GameSessionProvider for ActorBoundProvider<H> {
    fn play(
        &self,
        context: InvocationContext,
        request: PlayRequest,
    ) -> LocalBoxFuture<'static, Result<Box<dyn NativeStreamSession>, GameSessionInvocationError>>
    {
        let actor = self.verifier.project_context::<PlayerActor>(
            &context,
            CAPABILITY_ID,
            PLAY_OPERATION,
            self.clock.as_ref(),
        );
        let handler = Rc::clone(&self.handler);
        Box::pin(async move {
            let actor =
                actor.map_err(|_| GameSessionInvocationError::Domain(PlayError::ActorRequired))?;
            handler.play(actor, context, request).await
        })
    }
}

/// Endpoint wrapper that keeps assertion validation and typed actor projection at the target.
pub struct ActorBoundGameSessionEndpoint<H: GameSessionHandler> {
    inner: GameSessionEndpoint<ActorBoundProvider<H>>,
}

impl<H: GameSessionHandler> fmt::Debug for ActorBoundGameSessionEndpoint<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActorBoundGameSessionEndpoint")
            .finish_non_exhaustive()
    }
}

impl<H: GameSessionHandler> ActorBoundGameSessionEndpoint<H> {
    /// Binds game behavior to verification authority selected by App Composition.
    pub fn new(
        handler: H,
        verifier: ActorAssertionVerifier,
        clock: impl AssertionClock + 'static,
    ) -> Self {
        Self {
            inner: GameSessionEndpoint::new(ActorBoundProvider {
                handler: Rc::new(handler),
                verifier,
                clock: Rc::new(clock),
            }),
        }
    }
}

impl<H: GameSessionHandler> NativeStreamEndpoint for ActorBoundGameSessionEndpoint<H> {
    fn capability_id(&self) -> &'static str {
        self.inner.capability_id()
    }

    fn descriptor_version(&self) -> &'static str {
        self.inner.descriptor_version()
    }

    fn operations(&self) -> &'static [&'static str] {
        self.inner.operations()
    }

    fn open(
        &self,
        operation: &str,
        request: Box<dyn std::any::Any>,
        context: InvocationContext,
    ) -> LocalBoxFuture<
        'static,
        Result<Result<Box<dyn NativeStreamSession>, Box<dyn std::any::Any>>, RuntimeFailure>,
    > {
        self.inner.open(operation, request, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_auth_sdk::{ActorAssertionIssuer, FixedClock, Validity, audience};
    use lenso_kernel::{CancellationToken, InvocationContext};
    use std::collections::BTreeMap;
    use time::{Duration, OffsetDateTime};

    #[test]
    fn generated_game_session_values_round_trip() {
        let open = PlayRequest {
            room: "arena".to_owned(),
        };
        let wire = encode_play_request(&open).expect("open request should encode");
        assert_eq!(
            decode_play_request(&wire).expect("open request should decode"),
            open
        );
        let message = PlayResponse {
            action: "move:left".to_owned(),
        };
        let wire = encode_play_response(&message).expect("message should encode");
        assert_eq!(
            decode_play_response(&wire).expect("message should decode"),
            message
        );
    }

    #[test]
    fn player_actor_requires_the_game_actor_kind() {
        let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("valid time");
        let issuer = ActorAssertionIssuer::new("auth.players", b"game-key");
        let assertion = issuer.issue(
            "player-123",
            "player",
            "fixture",
            [audience(CAPABILITY_ID, PLAY_OPERATION)],
            Validity::new(now - Duration::seconds(1), now + Duration::minutes(1))
                .expect("validity should be ordered"),
            BTreeMap::new(),
        );
        let context = assertion
            .attach(InvocationContext::new(1, None, CancellationToken::new()))
            .expect("assertion should attach");
        let actor = issuer
            .verifier()
            .project_context::<PlayerActor>(
                &context,
                CAPABILITY_ID,
                PLAY_OPERATION,
                &FixedClock::new(now),
            )
            .expect("player assertion should project");
        assert_eq!(actor.subject(), "player-123");
    }
}
