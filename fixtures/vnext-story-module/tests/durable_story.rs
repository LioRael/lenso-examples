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
use lenso_capability_story_events::{
    CAPABILITY_ID as EVENTS_ID, DESCRIPTOR_VERSION as EVENTS_VERSION, Events, RECORD_OPERATION,
    RecordError, RecordRequest,
};
use lenso_capability_story_query::{
    CAPABILITY_ID as QUERY_ID, DESCRIPTOR_VERSION as QUERY_VERSION, Query, TIMELINE_OPERATION,
    TimelineError, TimelineRequest,
};
use lenso_kernel::{DeterministicDriver, Kernel, RuntimeFailure, ShutdownOutcome};
use lenso_native_adapter::NativeModuleRegistry;
use lenso_vnext_story_module::{
    STORY_PACKAGE_ID, StoryFactory, StoryRecoveryOutcome, StorySetupOutcome, StoryStorageError,
    recover_owned_story, setup_owned_story,
};

mod support;

use support::{
    DENIED_PACKAGE_ID, NoopFactory, PRODUCER_PACKAGE_ID, ProducerFactory, READER_PACKAGE_ID,
};

static NEXT_STORAGE_ID: AtomicUsize = AtomicUsize::new(0);

struct TestStorage {
    root: PathBuf,
    path: PathBuf,
}

impl TestStorage {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lenso-story-{}-{}",
            std::process::id(),
            NEXT_STORAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            path: root.join("story.json"),
            root,
        }
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn plan(storage_path: &Path, include_story: bool) -> ResolvedAppPlan {
    plan_with_events(storage_path, include_story, 100, 1_000, Vec::new())
}

fn plan_with_events(
    storage_path: &Path,
    include_story: bool,
    retention_limit: usize,
    idempotency_limit: usize,
    events: Vec<RecordRequest>,
) -> ResolvedAppPlan {
    let producer = ModuleInstancePlan::new("producer", PRODUCER_PACKAGE_ID)
        .with_configuration(serde_json::json!({"events": events}).to_string())
        .with_requirement(CapabilityRequirementPlan::many(EVENTS_ID, EVENTS_VERSION));
    if !include_story {
        return AppComposition::new(vec![producer], Vec::new())
            .resolve()
            .expect("a producer without Story should resolve");
    }

    let story = ModuleInstancePlan::new("story", STORY_PACKAGE_ID)
        .with_configuration(
            serde_json::json!({
                "storage_path": storage_path,
                "authorized_callers": ["reader"],
                "retention_limit": retention_limit,
                "idempotency_limit": idempotency_limit
            })
            .to_string(),
        )
        .with_capability(
            CapabilityEndpointPlan::new(EVENTS_ID, EVENTS_VERSION, [RECORD_OPERATION])
                .with_limits(16, 1),
        )
        .with_capability(
            CapabilityEndpointPlan::new(QUERY_ID, QUERY_VERSION, [TIMELINE_OPERATION])
                .with_limits(16, 1),
        );
    let reader = ModuleInstancePlan::new("reader", READER_PACKAGE_ID)
        .with_requirement(CapabilityRequirementPlan::one(QUERY_ID, QUERY_VERSION));
    let denied = ModuleInstancePlan::new("denied", DENIED_PACKAGE_ID)
        .with_requirement(CapabilityRequirementPlan::one(QUERY_ID, QUERY_VERSION));
    AppComposition::new(
        vec![producer, reader, denied, story],
        vec![
            CapabilityBinding::new("producer", EVENTS_ID, EVENTS_VERSION, "story"),
            CapabilityBinding::new("reader", QUERY_ID, QUERY_VERSION, "story"),
            CapabilityBinding::new("denied", QUERY_ID, QUERY_VERSION, "story"),
        ],
    )
    .resolve()
    .expect("Story Composition should resolve")
}

fn registry() -> NativeModuleRegistry {
    NativeModuleRegistry::new()
        .with_factory(ProducerFactory)
        .with_factory(NoopFactory::new(READER_PACKAGE_ID))
        .with_factory(NoopFactory::new(DENIED_PACKAGE_ID))
        .with_factory(StoryFactory)
}

fn event(event_id: &str, subject_id: &str, event_type: &str) -> RecordRequest {
    event_at(event_id, subject_id, event_type, "2026-08-21T00:00:00Z")
}

