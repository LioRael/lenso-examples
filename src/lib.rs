//! Actor-bound secure greeting Capability.

use std::{fmt, marker::PhantomData, rc::Rc};

use futures::future::LocalBoxFuture;
use lenso_auth_sdk::{ActorAssertionIssuer, AssertionClock, TypedActor};
use lenso_kernel::{InvocationContext, NativeRequestEndpoint, RuntimeFailure};

#[allow(dead_code)]
mod generated {
    include!("generated.rs");
}

pub use generated::{
    CAPABILITY_ID, DESCRIPTOR_VERSION, GREET_OPERATION, GreetError, GreetRequest, GreetResponse,
    SecureGreeting, SecureGreetingClient, SecureGreetingInvocationError, UnknownDomainError,
    decode_greet_error, decode_greet_request, decode_greet_response, encode_greet_error,
    encode_greet_request, encode_greet_response,
};

/// Target business logic that can only be called with a verified typed Actor.
pub trait SecureGreetingHandler<A>: fmt::Debug + 'static {
    /// Handles one request after actor binding succeeds.
    fn greet(
        &self,
        actor: A,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetError>>;
}

struct ActorBoundProvider<H, A> {
    handler: Rc<H>,
    verifier: ActorAssertionIssuer,
    clock: Rc<dyn AssertionClock>,
    actor: PhantomData<fn() -> A>,
}

impl<H: fmt::Debug, A> fmt::Debug for ActorBoundProvider<H, A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActorBoundProvider")
            .field("handler", &self.handler)
            .field("verifier", &self.verifier)
            .field("clock", &self.clock)
            .finish_non_exhaustive()
    }
}

impl<H, A> generated::SecureGreetingProvider for ActorBoundProvider<H, A>
where
    H: SecureGreetingHandler<A>,
    A: TypedActor + 'static,
{
    fn greet(
        &self,
        context: InvocationContext,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, generated::SecureGreetingInvocationError>>
    {
        let actor = self.verifier.project_context::<A>(
            &context,
            CAPABILITY_ID,
            GREET_OPERATION,
            self.clock.as_ref(),
        );
        let handler = Rc::clone(&self.handler);
        Box::pin(async move {
            let actor = actor.map_err(|_| {
                generated::SecureGreetingInvocationError::Domain(GreetError::ActorRequired)
            })?;
            handler
                .greet(actor, request)
                .await
                .map_err(generated::SecureGreetingInvocationError::Domain)
        })
    }
}

/// The only native endpoint constructor for this secured Capability.
pub struct ActorBoundSecureGreetingEndpoint<H, A>
where
    H: SecureGreetingHandler<A>,
    A: TypedActor + 'static,
{
    inner: generated::SecureGreetingEndpoint<ActorBoundProvider<H, A>>,
}

impl<H, A> fmt::Debug for ActorBoundSecureGreetingEndpoint<H, A>
where
    H: SecureGreetingHandler<A>,
    A: TypedActor + 'static,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActorBoundSecureGreetingEndpoint")
            .finish_non_exhaustive()
    }
}

impl<H, A> ActorBoundSecureGreetingEndpoint<H, A>
where
    H: SecureGreetingHandler<A>,
    A: TypedActor + 'static,
{
    /// Binds one target handler to its configured issuer and wall clock.
    pub fn new(
        handler: H,
        verifier: ActorAssertionIssuer,
        clock: impl AssertionClock + 'static,
    ) -> Self {
        Self {
            inner: generated::SecureGreetingEndpoint::new(ActorBoundProvider {
                handler: Rc::new(handler),
                verifier,
                clock: Rc::new(clock),
                actor: PhantomData,
            }),
        }
    }
}

impl<H, A> NativeRequestEndpoint for ActorBoundSecureGreetingEndpoint<H, A>
where
    H: SecureGreetingHandler<A>,
    A: TypedActor + 'static,
{
    fn capability_id(&self) -> &'static str {
        self.inner.capability_id()
    }

    fn descriptor_version(&self) -> &'static str {
        self.inner.descriptor_version()
    }

    fn operations(&self) -> &'static [&'static str] {
        self.inner.operations()
    }

    fn invoke(
        &self,
        operation: &str,
        request: Box<dyn std::any::Any>,
        context: InvocationContext,
    ) -> LocalBoxFuture<
        'static,
        Result<Result<Box<dyn std::any::Any>, Box<dyn std::any::Any>>, RuntimeFailure>,
    > {
        self.inner.invoke(operation, request, context)
    }
}
