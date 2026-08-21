//! Native game-session tracer bullet: protocol, Auth, and game Modules.

mod auth;
mod connection;
mod frame;
mod game;
mod protocol;
mod session;

use std::time::Duration;

use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    ModuleInstancePlan, PlanResolutionError, ResolvedAppPlan, RestartPolicy,
};
use lenso_auth_sdk::ActorAssertionIssuer;
use lenso_capability_auth::{
    AUTHENTICATE_OPERATION, CAPABILITY_ID as AUTH_CAPABILITY_ID,
    DESCRIPTOR_VERSION as AUTH_DESCRIPTOR_VERSION,
};
use lenso_capability_game_session::{
    CAPABILITY_ID as GAME_CAPABILITY_ID, DESCRIPTOR_VERSION as GAME_DESCRIPTOR_VERSION,
    PLAY_OPERATION,
};
use lenso_native_adapter::NativeModuleRegistry;

pub use auth::{AUTH_PACKAGE_ID, AuthModuleFactory};
pub use frame::{ClientFrame, ServerFrame, TerminalFrame};
pub use game::{GAME_PACKAGE_ID, GAME_REPLACEMENT_PACKAGE_ID, GameProviderFactory};
pub use protocol::{
    GameProtocolFactory, PROTOCOL_PACKAGE_ID, PROTOCOL_REPLACEMENT_PACKAGE_ID, ProtocolConfig,
    ProtocolVariant,
};
pub use session::SessionMode;

/// Builds the explicit Composition used by the game-session example.
pub fn composition(config: &ProtocolConfig) -> AppComposition {
    composition_with_variants(config, ProtocolVariant::Primary, SessionMode::Echo)
}

/// Builds the Composition with an explicitly selected game provider package.
pub fn composition_with_mode(config: &ProtocolConfig, mode: SessionMode) -> AppComposition {
    composition_with_variants(config, ProtocolVariant::Primary, mode)
}

/// Builds the Composition with explicitly selected protocol and game packages.
pub fn composition_with_variants(
    config: &ProtocolConfig,
    protocol_variant: ProtocolVariant,
    mode: SessionMode,
) -> AppComposition {
    let protocol = ModuleInstancePlan::new("protocol", protocol_variant.package_id())
        .with_configuration(config.to_json())
        .with_requirement(CapabilityRequirementPlan::one(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            GAME_CAPABILITY_ID,
            GAME_DESCRIPTOR_VERSION,
        ));
    let auth = ModuleInstancePlan::new("auth", AUTH_PACKAGE_ID).with_capability(
        CapabilityEndpointPlan::new(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
            [AUTHENTICATE_OPERATION],
        )
        .with_limits(4, 4),
    );
    let game = ModuleInstancePlan::new("game", GameProviderFactory::package_id_for(mode))
        .with_restart_policy(RestartPolicy::on_failure(
            2,
            Duration::from_secs(30),
            Duration::ZERO,
            Duration::ZERO,
            Duration::from_secs(1),
        ))
        .with_capability(
            CapabilityEndpointPlan::new(
                GAME_CAPABILITY_ID,
                GAME_DESCRIPTOR_VERSION,
                [PLAY_OPERATION],
            )
            .with_stream_operation(PLAY_OPERATION)
            .with_limits(config.max_connections(), config.max_connections()),
        );
    AppComposition::new(
        vec![protocol, auth, game],
        vec![
            CapabilityBinding::new(
                "protocol",
                AUTH_CAPABILITY_ID,
                AUTH_DESCRIPTOR_VERSION,
                "auth",
            )
            .with_limits(4, 4),
            CapabilityBinding::new(
                "protocol",
                GAME_CAPABILITY_ID,
                GAME_DESCRIPTOR_VERSION,
                "game",
            )
            .with_limits(config.max_connections(), config.max_connections()),
        ],
    )
}

/// Resolves the example's immutable App Plan.
pub fn resolved_plan(config: &ProtocolConfig) -> Result<ResolvedAppPlan, PlanResolutionError> {
    composition(config).resolve()
}

/// Resolves the Composition with an explicitly selected game provider package.
pub fn resolved_plan_with_mode(
    config: &ProtocolConfig,
    mode: SessionMode,
) -> Result<ResolvedAppPlan, PlanResolutionError> {
    composition_with_mode(config, mode).resolve()
}

/// Resolves the Composition with explicitly selected protocol and game packages.
pub fn resolved_plan_with_variants(
    config: &ProtocolConfig,
    protocol_variant: ProtocolVariant,
    mode: SessionMode,
) -> Result<ResolvedAppPlan, PlanResolutionError> {
    composition_with_variants(config, protocol_variant, mode).resolve()
}

/// Assembles the selected native Modules without adding protocol behavior to Kernel.
pub fn native_registry(
    issuer: ActorAssertionIssuer,
    session_mode: SessionMode,
) -> NativeModuleRegistry {
    native_registry_with_variants(issuer, ProtocolVariant::Primary, session_mode)
}

/// Assembles the exact protocol and game packages selected by Composition.
pub fn native_registry_with_variants(
    issuer: ActorAssertionIssuer,
    protocol_variant: ProtocolVariant,
    session_mode: SessionMode,
) -> NativeModuleRegistry {
    let verifier = issuer.verifier();
    NativeModuleRegistry::new()
        .with_factory(AuthModuleFactory::new(issuer))
        .with_factory(GameProviderFactory::new(verifier, session_mode))
        .with_factory(GameProtocolFactory::with_variant(protocol_variant))
}
