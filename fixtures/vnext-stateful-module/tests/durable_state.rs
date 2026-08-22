use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    ModuleInstancePlan, ResolvedAppPlan,
};
use lenso_capability_counter::{
    CAPABILITY_ID as COUNTER_ID, CounterIncrement, CounterRead,
    DESCRIPTOR_VERSION as COUNTER_VERSION, INCREMENT_OPERATION, IncrementRequest, READ_OPERATION,
    ReadRequest,
};
use lenso_capability_secrets::{
    CAPABILITY_ID as SECRETS_ID, DESCRIPTOR_VERSION as SECRETS_VERSION, RESOLVE_OPERATION,
};
use lenso_kernel::{DeterministicDriver, Kernel, RuntimeFailure, ShutdownOutcome};
use lenso_native_adapter::NativeModuleRegistry;
use lenso_vnext_stateful_module::{
    COUNTER_PACKAGE_ID, CounterFactory, RecoveryOutcome, SetupOutcome, UpgradeOutcome,
    recover_owned_state, setup_owned_state, upgrade_owned_state,
};

mod support;
use support::{CALLER_PACKAGE_ID, CallerFactory, SECRETS_PACKAGE_ID, SecretsFactory};

static NEXT_STORAGE_ID: AtomicUsize = AtomicUsize::new(0);

struct TestStorage {
    root: PathBuf,
    path: PathBuf,
}

impl TestStorage {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lenso-stateful-{}-{}",
            std::process::id(),
            NEXT_STORAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            path: root.join("counter.json"),
            root,
        }
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn plan(storage_path: &Path) -> ResolvedAppPlan {
    let state = ModuleInstancePlan::new("state", COUNTER_PACKAGE_ID)
        .with_configuration(
            serde_json::json!({
                "storage_path": storage_path,
                "secret_ref": "counter-key"
            })
            .to_string(),
        )
        .with_capability(CapabilityEndpointPlan::new(
            COUNTER_ID,
            COUNTER_VERSION,
            [INCREMENT_OPERATION, READ_OPERATION],
        ))
        .with_requirement(CapabilityRequirementPlan::one(SECRETS_ID, SECRETS_VERSION));
    let secrets = ModuleInstancePlan::new("secrets", SECRETS_PACKAGE_ID).with_capability(
        CapabilityEndpointPlan::new(SECRETS_ID, SECRETS_VERSION, [RESOLVE_OPERATION]),
    );
    let caller = ModuleInstancePlan::new("caller", CALLER_PACKAGE_ID)
        .with_requirement(CapabilityRequirementPlan::one(COUNTER_ID, COUNTER_VERSION));
    AppComposition::new(
        vec![caller, state, secrets],
        vec![
            CapabilityBinding::new("caller", COUNTER_ID, COUNTER_VERSION, "state"),
            CapabilityBinding::new("state", SECRETS_ID, SECRETS_VERSION, "secrets"),
        ],
    )
    .resolve()
    .expect("stateful fixture Composition should resolve")
}

fn registry(secret: Option<&str>) -> NativeModuleRegistry {
    let values = secret
        .map(|value| BTreeMap::from([(String::from("counter-key"), value.to_owned())]))
        .unwrap_or_default();
    NativeModuleRegistry::new()
        .with_factory(CallerFactory)
        .with_factory(CounterFactory)
        .with_factory(SecretsFactory::new(values))
}

#[test]
fn setup_is_explicit_and_reports_a_reviewable_outcome() {
    let storage = TestStorage::new();

    assert_eq!(
        setup_owned_state(&storage.path).expect("setup should apply the owned migration"),
        SetupOutcome::Created { schema_version: 1 }
    );
    assert_eq!(
        setup_owned_state(&storage.path).expect("setup should be idempotent"),
        SetupOutcome::AlreadyCurrent { schema_version: 1 }
    );
    assert_eq!(
        upgrade_owned_state(&storage.path).expect("upgrade should report current schema"),
        UpgradeOutcome::AlreadyCurrent { schema_version: 1 }
    );
}

#[test]
fn durable_counter_behavior_survives_module_restart_through_the_public_capability() {
    let storage = TestStorage::new();
    setup_owned_state(&storage.path).expect("explicit setup should create durable storage");

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path),
            driver.clone(),
            registry(Some("fixture-secret")),
        ))
        .expect("configured storage and Secrets should start the App");
    let first = driver
        .run(app.invoke::<CounterIncrement>(
            "caller",
            INCREMENT_OPERATION,
            IncrementRequest {
                key: "alpha".to_owned(),
                amount: 2,
            },
        ))
        .expect("increment should reach the public Capability")
        .expect("increment should be accepted");
    assert_eq!(first.value, 2);
    assert_eq!(first.revision, "1");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path),
            driver.clone(),
            registry(Some("fixture-secret")),
        ))
        .expect("the same durable storage should be reusable after restart");
    let after_restart = driver
        .run(app.invoke::<CounterRead>(
            "caller",
            READ_OPERATION,
            ReadRequest {
                key: "alpha".to_owned(),
            },
        ))
        .expect("read should reach the public Capability")
        .expect("the value should survive restart");
    assert_eq!(after_restart.value, 2);
    assert_eq!(after_restart.revision, "1");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn missing_storage_fails_preparation_without_an_in_memory_fallback() {
    let storage = TestStorage::new();
    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path),
        driver.clone(),
        registry(Some("fixture-secret")),
    ));

    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail })
            if detail.contains("required durable storage") && detail.contains("run setup")
    ));
    assert!(!storage.path.exists());
}

