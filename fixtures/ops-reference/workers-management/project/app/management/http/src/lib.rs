use lenso_capability_auth as auth;
use lenso_capability_http_endpoint as http;
use lenso_capability_management as management;
use lenso_capability_management_human as human;
use lenso_kernel::{InvocationContext, NativeRequestFuture};
use lenso_management_http::{BearerHttpProvider, McpAuthentication, Profile};
use serde::Deserialize;
use http::EndpointProvider as _;

pub mod host_facilities;
use host_facilities::McpHandle;

#[derive(Clone, Debug, Deserialize, lenso::PluginConfig)]
#[serde(deny_unknown_fields)]
struct Config {
    public_origin: String,
    mcp_resource: Option<String>,
    mcp_authentication: Option<String>,
    authorization_servers: Vec<String>,
    allowed_origins: Vec<String>,
}

#[lenso::plugin]
#[derive(Clone, Debug)]
struct ManagementWeb {
    #[config]
    config: Config,
    #[dependency(id = "auth")]
    auth: auth::AuthClient,
    #[dependency(id = "management")]
    management: management::ManagementClient,
    #[dependency(id = "human")]
    human: human::ManagementHumanClient,
    #[facility(id = "mcp")]
    mcp: Option<McpHandle>,
    provider: BearerHttpProvider,
}

#[lenso::plugin_impl]
impl ManagementWeb {
    #[create]
    fn create(
        config: Config,
        auth: auth::AuthClient,
        management: management::ManagementClient,
        human: human::ManagementHumanClient,
        mcp: Option<McpHandle>,
    ) -> Result<Self, &'static str> {
        let transport = match &mcp {
            Some(handle) => Some(handle.transport(
                config.mcp_resource.clone().ok_or("MCP resource is required")?,
            )?),
            None => None,
        };
        let provider = BearerHttpProvider::new(
            Profile {
                public_origin: config.public_origin.clone(),
                mcp_resource: config.mcp_resource.clone(),
                mcp_authentication: match config.mcp_authentication.as_deref().unwrap_or("oauth") {
                    "oauth" => McpAuthentication::OAuth,
                    "preissued-bearer" => McpAuthentication::PreissuedBearer,
                    _ => return Err("Unsupported MCP authentication profile"),
                },
                authorization_servers: config.authorization_servers.clone(),
                allowed_origins: config.allowed_origins.clone(),
            }, auth.clone(), management.clone(), human.clone(),
            transport,
        )?;
        Ok(Self {config, auth, management, human, mcp, provider})
    }
}

#[lenso::provides(http::Endpoint)]
impl ManagementWeb {
    fn describe(
        &self, context: InvocationContext, request: http::DescribeRequest,
    ) -> NativeRequestFuture<http::EndpointDescribe> {
        self.provider.describe(context, request)
    }
    fn handle(
        &self, context: InvocationContext, request: http::HandleRequest,
    ) -> NativeRequestFuture<http::EndpointHandle> {
        self.provider.handle(context, request)
    }
}
