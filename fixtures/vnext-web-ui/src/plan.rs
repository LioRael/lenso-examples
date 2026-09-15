use lenso_app_plan::{
    CapabilityEndpointPlan, CapabilityRequirementPlan, ResolvedAppPlan,
    authoring::{
        HostBinding, HostCatalog, HostDefaultPlugin, HostPluginRelease, HostSlot, PluginDescriptor,
        PluginInstanceId, PluginRootResolutionError, PluginRootSnapshot, resolve_plugin_root,
    },
};
use lenso_capability_auth::{
    AUTHENTICATE_OPERATION, CAPABILITY_ID as AUTH_CAPABILITY_ID,
    DESCRIPTOR_VERSION as AUTH_DESCRIPTOR_VERSION,
};
use lenso_capability_secure_greeting::{
    CAPABILITY_ID as GREETING_CAPABILITY_ID, DESCRIPTOR_VERSION as GREETING_DESCRIPTOR_VERSION,
    GREET_OPERATION,
};
use lenso_capability_ui_contribution::{
    CAPABILITY_ID as UI_CAPABILITY_ID, DESCRIBE_OPERATION,
    DESCRIPTOR_VERSION as UI_DESCRIPTOR_VERSION,
};
use lenso_capability_web_shell::{
    CAPABILITY_ID as SHELL_CAPABILITY_ID, DESCRIPTOR_VERSION as SHELL_DESCRIPTOR_VERSION,
    READ_ASSET_OPERATION, RENDER_ROUTE_OPERATION,
};

use crate::{MetadataScenario, ORDERS_PACKAGE_ID, orders::contribution_configuration};

pub const UI_CONTRIBUTION_CAPABILITY_ID: &str = UI_CAPABILITY_ID;
pub const WEB_SHELL_CAPABILITY_ID: &str = SHELL_CAPABILITY_ID;
const RELEASE_VERSION: &str = "0.1.0";
const DEFAULT_INSTANCE: &str = "default";

const ORDERS: &str = "orders";
const AUTH: &str = "auth";
const WORKER: &str = "worker";
const ORDERS_UI: &str = "orders-ui";
const ORDERS_UI_COPY: &str = "orders-ui-copy";
const WEB_SHELL: &str = "web-shell";
const BROWSER_ADAPTER: &str = "browser-adapter";

pub fn plugin_root(web_enabled: bool) -> PluginRootSnapshot {
    let disabled = if web_enabled {
        Vec::new()
    } else {
        [ORDERS_UI, WEB_SHELL, BROWSER_ADAPTER]
            .into_iter()
            .map(instance_id)
            .collect()
    };
    PluginRootSnapshot::new([], [], disabled)
}

pub fn resolve(
    metadata: MetadataScenario,
    web_enabled: bool,
) -> Result<ResolvedAppPlan, PluginRootResolutionError> {
    resolve_plugin_root(&host_catalog(metadata), &plugin_root(web_enabled))
        .map(|app| app.plan().clone())
}

fn host_catalog(metadata: MetadataScenario) -> HostCatalog {
    let slots = vec![
        HostSlot::one(ORDERS),
        HostSlot::one(AUTH),
        HostSlot::one(WORKER),
        HostSlot::many("ui"),
        HostSlot::optional(WEB_SHELL),
        HostSlot::optional(BROWSER_ADAPTER),
    ];
    let mut releases = vec![
        release(orders_backend()),
        release(auth()),
        release(worker()),
        release(orders_ui(
            ORDERS_UI,
            &contribution_configuration(metadata, false),
        )),
        release(web_shell()),
        release(browser_adapter()),
    ];
    let mut defaults = vec![
        default(ORDERS, false),
        default(AUTH, false),
        default(WORKER, false),
        default(ORDERS_UI, true),
        default(WEB_SHELL, true),
        default(BROWSER_ADAPTER, true),
    ];
    let mut bindings = vec![
        binding(WORKER, AUTH_CAPABILITY_ID, AUTH),
        binding(WORKER, GREETING_CAPABILITY_ID, ORDERS),
        binding(ORDERS_UI, GREETING_CAPABILITY_ID, ORDERS),
        HostBinding::new(instance_id(WEB_SHELL), UI_CAPABILITY_ID, "ui"),
        binding(BROWSER_ADAPTER, SHELL_CAPABILITY_ID, WEB_SHELL),
        binding(BROWSER_ADAPTER, AUTH_CAPABILITY_ID, AUTH),
        binding(BROWSER_ADAPTER, GREETING_CAPABILITY_ID, ORDERS),
    ];

    if metadata == MetadataScenario::CollidingRoute {
        releases.push(release(orders_ui(
            ORDERS_UI_COPY,
            &contribution_configuration(metadata, true),
        )));
        defaults.push(default(ORDERS_UI_COPY, true));
        bindings.push(binding(ORDERS_UI_COPY, GREETING_CAPABILITY_ID, ORDERS));
    }

    HostCatalog::new(slots, releases, defaults).with_bindings(bindings)
}