fn event_at(
    event_id: &str,
    subject_id: &str,
    event_type: &str,
    occurred_at: &str,
) -> RecordRequest {
    RecordRequest {
        event_id: event_id.to_owned(),
        event_version: 1,
        occurred_at: occurred_at.to_owned(),
        subject_id: subject_id.to_owned(),
        event_type: event_type.to_owned(),
        facts: BTreeMap::from([(String::from("amount"), serde_json::json!(7))]),
    }
}

fn record(
    driver: &DeterministicDriver,
    app: &lenso_kernel::NativeApp,
    event: RecordRequest,
) -> Result<(), RecordError> {
    driver
        .run(app.invoke::<Events>("producer", RECORD_OPERATION, event))
        .expect("durable Story recording should reach the Module")
}

fn query(
    driver: &DeterministicDriver,
    app: &lenso_kernel::NativeApp,
    caller: &str,
    subject_id: &str,
) -> Result<lenso_capability_story_query::TimelineResponse, TimelineError> {
    driver
        .run(app.invoke::<Query>(
            caller,
            TIMELINE_OPERATION,
            TimelineRequest {
                subject_id: subject_id.to_owned(),
                limit: 10,
            },
        ))
        .expect("the Story query should reach the Module")
}

#[test]
fn explicit_business_event_is_persisted_and_duplicate_delivery_is_idempotent() {
    let storage = TestStorage::new();
    assert_eq!(
        setup_owned_story(&storage.path).expect("Story setup should be explicit"),
        StorySetupOutcome::Created { schema_version: 1 }
    );
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                100,
                1_000,
                vec![
                    event("event-1", "order-1", "order.created"),
                    event("event-1", "order-1", "order.created"),
                ],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should start");

    let response = query(&driver, &app, "reader", "order-1").expect("query is authorized");
    assert_eq!(response.entries.len(), 1);
    assert_eq!(response.entries[0].event_id, "event-1");
    assert_eq!(response.entries[0].source_instance, "producer");
    assert_eq!(response.entries[0].facts["amount"], serde_json::json!(7));
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn story_survives_restart_without_runtime_diagnostics() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                100,
                1_000,
                vec![event("event-2", "order-2", "order.paid")],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should start without a diagnostics observer");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                100,
                1_000,
                vec![event("event-2", "order-2", "order.paid")],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("the Story Module should recover its owned storage after restart");
    let response = query(&driver, &app, "reader", "order-2").expect("query is authorized");
    assert_eq!(response.entries.len(), 1);
    assert_eq!(response.entries[0].event_type, "order.paid");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn story_retention_is_owned_by_the_story_module() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                2,
                100,
                vec![
                    event("event-1", "order-retained", "order.created"),
                    event("event-2", "order-retained", "order.paid"),
                    event("event-3", "order-retained", "order.shipped"),
                ],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should start with its configured retention");

    let response = query(&driver, &app, "reader", "order-retained").expect("query is authorized");
    let event_ids: Vec<_> = response
        .entries
        .iter()
        .map(|entry| entry.event_id.as_str())
        .collect();
    assert_eq!(event_ids, ["event-2", "event-3"]);
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn retention_orders_rfc3339_timestamps_by_instant() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                2,
                100,
                vec![
                    event_at(
                        "event-oldest",
                        "order-time",
                        "order.created",
                        "2026-08-21T00:30:00+01:00",
                    ),
                    event_at(
                        "event-middle",
                        "order-time",
                        "order.paid",
                        "2026-08-20T23:45:00Z",
                    ),
                    event_at(
                        "event-newest",
                        "order-time",
                        "order.shipped",
                        "2026-08-21T00:00:00Z",
                    ),
                ],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should normalize and order Event timestamps");

    let response = query(&driver, &app, "reader", "order-time").expect("query is authorized");
    let event_ids: Vec<_> = response
        .entries
        .iter()
        .map(|entry| entry.event_id.as_str())
        .collect();
    assert_eq!(event_ids, ["event-middle", "event-newest"]);
}

