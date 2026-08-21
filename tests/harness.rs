use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use lenso_capability_agent::{Agent, RUN_OPERATION, RunError, RunRequest};
use lenso_capability_agent_progress::UpdateRequest;
use lenso_kernel::{
    CancellationToken, DeterministicDriver, Kernel, RuntimeDriver, RuntimeFailure, ShutdownOutcome,
};
use lenso_vnext_agent_harness::{
    ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, FRIENDLY_MODEL_PACKAGE_ID,
    UPPERCASE_TOOL_PACKAGE_ID, composition, registry, setup_owned_memory,
};

static NEXT_STORAGE_ID: AtomicUsize = AtomicUsize::new(0);

struct TestStorage {
    root: PathBuf,
    path: PathBuf,
}

impl TestStorage {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lenso-agent-harness-{}-{}",
            std::process::id(),
            NEXT_STORAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            path: root.join("memory.json"),
            root,
        }
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn start(
    storage: &TestStorage,
    model_package_id: &'static str,
    tool_package_id: &'static str,
    include_progress: bool,
) -> (
    DeterministicDriver,
    lenso_kernel::NativeApp,
    Rc<RefCell<Vec<UpdateRequest>>>,
) {
    let driver = DeterministicDriver::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    let app = driver
        .run(Kernel::start_native(
            composition(
                &storage.path,
                model_package_id,
                tool_package_id,
                include_progress,
            ),
            driver.clone(),
            registry(events.clone()),
        ))
        .expect("configured agent App should start");
    (driver, app, events)
}

#[test]
fn harness_streams_model_output_invokes_tool_and_reports_progress() {
    let storage = TestStorage::new();
    assert!(matches!(
        setup_owned_memory(&storage.path).expect("memory setup should succeed"),
        lenso_vnext_agent_harness::MemorySetupOutcome::Created { schema_version: 1 }
    ));
    let (driver, app, events) = start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, true);

    let response = driver
        .run(app.invoke::<Agent>(
            "caller",
            RUN_OPERATION,
            RunRequest {
                prompt: "find a document".to_owned(),
                run_id: "run-1".to_owned(),
            },
        ))
        .expect("the harness request should reach its endpoint")
        .expect("the harness should complete");
    driver.run(driver.yield_now());

    assert!(response.text.contains("echo model:"));
    assert!(response.text.contains("echo tool: find a document"));
    assert_eq!(response.revision, "1");
    let event_log = events.borrow();
    let stages = event_log
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    assert_eq!(stages.first(), Some(&"started"));
    assert!(stages.contains(&"memory"));
    assert!(stages.contains(&"tool"));
    assert!(stages.contains(&"model"));
    assert_eq!(stages.last(), Some(&"completed"));
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn model_and_tool_providers_are_selected_only_by_composition() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    let (driver, app, _events) = start(
        &storage,
        FRIENDLY_MODEL_PACKAGE_ID,
        UPPERCASE_TOOL_PACKAGE_ID,
        false,
    );

    let response = driver
        .run(app.invoke::<Agent>(
            "caller",
            RUN_OPERATION,
            RunRequest {
                prompt: "replaceable provider".to_owned(),
                run_id: "run-swap".to_owned(),
            },
        ))
        .expect("the swapped Composition should invoke")
        .expect("the swapped providers should satisfy the same contract");

    assert!(response.text.contains("friendly model:"));
    assert!(
        response
            .text
            .contains("uppercase tool: REPLACEABLE PROVIDER")
    );
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn durable_memory_survives_restart_and_progress_is_optional() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    let (driver, app, events) = start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, false);
    let first = driver
        .run(app.invoke::<Agent>(
            "caller",
            RUN_OPERATION,
            RunRequest {
                prompt: "remember this".to_owned(),
                run_id: "run-first".to_owned(),
            },
        ))
        .expect("first run should reach the harness")
        .expect("first run should complete");
    assert_eq!(first.revision, "1");
    assert!(events.borrow().is_empty());
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));

    let (driver, app, _events) =
        start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, false);
    let second = driver
        .run(app.invoke::<Agent>(
            "caller",
            RUN_OPERATION,
            RunRequest {
                prompt: "use memory".to_owned(),
                run_id: "run-second".to_owned(),
            },
        ))
        .expect("the restarted harness should reach the public Capability")
        .expect("the restarted harness should complete");
    assert!(second.text.contains("1 prior memories"));
    assert_eq!(second.revision, "2");
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn missing_memory_fails_startup_without_an_in_memory_fallback() {
    let storage = TestStorage::new();
    let driver = DeterministicDriver::new();
    let result = driver.run(Kernel::start_native(
        composition(
            &storage.path,
            ECHO_MODEL_PACKAGE_ID,
            ECHO_TOOL_PACKAGE_ID,
            false,
        ),
        driver.clone(),
        registry(Rc::new(RefCell::new(Vec::new()))),
    ));

    assert!(matches!(
        result,
        Err(RuntimeFailure::Internal { detail })
            if detail.contains("required durable agent memory")
                && detail.contains("setup_owned_memory")
    ));
    assert!(!storage.path.exists());
}

