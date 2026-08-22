//! Statically linked native Rust Greeting Module fixture.

use futures::future::LocalBoxFuture;
use lenso_capability_greeting::{
    GreetError, GreetRequest, GreetResponse, GreetingClient, GreetingEndpoint,
    GreetingInvocationError, GreetingProvider,
};
use lenso_kernel::{ActivateContext, InvocationContext, ModuleLifecycle, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleFactoryContext, NativeModuleInstance};
use std::rc::Rc;

pub const GREETER_PACKAGE_ID: &str = "example.native-greeter";
pub const ALTERNATE_GREETER_PACKAGE_ID: &str = "example.native-alternate-greeter";
pub const CONSUMER_PACKAGE_ID: &str = "example.native-consumer";

#[derive(Debug)]
pub struct ConsumerFactory;
impl NativeModuleFactory for ConsumerFactory {
    fn package_id(&self) -> &'static str {
        CONSUMER_PACKAGE_ID
    }
    fn package_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::with_lifecycle(
            Vec::new(),
            ConsumerLifecycle,
        ))
    }
}

#[derive(Debug)]
struct ConsumerLifecycle;

impl ModuleLifecycle for ConsumerLifecycle {
    fn activate(&self, context: ActivateContext) -> lenso_kernel::ModuleFuture {
        let client = (context.dependencies().len() == 1)
            .then(|| GreetingClient::from_dependencies(context.dependencies()));
        Box::pin(async move {
            let Some(client) = client else {
                return Ok(());
            };
            let client = client?;
            match client
                .greet(GreetRequest {
                    name: "activation".to_owned(),
                })
                .await
            {
                Ok(_) => Ok(()),
                Err(GreetingInvocationError::Runtime(error)) => Err(error),
                Err(GreetingInvocationError::Domain(error)) => Err(RuntimeFailure::ModuleFailure {
                    detail: format!("Greeting activation dependency returned {error:?}"),
                }),
            }
        })
    }
}

#[derive(Debug)]
pub struct GreeterFactory;
impl NativeModuleFactory for GreeterFactory {
    fn package_id(&self) -> &'static str {
        GREETER_PACKAGE_ID
    }
    fn package_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(vec![Rc::new(
            GreetingEndpoint::new(Greeter),
        )]))
    }
}

/// A second statically linked implementation of the same generated Capability.
#[derive(Debug)]
pub struct AlternateGreeterFactory;

impl NativeModuleFactory for AlternateGreeterFactory {
    fn package_id(&self) -> &'static str {
        ALTERNATE_GREETER_PACKAGE_ID
    }
    fn package_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn instantiate(
        &self,
        _context: NativeModuleFactoryContext<'_>,
    ) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::new(vec![Rc::new(
            GreetingEndpoint::new(AlternateGreeter),
        )]))
    }
}

#[derive(Debug)]
struct Greeter;
impl GreetingProvider for Greeter {
    fn greet(
        &self,
        _context: InvocationContext,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetingInvocationError>> {
        Box::pin(async move {
            if request.name.is_empty() {
                Err(GreetingInvocationError::Domain(GreetError::EmptyName))
            } else {
                Ok(GreetResponse {
                    message: format!("Hello, {}!", request.name),
                })
            }
        })
    }
}

#[derive(Debug)]
struct AlternateGreeter;

impl GreetingProvider for AlternateGreeter {
    fn greet(
        &self,
        _context: InvocationContext,
        request: GreetRequest,
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetingInvocationError>> {
        Box::pin(async move {
            if request.name.is_empty() {
                Err(GreetingInvocationError::Domain(GreetError::EmptyName))
            } else {
                Ok(GreetResponse {
                    message: format!("Ahoy, {}!", request.name),
                })
            }
        })
    }
}
