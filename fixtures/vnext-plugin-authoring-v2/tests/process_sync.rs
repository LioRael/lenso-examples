use std::{fs, time::Duration};

use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    ExecutionClassId, PluginInstancePlan,
};
use lenso_bun_adapter::BunAdapter;
use lenso_capability_document_store::{
    CAPABILITY_ID as STORE_ID, DESCRIPTOR_VERSION as STORE_VERSION, DocumentStoreJsonCodec,
};
use lenso_capability_document_sync::{
    CAPABILITY_ID as SYNC_ID, DESCRIPTOR_VERSION as SYNC_VERSION, DocumentSync,
    DocumentSyncJsonCodec, SyncRequest,
};
use lenso_kernel::{DeterministicDriver, ExecutionAdapterCatalog, Kernel, RuntimeFailure};
use lenso_native_adapter::{
    NativePluginFactory, NativePluginFactoryContext, NativePluginInstance, NativePluginRegistry,
};
use lenso_process_adapter::{EXECUTION_CLASS, ProcessAdapter, RUNTIME_PROFILE_V2};
use lenso_runtime_codec::{ArtifactCatalog, ArtifactHandle};
use lenso_vnext_plugin_authoring_v2::stores::{Account, StoreCall, StoreFactory};
use sha2::{Digest as _, Sha256};

const SOURCE_PACKAGE: &str = "example.document-store.source";
const DESTINATION_PACKAGE: &str = "example.document-store.destination";
const SYNC_PACKAGE: &str = "example.document-sync.process";

#[derive(Debug)]
struct ConsumerFactory;

impl NativePluginFactory for ConsumerFactory {
    fn package_id(&self) -> &'static str {
        "example.document-sync.consumer"
    }

    fn instantiate(
        &self,
        _context: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}

fn artifact_catalog() -> ArtifactCatalog {
    let executable = std::path::Path::new(env!("CARGO_BIN_EXE_lenso-vnext-plugin-authoring-v2"));
    let bytes = fs::read(executable).expect("read Process artifact");
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let artifact = ArtifactHandle::open(executable, &digest, bytes.len() as u64)
        .expect("verify Process artifact");
    ArtifactCatalog::new()
        .with_artifact("sync", artifact)
        .expect("register Process artifact")
}

fn bun_artifact_catalog(path: &std::path::Path) -> ArtifactCatalog {
    let bytes = fs::read(path).expect("read Bun artifact");
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let artifact =
        ArtifactHandle::open(path, &digest, bytes.len() as u64).expect("verify Bun artifact");
    ArtifactCatalog::new()
        .with_artifact("sync", artifact)
        .expect("register Bun artifact")
}

