use std::{fmt, rc::Rc};
use futures::future::LocalBoxFuture;
use lenso_kernel::{
    NativeApp, NativeRequestEndpoint, NativeRequestHandle, RequestCapability, RuntimeFailure,
};

pub const GREETING_CAPABILITY_ID: &str = "__CAPABILITY_ID__";
pub const GREETING_DESCRIPTOR_VERSION: &str = "__DESCRIPTOR_VERSION__";
pub const GREET_OPERATION: &str = "__OPERATION__";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct __OPERATION_TYPE__Request { pub __REQUEST_FIELD__: String }
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct __OPERATION_TYPE__Response { pub __RESPONSE_FIELD__: String }
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum __OPERATION_TYPE__Error { __DOMAIN_ERROR__ }

#[derive(Debug)]
pub struct __CAPABILITY__;
impl RequestCapability for __CAPABILITY__ {
    type Request = __OPERATION_TYPE__Request;
    type Response = __OPERATION_TYPE__Response;
    type DomainError = __OPERATION_TYPE__Error;
    const ID: &'static str = GREETING_CAPABILITY_ID;
    const DESCRIPTOR_VERSION: &'static str = GREETING_DESCRIPTOR_VERSION;
}

pub trait __CAPABILITY__Provider: fmt::Debug + 'static {
    fn __OPERATION_FN__(&self, request: __OPERATION_TYPE__Request) -> LocalBoxFuture<'static, Result<__OPERATION_TYPE__Response, __OPERATION_TYPE__Error>>;
}

#[derive(Debug)]
pub struct __CAPABILITY__Endpoint<P> { provider: Rc<P> }
impl<P: __CAPABILITY__Provider> __CAPABILITY__Endpoint<P> {
    pub fn new(provider: P) -> Self { Self { provider: Rc::new(provider) } }
}
impl<P: __CAPABILITY__Provider> NativeRequestEndpoint for __CAPABILITY__Endpoint<P> {
    fn capability_id(&self) -> &'static str { GREETING_CAPABILITY_ID }
    fn descriptor_version(&self) -> &'static str { GREETING_DESCRIPTOR_VERSION }
    fn operations(&self) -> &'static [&'static str] { &[GREET_OPERATION] }
    fn invoke(&self, operation: &str, request: Box<dyn std::any::Any>) -> LocalBoxFuture<'static, Result<Result<Box<dyn std::any::Any>, Box<dyn std::any::Any>>, RuntimeFailure>> {
        if operation != GREET_OPERATION {
            return Box::pin(futures::future::ready(Err(RuntimeFailure::UnknownOperation { capability: GREETING_CAPABILITY_ID, operation: operation.to_owned() })));
        }
        let Ok(request) = request.downcast::<__OPERATION_TYPE__Request>() else {
            return Box::pin(futures::future::ready(Err(RuntimeFailure::ProtocolViolation { capability: GREETING_CAPABILITY_ID })));
        };
        let provider = Rc::clone(&self.provider);
        Box::pin(async move {
            Ok(provider.__OPERATION_FN__(*request).await
                .map(|value| Box::new(value) as Box<dyn std::any::Any>)
                .map_err(|error| Box::new(error) as Box<dyn std::any::Any>))
        })
    }
}

#[derive(Debug)]
pub struct __CAPABILITY__Client { handle: NativeRequestHandle<__CAPABILITY__> }
impl __CAPABILITY__Client {
    pub fn new(app: &NativeApp, caller: &str) -> Result<Self, RuntimeFailure> {
        Ok(Self { handle: app.handle::<__CAPABILITY__>(caller)? })
    }
    pub async fn __OPERATION_FN__(&self, request: __OPERATION_TYPE__Request) -> Result<__OPERATION_TYPE__Response, __CAPABILITY__InvocationError> {
        self.handle.invoke(GREET_OPERATION, request).await
            .map_err(__CAPABILITY__InvocationError::Runtime)?
            .map_err(__CAPABILITY__InvocationError::Domain)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum __CAPABILITY__InvocationError { Domain(__OPERATION_TYPE__Error), Runtime(RuntimeFailure) }
