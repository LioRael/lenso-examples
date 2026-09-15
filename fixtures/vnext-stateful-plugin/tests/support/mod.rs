use std::{collections::BTreeMap, rc::Rc};

use lenso_capability_secrets::{
    ResolveError, ResolveRequest, ResolveResponse, Secrets, SecretsEndpoint, SecretsProvider,
};
use lenso_kernel::{InvocationContext, NativeRequestFuture, RuntimeFailure};
use lenso_native_adapter::{NativePluginFactory, NativePluginFactoryContext, NativePluginInstance};

pub const SECRETS_PACKAGE_ID: &str = "example.fixture-secrets";
pub const CALLER_PACKAGE_ID: &str = "example.counter-caller";

#[derive(Debug)]
struct FixtureSecretsProvider {
    values: BTreeMap<String, String>,
}

impl SecretsProvider for FixtureSecretsProvider {
    fn resolve(
        &self,
        _context: InvocationContext,
        request: ResolveRequest,
    ) -> NativeRequestFuture<Secrets> {
        let result = self
            .values
            .get(&request.reference)
            .cloned()
            .map(|value| ResolveResponse { value })
            .ok_or(ResolveError::UnknownReference);
        Box::pin(async move { Ok(result) })
    }
}

#[derive(Clone, Debug)]
pub struct SecretsFactory {
    values: BTreeMap<String, String>,
}

impl SecretsFactory {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }
}

impl NativePluginFactory for SecretsFactory {
    fn package_id(&self) -> &'static str {
        SECRETS_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::new(vec![Rc::new(
            SecretsEndpoint::new(FixtureSecretsProvider {
                values: self.values.clone(),
            }),
        )]))
    }
}

#[derive(Debug)]
pub struct CallerFactory;

impl NativePluginFactory for CallerFactory {
    fn package_id(&self) -> &'static str {
        CALLER_PACKAGE_ID
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}