fn composition(
    execution_class: ExecutionClassId,
    runtime_profile: &str,
    entrypoint: &str,
) -> lenso_app_plan::ResolvedAppPlan {
    AppComposition::new(
        vec![
            PluginInstancePlan::new("source-account", SOURCE_PACKAGE).with_capability(
                CapabilityEndpointPlan::new(STORE_ID, STORE_VERSION, ["put", "read"]),
            ),
            PluginInstancePlan::new("destination-account", DESTINATION_PACKAGE).with_capability(
                CapabilityEndpointPlan::new(STORE_ID, STORE_VERSION, ["put", "read"]),
            ),
            PluginInstancePlan::new("sync", SYNC_PACKAGE)
                .with_authoring(2, runtime_profile)
                .with_configuration(r#"{"document":"guide","ruleset":"identity-v1"}"#)
                .with_entrypoint(entrypoint)
                .with_execution_class(execution_class)
                .with_requirement(
                    CapabilityRequirementPlan::one(STORE_ID, STORE_VERSION)
                        .with_requirement_id("source"),
                )
                .with_requirement(
                    CapabilityRequirementPlan::one(STORE_ID, STORE_VERSION)
                        .with_requirement_id("destination"),
                )
                .with_capability(CapabilityEndpointPlan::new(SYNC_ID, SYNC_VERSION, ["sync"])),
            PluginInstancePlan::new("consumer", "example.document-sync.consumer")
                .with_requirement(CapabilityRequirementPlan::one(SYNC_ID, SYNC_VERSION)),
        ],
        vec![
            CapabilityBinding::new("sync", STORE_ID, STORE_VERSION, "source-account")
                .with_requirement_id("source"),
            CapabilityBinding::new("sync", STORE_ID, STORE_VERSION, "destination-account")
                .with_requirement_id("destination"),
            CapabilityBinding::new("consumer", SYNC_ID, SYNC_VERSION, "sync"),
        ],
    )
    .resolve()
    .expect("resolve named document sync composition")
}

#[test]
fn rust_process_syncs_between_two_exact_native_accounts() {
    let source = Account::with_document("guide", "one plugin model");
    let destination = Account::default();
    let native = NativePluginRegistry::new()
        .with_factory(StoreFactory::new(SOURCE_PACKAGE, source.clone()))
        .with_factory(StoreFactory::new(DESTINATION_PACKAGE, destination.clone()))
        .with_factory(ConsumerFactory);
    let process = ProcessAdapter::new(artifact_catalog())
        .with_codec(DocumentStoreJsonCodec)
        .with_codec(DocumentSyncJsonCodec);
    let adapters = ExecutionAdapterCatalog::new()
        .with_adapter(native)
        .expect("register Native adapter")
        .with_adapter(process)
        .expect("register Process adapter");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start(
            composition(
                ExecutionClassId::new(EXECUTION_CLASS),
                RUNTIME_PROFILE_V2,
                "plugin",
            ),
            driver.clone(),
            adapters,
        ))
        .expect("start mixed Native and Process App");

    let response = driver
        .run(
            app.handle::<DocumentSync>("consumer")
                .expect("consumer receives sync dependency")
                .invoke(
                    "sync",
                    SyncRequest {
                        document: "guide".to_owned(),
                    },
                ),
        )
        .expect("sync Runtime succeeds")
        .expect("sync domain succeeds");

    assert_eq!(response.text, "one plugin model");
    assert_eq!(
        destination.text("guide").as_deref(),
        Some("one plugin model")
    );
    assert_eq!(
        source.calls(),
        [StoreCall::Read {
            document: "guide".to_owned()
        }]
    );
    assert_eq!(
        destination.calls(),
        [StoreCall::Put {
            document: "guide".to_owned(),
            text: "one plugin model".to_owned()
        }]
    );
    assert_eq!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        lenso_kernel::ShutdownOutcome::Clean
    );
}

#[test]
#[ignore = "requires LENSO_DOCUMENT_SYNC_BUN_ARTIFACT from `lenso plugin pack`"]
fn typescript_bun_syncs_through_the_same_named_host_dependencies() {
    let artifact = std::path::PathBuf::from(
        std::env::var_os("LENSO_DOCUMENT_SYNC_BUN_ARTIFACT")
            .expect("set the extracted TypeScript Bun artifact path"),
    );
    let source = Account::with_document("guide", "one plugin model");
    let destination = Account::default();
    let native = NativePluginRegistry::new()
        .with_factory(StoreFactory::new(SOURCE_PACKAGE, source.clone()))
        .with_factory(StoreFactory::new(DESTINATION_PACKAGE, destination.clone()))
        .with_factory(ConsumerFactory);
    let bun = BunAdapter::production("bun")
        .with_artifacts(bun_artifact_catalog(&artifact))
        .with_authoring_codec(DocumentStoreJsonCodec)
        .with_authoring_codec(DocumentSyncJsonCodec);
    let adapters = ExecutionAdapterCatalog::new()
        .with_adapter(native)
        .expect("register Native adapter")
        .with_adapter(bun)
        .expect("register Bun adapter");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start(
            composition(
                ExecutionClassId::bun_child_process(),
                "lenso.bun-authoring@2",
                "plugin.js",
            ),
            driver.clone(),
            adapters,
        ))
        .expect("start mixed Native and Bun App");

    let response = driver
        .run(
            app.handle::<DocumentSync>("consumer")
                .expect("consumer receives sync dependency")
                .invoke(
                    "sync",
                    SyncRequest {
                        document: "guide".to_owned(),
                    },
                ),
        )
        .expect("sync Runtime succeeds")
        .expect("sync domain succeeds");

    assert_eq!(response.text, "one plugin model");
    assert_eq!(
        destination.text("guide").as_deref(),
        Some("one plugin model")
    );
    assert_eq!(
        driver.run(app.shutdown(Duration::from_secs(2))),
        lenso_kernel::ShutdownOutcome::Clean
    );
}
