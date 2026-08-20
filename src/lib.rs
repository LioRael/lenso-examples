//! Statically linked native Rust Greeting Module fixture.

use futures::future::LocalBoxFuture;
use lenso_capability_greeting::{
    GreetError, GreetRequest, GreetResponse, GreetingEndpoint, GreetingProvider,
};
use lenso_kernel::{InvocationContext, RuntimeFailure};
use lenso_native_adapter::{NativeModuleFactory, NativeModuleInstance};
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

    fn instantiate(&self, _instance_key: &str) -> Result<NativeModuleInstance, RuntimeFailure> {
        Ok(NativeModuleInstance::default())
    }
}

#[derive(Debug)]
pub struct GreeterFactory;
impl NativeModuleFactory for GreeterFactory {
    fn package_id(&self) -> &'static str {
        GREETER_PACKAGE_ID
    }
    fn instantiate(&self, _instance_key: &str) -> Result<NativeModuleInstance, RuntimeFailure> {
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

    fn instantiate(&self, _instance_key: &str) -> Result<NativeModuleInstance, RuntimeFailure> {
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
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetError>> {
        Box::pin(async move {
            if request.name.is_empty() {
                Err(GreetError::EmptyName)
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
    ) -> LocalBoxFuture<'static, Result<GreetResponse, GreetError>> {
        Box::pin(async move {
            if request.name.is_empty() {
                Err(GreetError::EmptyName)
            } else {
                Ok(GreetResponse {
                    message: format!("Ahoy, {}!", request.name),
                })
            }
        })
    }
}
