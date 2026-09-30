use crate::{
    audit_proof::{self, Expected, PRODUCER},
    unavailable,
};
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    PluginInstancePlan,
};
use lenso_audit_log_d1_plugin as owner;
use lenso_capability_audit_log as audit;
use lenso_kernel::{Kernel, RuntimeFailure, ShutdownOutcome};
use lenso_native_adapter::{
    NativeFacilities, NativeInstanceFacilities, NativePluginFactory, NativePluginFactoryContext,
    NativePluginInstance, NativePluginRegistry,
};
use lenso_workers_driver::WorkersDriver;
use serde_json::json;
use std::time::Duration;
use wasm_bindgen::prelude::*;

const READER: &str = "example.ops-workers-audit-reader/default";
const READER_PACKAGE: &str = "example.ops-workers-audit-reader";
const AUDIT: &str = "lenso.audit-log.d1/default";

#[derive(Debug)]
struct Reader;
impl NativePluginFactory for Reader {
    fn package_id(&self) -> &'static str {
        READER_PACKAGE
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::default())
    }
}
struct Event(WorkersDriver);
impl Drop for Event {
    fn drop(&mut self) {
        self.0.request_shutdown();
    }
}

#[wasm_bindgen]
pub async fn inspect_audit(store: JsValue, expected: String) -> Result<String, JsValue> {
    let expected: Expected = serde_json::from_str(&expected).map_err(|_| unavailable())?;
    if !crate::label(&expected.deployment) || !crate::label(&expected.operation_id) {
        return Err(unavailable());
    }
    owner::link_plugin();
    let configuration = json!({"writer_instances":[PRODUCER],"reader_instances":[READER],
        "reader_scopes":{READER:[{"kind":"deployment","id":expected.deployment}]}});
    let plan = AppComposition::new(
        vec![
            PluginInstancePlan::new(READER, READER_PACKAGE).with_requirement(
                CapabilityRequirementPlan::one(audit::CAPABILITY_ID, audit::DESCRIPTOR_VERSION),
            ),
            PluginInstancePlan::new(AUDIT, owner::PACKAGE_ID)
                .with_configuration(configuration.to_string())
                .with_capability(CapabilityEndpointPlan::new(
                    audit::CAPABILITY_ID,
                    audit::DESCRIPTOR_VERSION,
                    [
                        audit::APPEND_EVENT_OPERATION,
                        audit::GET_EVENT_OPERATION,
                        audit::LIST_EVENTS_OPERATION,
                    ],
                )),
        ],
        vec![CapabilityBinding::new(
            READER,
            audit::CAPABILITY_ID,
            audit::DESCRIPTOR_VERSION,
            AUDIT,
        )],
    )
    .resolve()
    .map_err(|_| unavailable())?;
    let facilities = NativeInstanceFacilities::new()
        .with(
            AUDIT,
            NativeFacilities::new()
                .with(
                    "store",
                    owner::host_facilities::store(&store).map_err(|_| unavailable())?,
                )
                .map_err(|_| unavailable())?,
        )
        .map_err(|_| unavailable())?;
    let driver = WorkersDriver::new();
    let _event = Event(driver.clone());
    let app = Kernel::start_native(
        plan,
        driver,
        NativePluginRegistry::new()
            .with_linked_factories()
            .with_factory(Reader)
            .with_facilities(facilities),
    )
    .await
    .map_err(|_| unavailable())?;
    let observed = app
        .invoke::<audit::AuditLogListEvents>(
            READER,
            audit::LIST_EVENTS_OPERATION,
            audit::ListEventsRequest {
                actor_id: None,
                actor_kind: None,
                correlation_id: Some(expected.operation_id.clone()),
                cursor: None,
                event_name: Some("management-operation".into()),
                limit: 100,
                occurred_after: None,
                occurred_before: None,
                outcome: None,
                resource_id: Some(expected.operation_id.clone()),
                resource_type: Some("management-operation".into()),
                scope_id: Some(expected.deployment.clone()),
                scope_module: None,
                scope_type: Some("deployment".into()),
                severity: None,
                source_instance: Some(PRODUCER.into()),
            },
        )
        .await;
    if app.shutdown(Duration::from_secs(2)).await != ShutdownOutcome::Clean {
        return Err(unavailable());
    }
    let observed = observed
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    if observed.next_cursor.is_some() {
        return Err(unavailable());
    }
    let events = observed
        .events
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unavailable())?;
    let mut proof = audit_proof::verify(&events, &expected).map_err(|()| unavailable())?;
    proof["shutdown"] = json!("clean");
    serde_json::to_string(&proof).map_err(|_| unavailable())
}