#[test]
fn idempotency_retention_bounds_the_identity_index() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                2,
                2,
                vec![
                    event("event-1", "order-bounded", "order.created"),
                    event("event-2", "order-bounded", "order.paid"),
                    event("event-3", "order-bounded", "order.shipped"),
                ],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should enforce bounded idempotency retention");

    let document: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&storage.path).expect("Story storage should remain readable"),
    )
    .expect("Story storage should remain valid JSON");
    let event_ids = document["event_ids"]
        .as_object()
        .expect("the idempotency index should be an object");
    assert_eq!(event_ids.len(), 2);
    assert!(!event_ids.contains_key("event-1"));
    assert_eq!(
        document["event_id_order"],
        serde_json::json!(["event-2", "event-3"])
    );
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn rejected_event_does_not_disable_existing_timelines() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                100,
                1_000,
                vec![event("event-1", "order-stable", "order.created")],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should start");
    let mut conflict = event("event-1", "order-stable", "order.created");
    conflict
        .facts
        .insert(String::from("amount"), serde_json::json!(8));
    assert_eq!(
        record(&driver, &app, conflict),
        Err(RecordError::ConflictingEventId)
    );

    let response = query(&driver, &app, "reader", "order-stable")
        .expect("a rejected Event must not poison durable reads");
    assert_eq!(response.entries.len(), 1);
    assert_eq!(response.entries[0].facts["amount"], serde_json::json!(7));
}

#[test]
fn story_schema_upgrade_is_explicit_and_owned() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    std::fs::write(&storage.path, r#"{"schema_version":0,"entries":[]}"#)
        .expect("the test should install a legacy Story document");

    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path, true),
        driver.clone(),
        registry(),
    ));
    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail }) if detail.contains("explicit upgrade workflow")
    ));
    assert_eq!(
        lenso_vnext_story_module::upgrade_owned_story(&storage.path)
            .expect("the Story owner should apply its migration"),
        lenso_vnext_story_module::StoryUpgradeOutcome::Applied { from: 0, to: 1 }
    );

    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan(&storage.path, true),
            driver.clone(),
            registry(),
        ))
        .expect("the upgraded Story storage should be bootable");
    assert!(
        query(&driver, &app, "reader", "order-upgraded")
            .expect("query is authorized")
            .entries
            .is_empty()
    );
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn query_authorization_is_owned_by_story_module() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let driver = DeterministicDriver::new();
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                true,
                100,
                1_000,
                vec![event("event-3", "order-3", "order.shipped")],
            ),
            driver.clone(),
            registry(),
        ))
        .expect("Story App should start");

    assert_eq!(
        query(&driver, &app, "denied", "order-3"),
        Err(TimelineError::Unauthorized)
    );
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn interrupted_story_commit_requires_explicit_recovery() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let temporary_path = PathBuf::from(format!("{}.tmp", storage.path.display()));
    std::fs::copy(&storage.path, &temporary_path).expect("an interrupted commit should be staged");
    std::fs::remove_file(&storage.path).expect("the committed document should be absent");

    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        plan(&storage.path, true),
        driver.clone(),
        registry(),
    ));
    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail }) if detail.contains("explicit recovery workflow")
    ));
    assert_eq!(
        recover_owned_story(&storage.path).expect("recovery should be an explicit owner workflow"),
        StoryRecoveryOutcome::Restored { schema_version: 1 }
    );
}

#[test]
fn recovery_does_not_discard_a_temporary_document_when_stable_storage_is_invalid() {
    let storage = TestStorage::new();
    setup_owned_story(&storage.path).expect("Story setup should create owned storage");
    let temporary_path = PathBuf::from(format!("{}.tmp", storage.path.display()));
    std::fs::copy(&storage.path, &temporary_path).expect("an interrupted commit should be staged");
    std::fs::write(
        &storage.path,
        r#"{"schema_version":1,"revision":0,"entries":"corrupt","event_ids":{}}"#,
    )
    .expect("the test should install a malformed stable document");

    assert!(matches!(
        recover_owned_story(&storage.path),
        Err(StoryStorageError::InvalidDocument { .. })
    ));
    assert!(temporary_path.exists());
}

#[test]
fn removing_story_leaves_the_kernel_and_producer_composition_valid() {
    let storage = TestStorage::new();
    let driver = DeterministicDriver::new();
    let registry = NativeModuleRegistry::new().with_factory(ProducerFactory);
    let app = driver
        .run(Kernel::start_native(
            plan_with_events(
                &storage.path,
                false,
                100,
                1_000,
                vec![event("event-4", "order-4", "order.created")],
            ),
            driver.clone(),
            registry,
        ))
        .expect("an App without Story should not require Story infrastructure");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}
