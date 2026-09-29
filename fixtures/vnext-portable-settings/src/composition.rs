use std::rc::Rc;

use lenso::{ConfiguredPluginFactory, NativePluginDefinition};
use lenso_app_plan::{ResolvedAppPlan, authoring::*};
use lenso_kernel::RuntimeFailure;
use lenso_native_adapter::{NativePluginFactory, NativePluginRegistry};
use lenso_web_ingress_plugin::{WebIngressConfig, WebIngressEventFactory};

use crate::{
    endpoint::SettingsHttp,
    failure,
    storage::{Persistence, Store},
};

/// Only the Host owns availability and slot policy. The public resolver owns
/// all Instance and binding lowering; this fixture never authors a derived Plan.
pub fn catalog() -> HostCatalog {
    SettingsHttp::link();
    Store::link();
    let linked = NativePluginRegistry::host_catalog(
        [
            HostSlot::one("settings-http"),
            HostSlot::optional("settings-store"),
            HostSlot::one("http-ingress"),
        ],
        [],
    )
    .expect("generated Plugin descriptors");
    HostCatalog::new(
        linked.slots().to_vec(),
        linked
            .plugins()
            .iter()
            .cloned()
            .chain([HostPluginRelease::new(
                WebIngressEventFactory::plugin_descriptor(),
            )]),
        [],
    )
}

pub fn root(with_store: bool) -> Result<PluginRootSnapshot, RuntimeFailure> {
    let mut instances = vec![
        PluginRootInstance::new(crate::endpoint::PACKAGE_ID, "default"),
        PluginRootInstance::new(lenso_web_ingress_plugin::PACKAGE_ID, "default")
            .with_configuration(
                serde_json::to_value(
                    WebIngressConfig::default()
                        .with_bind_address(([127, 0, 0, 1], 0).into())
                        .map_err(failure)?,
                )
                .map_err(failure)?,
            ),
    ];
    if with_store {
        instances.push(PluginRootInstance::new(
            crate::storage::PACKAGE_ID,
            "default",
        ));
    }
    Ok(PluginRootSnapshot::new([], instances, []))
}

pub fn plan() -> Result<ResolvedAppPlan, RuntimeFailure> {
    Ok(resolve_plugin_root(&catalog(), &root(true)?)
        .map_err(failure)?
        .plan()
        .clone())
}

pub fn registry(
    resource: Rc<dyn Persistence>,
    ingress: impl NativePluginFactory,
) -> NativePluginRegistry {
    SettingsHttp::link();
    Store::link();
    NativePluginRegistry::new()
        .with_factory_override(ConfiguredPluginFactory::<Store, _>::new(move |plugin| {
            plugin.persistence = Some(resource.clone());
            Ok(())
        }))
        .expect("one approved storage resource binding")
        .with_linked_factories()
        .with_factory(ingress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_packages_link_generated_identities_and_factories() {
        let catalog = catalog();
        assert_eq!(catalog.plugins().len(), 3);
        assert!(catalog.plugins().iter().any(|plugin| {
            plugin.descriptor().plugin_id() == <SettingsHttp as NativePluginDefinition>::PACKAGE_ID
        }));
        assert!(catalog.plugins().iter().any(|plugin| {
            plugin.descriptor().plugin_id() == <Store as NativePluginDefinition>::PACKAGE_ID
        }));
        let linked = NativePluginRegistry::new().with_linked_factories();
        for id in [crate::endpoint::PACKAGE_ID, crate::storage::PACKAGE_ID] {
            assert_eq!(
                linked
                    .factories()
                    .filter(|factory| factory.package_id() == id)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn resolver_binds_two_business_plugins_and_ingress() {
        let plan = plan().unwrap();
        assert_eq!(plan.plugin_instances().len(), 3);
        let binding = plan
            .capability_bindings()
            .iter()
            .find(|binding| binding.capability_id() == crate::contract::CAPABILITY_ID)
            .unwrap();
        assert_eq!(binding.consumer_instance(), "example.settings-http/default");
        assert_eq!(
            binding.provider_instance(),
            "example.settings-store/default"
        );
        assert_eq!(
            plan.plugin_instance(binding.consumer_instance())
                .unwrap()
                .required_capabilities()
                .len(),
            1
        );
    }

    #[test]
    fn deleting_storage_fails_resolution_instead_of_hiding_a_bridge() {
        assert!(resolve_plugin_root(&catalog(), &root(false).unwrap()).is_err());
    }

    #[test]
    fn kernel_rejects_storage_without_private_host_resource_before_ready() {
        use lenso_kernel::{DeterministicDriver, Kernel};

        let plan = plan().unwrap();
        let driver = DeterministicDriver::new();
        let registry = NativePluginRegistry::new()
            .with_linked_factories()
            .with_factory(WebIngressEventFactory::new());
        let result = driver.run(Kernel::start_native(plan, driver.clone(), registry));
        let error = match result {
            Ok(_) => panic!("unconfigured storage must not become ready"),
            Err(error) => error,
        };
        assert!(format!("{error:?}").contains("persistence was not attached by Host"));
    }

    #[test]
    fn kernel_connects_generated_endpoints_before_any_database_is_started() {
        use crate::{contract::*, storage::*};
        use futures::future::LocalBoxFuture;
        use lenso_kernel::{CancellationToken, DeterministicDriver, Kernel};

        #[derive(Debug)]
        struct Unavailable;
        impl Persistence for Unavailable {
            fn read(&self, _: ReadRequest) -> LocalBoxFuture<'static, ReadResult> {
                Box::pin(async { Err(failure("deliberately unavailable")) })
            }
            fn change(&self, _: ChangeRequest) -> LocalBoxFuture<'static, ChangeResult> {
                Box::pin(async { Err(failure("deliberately unavailable")) })
            }
        }
        let driver = DeterministicDriver::new();
        let ingress = WebIngressEventFactory::new();
        let app = driver
            .run(Kernel::start_native(
                plan().unwrap(),
                driver.clone(),
                registry(Rc::new(Unavailable), ingress.clone()),
            ))
            .unwrap();
        assert!(app.is_ready());
        let request = crate::host::Input {
            method: "GET".into(),
            uri: "/settings".into(),
            content_type: "application/json".into(),
            principal: "test".into(),
            body: String::new(),
        }
        .request()
        .unwrap();
        let response = driver
            .run(ingress.handle(request, CancellationToken::new()))
            .unwrap();
        assert_eq!(response.status(), 503);
        assert!(matches!(
            driver.run(app.shutdown(std::time::Duration::from_secs(1))),
            lenso_kernel::ShutdownOutcome::Clean,
        ));
    }
}
