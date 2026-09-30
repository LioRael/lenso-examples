use lenso_capability_secrets as secrets;
pub mod host_facilities;
use host_facilities::SecretHandle;

#[lenso::plugin]
#[derive(Clone, Debug)]
struct WorkerSecrets {
    #[facility(id = "secrets")]
    host: SecretHandle,
}

#[lenso::provides(secrets::Secrets)]
impl WorkerSecrets {
    fn resolve(
        &self, context: lenso_kernel::InvocationContext, request: secrets::ResolveRequest,
    ) -> lenso_kernel::NativeRequestFuture<secrets::Secrets> {
        let host = self.host.clone();
        Box::pin(async move {
            let Some(caller) = context.caller_instance() else {
                return Ok(Err(secrets::ResolveError::UnknownReference));
            };
            host.resolve(&request.reference, caller).await.map(|value| {
                value.map(|value| secrets::ResolveResponse {value})
                    .ok_or(secrets::ResolveError::UnknownReference)
            })
        })
    }
}