#[test]
fn oversized_memory_fails_with_resource_exhaustion() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    std::fs::write(&storage.path, vec![b' '; 4 * 1024 * 1024 + 1])
        .expect("oversized memory fixture should be written");
    let driver = DeterministicDriver::new();

    let result = driver.run(Kernel::start_native(
        composition(
            &storage.path,
            ECHO_MODEL_PACKAGE_ID,
            ECHO_TOOL_PACKAGE_ID,
            false,
        ),
        driver.clone(),
        registry(Rc::new(RefCell::new(Vec::new()))),
    ));

    assert!(matches!(
        result,
        Err(RuntimeFailure::ResourceExhausted {
            capability,
            operation,
        }) if capability == lenso_capability_agent_memory::CAPABILITY_ID
            && operation == "storage"
    ));
}

#[test]
fn domain_tool_failure_stays_distinct_from_runtime_failure() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    let (driver, app, _events) =
        start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, false);

    let domain = driver.run(app.invoke::<Agent>(
        "caller",
        RUN_OPERATION,
        RunRequest {
            prompt: "tool-domain-error".to_owned(),
            run_id: "run-domain".to_owned(),
        },
    ));
    assert!(matches!(domain, Ok(Err(RunError::ToolRejected))));

    let runtime = driver.run(app.invoke::<Agent>(
        "caller",
        RUN_OPERATION,
        RunRequest {
            prompt: "tool-runtime-failure".to_owned(),
            run_id: "run-runtime".to_owned(),
        },
    ));
    assert!(matches!(
        runtime,
        Err(RuntimeFailure::ModuleFailure { detail })
            if detail.contains("selected tool provider failed")
    ));
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}

#[test]
fn deadline_cancellation_provider_unavailability_and_shutdown_are_runtime_outcomes() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    let (driver, app, _events) =
        start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, false);

    let deadline = app.invocation_context_after(Duration::ZERO, CancellationToken::new());
    assert!(matches!(
        driver.run(app.invoke_with_context::<Agent>(
            "caller",
            RUN_OPERATION,
            deadline,
            RunRequest {
                prompt: "model-delay".to_owned(),
                run_id: "run-deadline".to_owned(),
            },
        )),
        Err(RuntimeFailure::DeadlineExceeded { .. })
    ));

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let context = app.invocation_context(None, cancellation);
    assert!(matches!(
        driver.run(app.invoke_with_context::<Agent>(
            "caller",
            RUN_OPERATION,
            context,
            RunRequest {
                prompt: "cancelled".to_owned(),
                run_id: "run-cancelled".to_owned(),
            },
        )),
        Err(RuntimeFailure::Cancelled { .. })
    ));

    let unavailable = driver.run(app.invoke::<Agent>(
        "caller",
        RUN_OPERATION,
        RunRequest {
            prompt: "model-runtime-failure".to_owned(),
            run_id: "run-unavailable".to_owned(),
        },
    ));
    assert!(matches!(
        unavailable,
        Err(RuntimeFailure::Unavailable { capability })
            if capability == lenso_capability_agent_model::CAPABILITY_ID
    ));

    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
    assert!(matches!(
        driver.run(app.invoke::<Agent>(
            "caller",
            RUN_OPERATION,
            RunRequest {
                prompt: "after shutdown".to_owned(),
                run_id: "run-shutdown".to_owned(),
            },
        )),
        Err(RuntimeFailure::AdmissionClosed)
    ));
}

#[test]
fn model_output_over_limit_cancels_stream_with_resource_exhaustion() {
    let storage = TestStorage::new();
    setup_owned_memory(&storage.path).expect("memory setup should succeed");
    let (driver, app, _events) =
        start(&storage, ECHO_MODEL_PACKAGE_ID, ECHO_TOOL_PACKAGE_ID, false);

    let result = driver.run(app.invoke::<Agent>(
        "caller",
        RUN_OPERATION,
        RunRequest {
            prompt: "model-output-overflow".to_owned(),
            run_id: "run-output-limit".to_owned(),
        },
    ));

    assert!(matches!(
        result,
        Err(RuntimeFailure::ResourceExhausted {
            capability,
            operation,
        }) if capability == lenso_capability_agent_model::CAPABILITY_ID
            && operation == lenso_capability_agent_model::COMPLETE_OPERATION
    ));
    assert!(matches!(
        driver.run(app.shutdown(Duration::from_secs(1))),
        ShutdownOutcome::Clean
    ));
}