fn orders_backend() -> PluginDescriptor {
    PluginDescriptor::new(ORDERS, RELEASE_VERSION, ORDERS)
        .with_runtime_package(ORDERS_PACKAGE_ID, RELEASE_VERSION)
        .with_entrypoint("backend")
        .with_capability(CapabilityEndpointPlan::new(
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
            [GREET_OPERATION],
        ))
}

fn auth() -> PluginDescriptor {
    PluginDescriptor::new(AUTH, RELEASE_VERSION, AUTH)
        .with_runtime_package("fixture.web-auth", RELEASE_VERSION)
        .with_capability(CapabilityEndpointPlan::new(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
            [AUTHENTICATE_OPERATION],
        ))
}

fn worker() -> PluginDescriptor {
    PluginDescriptor::new(WORKER, RELEASE_VERSION, WORKER)
        .with_runtime_package("fixture.orders-worker", RELEASE_VERSION)
        .with_requirement(CapabilityRequirementPlan::one(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
        ))
}

fn orders_ui(plugin_id: &str, configuration: &str) -> PluginDescriptor {
    PluginDescriptor::new(plugin_id, RELEASE_VERSION, "ui")
        .with_runtime_package(ORDERS_PACKAGE_ID, RELEASE_VERSION)
        .with_entrypoint("ui")
        .with_configuration_schema(
            serde_json::from_str(include_str!("../contribution-configuration.schema.json"))
                .expect("fixture configuration schema is valid"),
        )
        .with_configuration_defaults(
            serde_json::from_str(configuration).expect("fixture configuration is valid"),
        )
        .with_capability(CapabilityEndpointPlan::new(
            UI_CAPABILITY_ID,
            UI_DESCRIPTOR_VERSION,
            [DESCRIBE_OPERATION],
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
        ))
}

fn web_shell() -> PluginDescriptor {
    PluginDescriptor::new(WEB_SHELL, RELEASE_VERSION, WEB_SHELL)
        .with_runtime_package("lenso.web-shell", RELEASE_VERSION)
        .with_capability(CapabilityEndpointPlan::new(
            SHELL_CAPABILITY_ID,
            SHELL_DESCRIPTOR_VERSION,
            [READ_ASSET_OPERATION, RENDER_ROUTE_OPERATION],
        ))
        .with_requirement(CapabilityRequirementPlan::many(
            UI_CAPABILITY_ID,
            UI_DESCRIPTOR_VERSION,
        ))
}

fn browser_adapter() -> PluginDescriptor {
    PluginDescriptor::new(BROWSER_ADAPTER, RELEASE_VERSION, BROWSER_ADAPTER)
        .with_runtime_package("lenso.browser-adapter", RELEASE_VERSION)
        .with_requirement(CapabilityRequirementPlan::one(
            SHELL_CAPABILITY_ID,
            SHELL_DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
        ))
        .with_requirement(CapabilityRequirementPlan::one(
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
        ))
}

fn release(descriptor: PluginDescriptor) -> HostPluginRelease {
    HostPluginRelease::new(descriptor)
}

fn default(plugin_id: &str, disableable: bool) -> HostDefaultPlugin {
    let plugin = HostDefaultPlugin::new(plugin_id, DEFAULT_INSTANCE);
    if disableable {
        plugin.disableable()
    } else {
        plugin
    }
}

fn instance_id(plugin_id: &str) -> PluginInstanceId {
    PluginInstanceId::new(plugin_id, DEFAULT_INSTANCE)
}

fn binding(consumer: &str, capability_id: &str, provider: &str) -> HostBinding {
    HostBinding::to_instance(instance_id(consumer), capability_id, instance_id(provider))
}
