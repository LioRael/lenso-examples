use std::rc::Rc;

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionVerifier, AssertionClock};
use lenso_capability_game_session::{
    ActorBoundGameSessionEndpoint, GameSessionHandler, GameSessionInvocationError, PlayError,
    PlayRequest, PlayerActor,
};
use lenso_kernel::{InvocationContext, NativeStreamSession, NoopModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use time::OffsetDateTime;

use crate::{SessionMode, session::GameSession};

/// Native package identity for the replaceable game provider.
pub const GAME_PACKAGE_ID: &str = "fixture.game.provider";

/// Alternate package identity selected by Composition for the replacement provider.
pub const GAME_REPLACEMENT_PACKAGE_ID: &str = "fixture.game.provider.replacement";

/// Creates the selected game provider Module.
#[derive(Clone, Debug)]
pub struct GameProviderFactory {
    verifier: ActorAssertionVerifier,
    mode: SessionMode,
}

impl GameProviderFactory {
    /// Creates a provider whose behavior can be swapped through Composition.
    pub fn new(verifier: ActorAssertionVerifier, mode: SessionMode) -> Self {
        Self { verifier, mode }
    }

    /// Returns the package identity selected for one provider implementation.
    pub const fn package_id_for(mode: SessionMode) -> &'static str {
        match mode {
            SessionMode::Echo => GAME_PACKAGE_ID,
            SessionMode::Replacement => GAME_REPLACEMENT_PACKAGE_ID,
        }
    }
}

impl NativeModuleFactory for GameProviderFactory {
    fn package_id(&self) -> &'static str {
        Self::package_id_for(self.mode)
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        let endpoint = ActorBoundGameSessionEndpoint::new(
            GameHandler { mode: self.mode },
            self.verifier.clone(),
            WallClock,
        );
        Ok(NativeModuleInstance::with_stream_endpoints(
            vec![Rc::new(endpoint)],
            NoopModuleLifecycle,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
struct GameHandler {
    mode: SessionMode,
}

impl GameSessionHandler for GameHandler {
    fn play(
        &self,
        actor: PlayerActor,
        _context: InvocationContext,
        request: PlayRequest,
    ) -> LocalBoxFuture<'static, Result<Box<dyn NativeStreamSession>, GameSessionInvocationError>>
    {
        let mode = self.mode;
        Box::pin(async move {
            if actor.subject() != "player-123" || request.room == "forbidden" {
                return Err(GameSessionInvocationError::Domain(PlayError::NotAllowed));
            }
            if request.room == "closed" {
                return Err(GameSessionInvocationError::Domain(PlayError::RoomClosed));
            }
            Ok(
                Box::new(GameSession::new(&request.room, actor.subject(), mode, 16))
                    as Box<dyn NativeStreamSession>,
            )
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct WallClock;

impl AssertionClock for WallClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
