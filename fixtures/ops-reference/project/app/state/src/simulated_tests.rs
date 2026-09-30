use super::*;
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    PluginInstancePlan, ResolvedAppPlan,
};
use lenso_kernel::{RuntimeFailure, ShutdownOutcome};
use lenso_native_adapter::{
    ConfiguredPluginFactory, NativeFacilities, NativeInstanceFacilities, NativePluginDefinition,
    NativePluginFactory, NativePluginFactoryContext, NativePluginInstance, NativePluginRegistry,
};
use lenso_test::{TestApp, TestSimulator};
use std::time::Duration;

#[derive(Debug)]
struct Caller;
impl NativePluginFactory for Caller {
    fn package_id(&self) -> &'static str {
        "test.ops-caller"
    }
    fn runtime_profile(&self) -> &'static str {
        "lenso.native-authoring@2"
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}

#[derive(Clone, Debug, Default)]
struct World {
    primary: Rc<RefCell<MemoryState>>,
    secondary: Rc<RefCell<MemoryState>>,
}
impl World {
    fn start(&self, simulator: TestSimulator) -> TestApp {
        let world = self.clone();
        let factory = ConfiguredPluginFactory::<OpsState, _>::new(move |plugin| {
            plugin.state = match plugin.config.label.as_str() {
                "primary-state" => world.primary.clone(),
                "secondary-state" => world.secondary.clone(),
                _ => {
                    return Err(lenso::RuntimeFailure::PluginFailure {
                        detail: "unknown simulated store".into(),
                    });
                }
            };
            Ok(())
        });
        let mut facilities = NativeInstanceFacilities::new();
        for name in ["primary", "secondary"] {
            facilities = facilities
                .with(
                    name,
                    NativeFacilities::new()
                        .with(
                            "state",
                            host_facilities::state(&serde_json::json!({"profile":"simulated"}))
                                .unwrap(),
                        )
                        .unwrap(),
                )
                .unwrap();
        }
        TestApp::builder(plan())
            .with_simulator(simulator)
            .with_registry(
                NativePluginRegistry::new()
                    .with_factory(Caller)
                    .with_factory(factory)
                    .with_facilities(facilities),
            )
            .start()
            .unwrap()
    }
}

fn plan() -> ResolvedAppPlan {
    let mut caller = PluginInstancePlan::new("caller", "test.ops-caller")
        .with_authoring(2, "lenso.native-authoring@2");
    let mut instances = Vec::new();
    let mut bindings = Vec::new();
    for (name, value) in [("primary", 11), ("secondary", 29)] {
        caller = caller.with_requirement(
            CapabilityRequirementPlan::one(state::CAPABILITY_ID, state::DESCRIPTOR_VERSION)
                .with_requirement_id(name),
        );
        instances.push(PluginInstancePlan::new(name, <OpsState as NativePluginDefinition>::PACKAGE_ID)
            .with_authoring(1, <OpsState as NativePluginDefinition>::RUNTIME_PROFILE)
            .with_configuration(serde_json::json!({"profile":"simulated","label":format!("{name}-state"),"initial_value":value}).to_string())
            .with_capability(CapabilityEndpointPlan::new(state::CAPABILITY_ID, state::DESCRIPTOR_VERSION,
                [state::READ_OPERATION,state::RECEIPT_OPERATION,state::UPDATE_OPERATION])));
        bindings.push(
            CapabilityBinding::new(
                "caller",
                state::CAPABILITY_ID,
                state::DESCRIPTOR_VERSION,
                name,
            )
            .with_requirement_id(name),
        );
    }
    instances.push(caller);
    AppComposition::new(instances, bindings).resolve().unwrap()
}

fn clients(app: &TestApp) -> (state::OpsStateClient, state::OpsStateClient) {
    let dependencies = app.app().dependencies("caller").unwrap();
    (
        state::OpsStateClient::from_requirement(&dependencies, "primary").unwrap(),
        state::OpsStateClient::from_requirement(&dependencies, "secondary").unwrap(),
    )
}

#[test]
fn real_kernel_simulation_preserves_named_state_and_receipts_across_restart() {
    let simulator = TestSimulator::new();
    let world = World::default();
    let app = world.start(simulator.clone());
    let (primary, secondary) = clients(&app);
    let command = UpdateRequest {
        value: 47,
        expected_revision: 0,
        idempotency_key: "simulation-write".into(),
    };
    let receipt = app.run(primary.update(command.clone())).unwrap();
    assert_eq!(receipt.revision, 1);
    assert_eq!(app.run(primary.update(command.clone())).unwrap(), receipt);
    assert!(matches!(
        app.run(primary.update(UpdateRequest {
            value: 48,
            ..command.clone()
        })),
        Err(state::OpsStateUpdateInvocationError::Domain(
            state::UpdateError::IdempotencyConflict
        ))
    ));
    assert_eq!(app.run(secondary.read(ReadRequest {})).unwrap().value, 29);
    simulator.advance(Duration::from_secs(5));
    assert_eq!(app.shutdown(Duration::from_secs(1)), ShutdownOutcome::Clean);

    let restarted = world.start(simulator);
    let (current, secondary) = clients(&restarted);
    assert_eq!(restarted.run(current.update(command)).unwrap(), receipt);
    let query = restarted
        .run(current.receipt(ReceiptRequest {
            idempotency_key: "simulation-write".into(),
        }))
        .unwrap();
    assert!(query.found);
    assert_eq!(query.revision, Some(Some(1)));
    assert_eq!(
        restarted
            .run(current.read(ReadRequest {}))
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        restarted
            .run(secondary.read(ReadRequest {}))
            .unwrap()
            .revision,
        0
    );
    assert!(
        restarted.run(primary.read(ReadRequest {})).is_err(),
        "retired handle must not enter the new generation"
    );
    assert_eq!(
        restarted.shutdown(Duration::from_secs(1)),
        ShutdownOutcome::Clean
    );
}

#[test]
fn real_kernel_simulation_commits_one_contender_and_rejects_invalid_input() {
    let app = World::default().start(TestSimulator::new());
    let (primary, _) = clients(&app);
    let (left, right) = app.run(async {
        futures::join!(
            primary.update(UpdateRequest {
                value: 71,
                expected_revision: 0,
                idempotency_key: "left".into()
            }),
            primary.update(UpdateRequest {
                value: 83,
                expected_revision: 0,
                idempotency_key: "right".into()
            })
        )
    });
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(
        matches!(
            left,
            Err(state::OpsStateUpdateInvocationError::Domain(
                state::UpdateError::StaleRevision
            ))
        ) || matches!(
            right,
            Err(state::OpsStateUpdateInvocationError::Domain(
                state::UpdateError::StaleRevision
            ))
        )
    );
    assert_eq!(app.run(primary.read(ReadRequest {})).unwrap().revision, 1);
    assert!(matches!(
        app.run(primary.update(UpdateRequest {
            value: i64::MAX,
            expected_revision: 1,
            idempotency_key: "invalid".into()
        })),
        Err(state::OpsStateUpdateInvocationError::Domain(
            state::UpdateError::InvalidInput
        ))
    ));
    assert_eq!(app.shutdown(Duration::from_secs(1)), ShutdownOutcome::Clean);
}
