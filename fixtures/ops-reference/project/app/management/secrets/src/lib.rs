use lenso_capability_secrets as secrets;
pub mod host_facilities;
use host_facilities::SecretHandle;

#[lenso::plugin]
#[derive(Clone, Debug)]
struct FileSecrets {
    #[facility(id = "secrets")]
    host: SecretHandle,
}
#[lenso::provides(secrets::Secrets)]
impl FileSecrets {
    async fn resolve(
        &self,
        context: lenso::Ctx,
        request: secrets::ResolveRequest,
    ) -> Result<secrets::ResolveResponse, secrets::ResolveError> {
        self.host
            .resolve(&request.reference, context.caller_instance())
            .map(|value| secrets::ResolveResponse { value })
            .ok_or(secrets::ResolveError::UnknownReference)
    }
}
