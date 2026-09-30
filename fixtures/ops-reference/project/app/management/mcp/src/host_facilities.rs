use std::{net::SocketAddr, time::Duration};

#[derive(Clone, Debug)]
pub struct TransportHandle {
    pub listen: SocketAddr,
    pub resource_uri: String,
    pub clock: lenso_native_adapter::NativeHostClock,
}
impl TransportHandle {
    pub fn now(&self) -> Duration {
        self.clock.now()
    }
}

pub fn transport(
    binding: &serde_json::Value,
    clock: &lenso_native_adapter::NativeHostClock,
) -> Result<TransportHandle, lenso::RuntimeFailure> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Binding {
        listen: SocketAddr,
        resource_uri: String,
    }
    let invalid = || lenso::RuntimeFailure::InvalidResolvedPlan {
        detail: "MCP requires an explicit matching loopback listener and resource URI".into(),
    };
    let selected: Binding = serde_json::from_value(binding.clone()).map_err(|_| invalid())?;
    let url = url::Url::parse(&selected.resource_uri).map_err(|_| invalid())?;
    if !selected.listen.ip().is_loopback()
        || selected.listen.port() == 0
        || url.scheme() != "http"
        || url.host_str() != Some(selected.listen.ip().to_string().as_str())
        || url.port_or_known_default() != Some(selected.listen.port())
        || url.path() != "/mcp"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    Ok(TransportHandle {
        listen: selected.listen,
        resource_uri: selected.resource_uri,
        clock: clock.clone(),
    })
}
