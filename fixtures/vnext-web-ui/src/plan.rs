use std::path::{Path, PathBuf};

use lenso_authoring::{
    Binding, CapabilityEndpoint, CapabilityRequirement, ContractInput, Module, ModuleRole,
    PackageInput, PackageSource, ProjectFile, WebProfile,
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
const FIXTURE_CRATE: &str = "lenso-vnext-web-ui";
const FIXTURE_VERSION: &str = "0.1.0";

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn project(metadata: MetadataScenario, web_enabled: bool) -> ProjectFile {
    let mut project = ProjectFile::default();
    add_contracts(&mut project);
    for package_id in [
        ORDERS_PACKAGE_ID,
        "fixture.web-auth",
        "fixture.orders-worker",
        "lenso.web-shell",
        "lenso.browser-adapter",
    ] {
        project.packages_mut().insert(
            package_id.to_owned(),
            PackageInput::new(package_id, PackageSource::Cargo, FIXTURE_VERSION)
                .with_package_name(FIXTURE_CRATE)
                .with_manifest("Cargo.toml")
                .with_lockfile("Cargo.lock"),
        );
    }
    add_base_modules(&mut project);
    if web_enabled {
        add_web_modules(&mut project, metadata);
    }
    project
}

fn add_contracts(project: &mut ProjectFile) {
    for (directory, capability_id) in [
        ("lenso-capability-auth", AUTH_CAPABILITY_ID),
        ("lenso-capability-secure-greeting", GREETING_CAPABILITY_ID),
        ("lenso-capability-ui-contribution", UI_CAPABILITY_ID),
        ("lenso-capability-web-shell", SHELL_CAPABILITY_ID),
    ] {
        project.contracts_mut().push(ContractInput::new(
            capability_id,
            "1.0.0",
            format!("crates/{directory}/capability.json"),
            format!("crates/{directory}/src/generated.rs"),
            format!("crates/{directory}/generated/bindings.ts"),
        ));
    }
}

fn add_base_modules(project: &mut ProjectFile) {
    let composition = project.composition_mut();
    composition.add_module(
        Module::new("orders", ORDERS_PACKAGE_ID)
            .with_entrypoint("backend")
            .with_capability(CapabilityEndpoint::request(
                GREETING_CAPABILITY_ID,
                GREETING_DESCRIPTOR_VERSION,
                [GREET_OPERATION],
            )),
    );
    composition.add_module(Module::new("auth", "fixture.web-auth").with_capability(
        CapabilityEndpoint::request(
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
            [AUTHENTICATE_OPERATION],
        ),
    ));
    composition.add_module(
        Module::new("worker", "fixture.orders-worker")
            .with_requirement(CapabilityRequirement::one(
                AUTH_CAPABILITY_ID,
                AUTH_DESCRIPTOR_VERSION,
            ))
            .with_requirement(CapabilityRequirement::one(
                GREETING_CAPABILITY_ID,
                GREETING_DESCRIPTOR_VERSION,
            )),
    );
    add_binding(
        composition,
        "worker",
        AUTH_CAPABILITY_ID,
        AUTH_DESCRIPTOR_VERSION,
        "auth",
    );
    add_binding(
        composition,
        "worker",
        GREETING_CAPABILITY_ID,
        GREETING_DESCRIPTOR_VERSION,
        "orders",
    );
}

fn add_web_modules(project: &mut ProjectFile, metadata: MetadataScenario) {
    let composition = project.composition_mut();
    composition.add_module(ui_module(
        "orders-ui",
        &contribution_configuration(metadata, false),
    ));
    composition.add_module(
        Module::new("web-shell", "lenso.web-shell")
            .with_role(ModuleRole::WebShell)
            .with_capability(CapabilityEndpoint::request(
                SHELL_CAPABILITY_ID,
                SHELL_DESCRIPTOR_VERSION,
                [READ_ASSET_OPERATION, RENDER_ROUTE_OPERATION],
            ))
            .with_requirement(CapabilityRequirement::many(
                UI_CAPABILITY_ID,
                UI_DESCRIPTOR_VERSION,
            )),
    );
    composition.add_module(
        Module::new("browser-adapter", "lenso.browser-adapter")
            .with_role(ModuleRole::BrowserAdapter)
            .with_requirement(CapabilityRequirement::one(
                SHELL_CAPABILITY_ID,
                SHELL_DESCRIPTOR_VERSION,
            ))
            .with_requirement(CapabilityRequirement::one(
                AUTH_CAPABILITY_ID,
                AUTH_DESCRIPTOR_VERSION,
            ))
            .with_requirement(CapabilityRequirement::one(
                GREETING_CAPABILITY_ID,
                GREETING_DESCRIPTOR_VERSION,
            )),
    );
    for (consumer, capability, version, provider) in [
        (
            "orders-ui",
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
            "orders",
        ),
        (
            "web-shell",
            UI_CAPABILITY_ID,
            UI_DESCRIPTOR_VERSION,
            "orders-ui",
        ),
        (
            "browser-adapter",
            SHELL_CAPABILITY_ID,
            SHELL_DESCRIPTOR_VERSION,
            "web-shell",
        ),
        (
            "browser-adapter",
            AUTH_CAPABILITY_ID,
            AUTH_DESCRIPTOR_VERSION,
            "auth",
        ),
        (
            "browser-adapter",
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
            "orders",
        ),
    ] {
        add_binding(composition, consumer, capability, version, provider);
    }
    let mut profile = WebProfile::new("web-shell", "browser-adapter")
        .with_ui_contribution("orders-ui")
        .with_module("orders")
        .with_module("auth")
        .with_module("worker");
    if metadata == MetadataScenario::CollidingRoute {
        composition.add_module(ui_module(
            "orders-ui-copy",
            &contribution_configuration(metadata, true),
        ));
        add_binding(
            composition,
            "orders-ui-copy",
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
            "orders",
        );
        add_binding(
            composition,
            "web-shell",
            UI_CAPABILITY_ID,
            UI_DESCRIPTOR_VERSION,
            "orders-ui-copy",
        );
        profile = profile.with_ui_contribution("orders-ui-copy");
    }
    project.profiles_mut().insert("web".to_owned(), profile);
}

fn ui_module(key: &str, configuration: &str) -> Module {
    Module::new(key, ORDERS_PACKAGE_ID)
        .with_entrypoint("ui")
        .with_configuration(serde_json::from_str(configuration).expect("fixture configuration"))
        .with_configuration_schema("fixtures/vnext-web-ui/contribution-configuration.schema.json")
        .with_role(ModuleRole::UiContribution)
        .with_capability(CapabilityEndpoint::request(
            UI_CAPABILITY_ID,
            UI_DESCRIPTOR_VERSION,
            [DESCRIBE_OPERATION],
        ))
        .with_requirement(CapabilityRequirement::one(
            GREETING_CAPABILITY_ID,
            GREETING_DESCRIPTOR_VERSION,
        ))
}

fn add_binding(
    composition: &mut lenso_authoring::CompositionFile,
    consumer: &str,
    capability: &str,
    version: &str,
    provider: &str,
) {
    composition.add_binding(Binding::new(consumer, capability, version, provider));
}