#[cfg(unix)]
#[test]
fn unwritable_storage_fails_preparation_before_the_first_operation() {
    use std::os::unix::fs::PermissionsExt;

    let storage = TestStorage::new();
    setup_owned_state(&storage.path).expect("explicit setup should create durable storage");
    let original_mode = std::fs::metadata(&storage.path)
        .expect("storage metadata should be readable")
        .permissions()
        .mode();
    std::fs::set_permissions(&storage.path, std::fs::Permissions::from_mode(0o444))
        .expect("storage should become read-only");

    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path),
        driver.clone(),
        registry(Some("fixture-secret")),
    ));

    std::fs::set_permissions(
        &storage.path,
        std::fs::Permissions::from_mode(original_mode),
    )
    .expect("storage permissions should be restored");
    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail }) if detail.contains("open storage read-write")
    ));
}

#[test]
fn missing_secret_fails_activation_without_starting_the_state_module() {
    let storage = TestStorage::new();
    setup_owned_state(&storage.path).expect("explicit setup should create durable storage");
    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path),
        driver.clone(),
        registry(None),
    ));

    assert!(matches!(
        result,
        Err(RuntimeFailure::ModuleFailure { detail })
            if detail.contains("required secret reference `counter-key`")
    ));
}

#[test]
fn secret_rotation_preserves_owned_state_identity() {
    let storage = TestStorage::new();
    setup_owned_state(&storage.path).expect("explicit setup should create durable storage");

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path),
            driver.clone(),
            registry(Some("fixture-secret")),
        ))
        .expect("the initial secret should activate the state owner");
    driver
        .run(app.invoke::<CounterIncrement>(
            "caller",
            INCREMENT_OPERATION,
            IncrementRequest {
                key: "rotated".to_owned(),
                amount: 3,
            },
        ))
        .expect("increment should reach the public Capability")
        .expect("increment should succeed");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path),
            driver.clone(),
            registry(Some("different-secret")),
        ))
        .expect("routine secret rotation should not change durable state identity");
    let value = driver
        .run(app.invoke::<CounterRead>(
            "caller",
            READ_OPERATION,
            ReadRequest {
                key: "rotated".to_owned(),
            },
        ))
        .expect("read should reach the public Capability")
        .expect("state should remain available after secret rotation");
    assert_eq!(value.value, 3);
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn interrupted_commit_requires_explicit_recovery_before_preparation() {
    let storage = TestStorage::new();
    setup_owned_state(&storage.path).expect("explicit setup should create durable storage");
    let temporary_path = PathBuf::from(format!("{}.tmp", storage.path.display()));
    std::fs::copy(&storage.path, &temporary_path).expect("interrupted commit should be simulated");
    std::fs::remove_file(&storage.path).expect("committed document should be absent");

    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path),
        driver.clone(),
        registry(Some("fixture-secret")),
    ));
    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail }) if detail.contains("explicit recovery workflow")
    ));
    assert_eq!(
        recover_owned_state(&storage.path).expect("explicit recovery should restore state"),
        RecoveryOutcome::Restored { schema_version: 1 }
    );
}

#[test]
fn stale_schema_requires_explicit_upgrade_before_preparation() {
    let storage = TestStorage::new();
    std::fs::create_dir_all(&storage.root).expect("test storage directory should be created");
    std::fs::write(
        &storage.path,
        r#"{"schema_version":0,"entries":{"alpha":4}}"#,
    )
    .expect("legacy document should be written");
    std::fs::write(
        PathBuf::from(format!("{}.lock", storage.path.display())),
        [],
    )
    .expect("legacy transaction lock should be written");

    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path),
        driver.clone(),
        registry(Some("fixture-secret")),
    ));
    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail })
            if detail.contains("run the explicit upgrade workflow")
    ));

    assert_eq!(
        upgrade_owned_state(&storage.path)
            .expect("explicit upgrade should apply the owned migration"),
        UpgradeOutcome::Applied { from: 0, to: 1 }
    );
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path),
            driver.clone(),
            registry(Some("fixture-secret")),
        ))
        .expect("the upgraded schema should prepare");
    let value = driver
        .run(app.invoke::<CounterRead>(
            "caller",
            READ_OPERATION,
            ReadRequest {
                key: "alpha".to_owned(),
            },
        ))
        .expect("read should reach the public Capability")
        .expect("the migrated value should remain visible");
    assert_eq!(value.value, 4);
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}
