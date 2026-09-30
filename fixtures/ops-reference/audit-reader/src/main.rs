//! Supplemental owner-port inspection of existing facts, without setup or append.
use lenso_app_plan::{
    AppComposition, CapabilityBinding, CapabilityEndpointPlan, CapabilityRequirementPlan,
    PluginInstancePlan,
};
use lenso_audit_log_postgres_plugin::{AuditLogConfig, AuditReadScope, PACKAGE_ID};
use lenso_capability_audit_log as audit;
use lenso_capability_secrets as secrets;
use lenso_kernel::{
    ActivateContext, InvocationContext, Kernel, NativeRequestEndpoint, NativeRequestFuture,
    PluginFuture, PluginLifecycle, RuntimeFailure, ShutdownOutcome,
};
use lenso_native_adapter::{
    NativePluginFactory, NativePluginFactoryContext, NativePluginInstance, NativePluginRegistry,
};
use lenso_ops_reference_pg::{PgBinding, PgStateHandle};
use lenso_runner::TokioDriver;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell, collections::BTreeSet, error::Error, path::PathBuf, rc::Rc, time::Duration,
};

const READER_PACKAGE: &str = "example.ops-audit-reader";
const READER: &str = "example.ops-audit-reader/default";
const SECRET_PACKAGE: &str = "example.ops-audit-reader-secrets";
const DATABASE_REFERENCE: &str = "audit/database";
const PRODUCER: &str = "example.ops-management/default";
type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Events = Vec<audit::ListEventsResponseEventsItem>;

#[derive(Deserialize)]
struct OwnerInput {
    audit_uri_file: PathBuf,
    database_uri_file: PathBuf,
    deployment: String,
    schemas: OwnerSchemas,
}

#[derive(Deserialize)]
struct OwnerSchemas {
    auth: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    operation_id: Option<String>,
    intent_digest: Option<String>,
    receipt_id: Option<String>,
    requester_subject: String,
    decider_subject: String,
    value: i64,
    revision: u64,
}

struct DatabaseSecret(String);
impl std::fmt::Debug for DatabaseSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseSecret(<redacted>)")
    }
}
#[derive(Clone, Debug)]
struct SecretFactory(Rc<DatabaseSecret>);
impl NativePluginFactory for SecretFactory {
    fn package_id(&self) -> &'static str {
        SECRET_PACKAGE
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> std::result::Result<NativePluginInstance, RuntimeFailure> {
        let endpoint =
            Rc::new(secrets::SecretsEndpoint::new(self.clone())) as Rc<dyn NativeRequestEndpoint>;
        Ok(NativePluginInstance::new(vec![endpoint]))
    }
}
impl secrets::SecretsProvider for SecretFactory {
    fn resolve(
        &self,
        _: InvocationContext,
        request: secrets::ResolveRequest,
    ) -> NativeRequestFuture<secrets::Secrets> {
        let value = if request.reference == DATABASE_REFERENCE {
            Ok(secrets::ResolveResponse {
                value: self.0.0.clone(),
            })
        } else {
            Err(secrets::ResolveError::UnknownReference)
        };
        Box::pin(async move { Ok(value) })
    }
}

#[derive(Clone)]
struct Reader {
    deployment: String,
    operation: Option<String>,
    events: Rc<RefCell<Option<Events>>>,
}
impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SupplementalAuditReader")
    }
}
impl NativePluginFactory for Reader {
    fn package_id(&self) -> &'static str {
        READER_PACKAGE
    }
    fn instantiate(
        &self,
        _: NativePluginFactoryContext<'_>,
    ) -> std::result::Result<NativePluginInstance, RuntimeFailure> {
        Ok(NativePluginInstance::with_lifecycle(
            Vec::new(),
            self.clone(),
        ))
    }
}
impl PluginLifecycle for Reader {
    fn activate(&self, context: ActivateContext) -> PluginFuture {
        let client = audit::AuditLogClient::from_dependencies(context.dependencies());
        let this = self.clone();
        Box::pin(async move {
            let client = client?;
            let mut events = Vec::new();
            let mut cursor = None;
            for _ in 0..10 {
                let response = client
                    .list_events(audit::ListEventsRequest {
                        actor_id: None,
                        actor_kind: None,
                        correlation_id: this.operation.clone(),
                        cursor,
                        event_name: Some("management-operation".into()),
                        limit: 100,
                        occurred_after: None,
                        occurred_before: None,
                        outcome: None,
                        resource_id: this.operation.clone(),
                        resource_type: Some("management-operation".into()),
                        scope_id: Some(this.deployment.clone()),
                        scope_module: None,
                        scope_type: Some("deployment".into()),
                        severity: None,
                        source_instance: None,
                    })
                    .await
                    .map_err(|_| RuntimeFailure::Unavailable {
                        capability: audit::CAPABILITY_ID,
                    })?;
                events.extend(response.events);
                match response.next_cursor {
                    Some(next) => {
                        cursor = Some(audit::ListEventsRequestCursor {
                            id: next.id,
                            occurred_at: next.occurred_at,
                        });
                    }
                    None => {
                        *this.events.borrow_mut() = Some(events);
                        return Ok(());
                    }
                }
            }
            Err(RuntimeFailure::Unavailable {
                capability: audit::CAPABILITY_ID,
            })
        })
    }
}

fn receipt(event: &audit::ListEventsResponseEventsItem) -> Option<&str> {
    event
        .metadata
        .get("receipt")?
        .as_str()
        .filter(|value| !value.is_empty())
}

async fn correlate_business_receipt(
    events: &Events,
    expected: &Expected,
    input: &OwnerInput,
) -> Result<Expected> {
    let owner = PgStateHandle::open(PgBinding {
        connection_uri_file: input.database_uri_file.clone(),
        schema: format!("{}_primary", input.schemas.auth),
    })
    .map_err(|_| "business Owner binding unavailable")?;
    let mut matches = Vec::new();
    for event in events.iter().filter(|event| {
        event.action == "management.completed"
            && event.metadata.get("entry_id") == Some(&json!("state.update"))
            && event.metadata.get("state") == Some(&json!("succeeded"))
            && expected
                .operation_id
                .as_deref()
                .is_none_or(|id| event.correlation_id.as_deref() == Some(id))
    }) {
        let operation = event
            .correlation_id
            .as_deref()
            .ok_or("missing audit operation")?;
        let actual = owner
            .receipt(operation)
            .await
            .map_err(|_| "business Owner receipt unavailable")?
            .ok_or("completed audit operation has no business Owner receipt")?;
        if receipt(event) != Some(actual.receipt_id.as_str()) {
            return Err("audit and business Owner receipt mismatch".into());
        }
        if actual.snapshot.value == expected.value
            && u64::try_from(actual.snapshot.revision).ok() == Some(expected.revision)
        {
            if expected
                .receipt_id
                .as_deref()
                .is_some_and(|id| id != actual.receipt_id)
            {
                return Err("expected response and business Owner receipt mismatch".into());
            }
            matches.push((operation.to_owned(), actual.receipt_id));
        }
    }
    if matches.len() != 1 {
        return Err("expected unique actual business Owner receipt".into());
    }
    let mut correlated = expected.clone();
    correlated.operation_id = Some(matches[0].0.clone());
    correlated.receipt_id = Some(matches[0].1.clone());
    Ok(correlated)
}

fn verify(
    events: &[audit::ListEventsResponseEventsItem],
    expected: &Expected,
    deployment: &str,
) -> Result<Value> {
    if expected.requester_subject.is_empty()
        || expected.decider_subject.is_empty()
        || expected.requester_subject == expected.decider_subject
    {
        return Err("distinct expected humans required".into());
    }
    let candidates: Vec<_> = events
        .iter()
        .filter(|event| {
            event.action == "management.completed"
                && event.metadata.get("entry_id") == Some(&json!("state.update"))
                && event.metadata.get("state") == Some(&json!("succeeded"))
                && expected
                    .operation_id
                    .as_deref()
                    .is_none_or(|id| event.correlation_id.as_deref() == Some(id))
                && receipt(event).is_some_and(|value| {
                    expected.receipt_id.as_deref().is_none_or(|id| id == value)
                })
        })
        .collect();
    if candidates.len() != 1 {
        return Err("expected unique persisted completed write".into());
    }
    let operation = candidates[0]
        .correlation_id
        .as_deref()
        .ok_or("missing operation correlation")?;
    if expected
        .operation_id
        .as_deref()
        .is_some_and(|id| id != operation)
    {
        return Err("operation mismatch".into());
    }
    let selected: Vec<_> = events
        .iter()
        .filter(|event| event.correlation_id.as_deref() == Some(operation))
        .collect();
    let phases = [
        "attempt",
        "human_attempt",
        "human_decided",
        "dispatch",
        "completed",
    ];
    let approved: Vec<_> = selected
        .iter()
        .filter(|event| {
            !event.action.starts_with("management.human_")
                || event.actor_id.as_ref() == Some(&expected.decider_subject)
        })
        .collect();
    let actions: BTreeSet<_> = approved.iter().map(|event| event.action.as_str()).collect();
    let required: BTreeSet<_> = phases
        .iter()
        .map(|phase| format!("management.{phase}"))
        .collect();
    if approved.len() != phases.len() || actions != required.iter().map(String::as_str).collect() {
        return Err("expected five unique persisted audit phases".into());
    }
    let mut requester_attempts = BTreeSet::new();
    let mut digest = None;
    for event in &selected {
        if event.source_instance != PRODUCER
            || event.event_name != "management-operation"
            || event.resource_type.as_deref() != Some("management-operation")
            || event.resource_id.as_deref() != Some(operation)
            || event.request_id.as_deref() != Some(operation)
            || event.scope_type.as_deref() != Some("deployment")
            || event.scope_id.as_deref() != Some(deployment)
            || event.actor_kind != "user"
            || event.metadata.get("entry_id") != Some(&json!("state.update"))
            || event.outcome != audit::ListEventsResponseEventsItemOutcome::Success
        {
            return Err("persisted source, references, actor kind or outcome mismatch".into());
        }
        let human = event.action.starts_with("management.human_");
        let approved_human = human && event.actor_id.as_ref() == Some(&expected.decider_subject);
        let actor = if approved_human {
            &expected.decider_subject
        } else {
            &expected.requester_subject
        };
        if event.actor_id.as_ref() != Some(actor) {
            return Err("persisted human attribution mismatch".into());
        }
        if human
            && !approved_human
            && (event.action != "management.human_attempt"
                || !requester_attempts.insert(event.id.as_str()))
        {
            return Err("persisted human attribution mismatch".into());
        }
        let intent = event
            .metadata
            .get("intent_digest")
            .and_then(Value::as_str)
            .ok_or("missing persisted intent digest")?;
        if intent.len() != 64 || !intent.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid persisted intent digest".into());
        }
        if digest.is_some_and(|prior| prior != intent)
            || expected
                .intent_digest
                .as_deref()
                .is_some_and(|value| value != intent)
        {
            return Err("persisted intent changed across phases".into());
        }
        digest = Some(intent);
        let state = match event.action.as_str() {
            "management.dispatch" => "executing",
            "management.completed" => "succeeded",
            _ => "pending_approval",
        };
        if event.metadata.get("state") != Some(&json!(state))
            || event.metadata.get("decision")
                != Some(&if human {
                    json!("approved")
                } else {
                    Value::Null
                })
        {
            return Err("persisted phase state or decision mismatch".into());
        }
        if event.action == "management.completed" {
            let actual = receipt(event).ok_or("missing persisted completed receipt")?;
            if expected
                .receipt_id
                .as_deref()
                .is_some_and(|id| id != actual)
            {
                return Err("persisted receipt differs from actual response".into());
            }
        } else if event.metadata.get("receipt") != Some(&Value::Null) {
            return Err("nonterminal phase contains a receipt".into());
        }
    }
    Ok(json!({
        "schema":"lenso.ops-owner-audit-proof.v1", "status":"passed",
        "proof_layer":"supplemental_owner_kernel_reader", "deployment":deployment,
        "operation_id":operation, "source_instance":PRODUCER,
        "event_count":selected.len(), "actions":required,
        "additional_requester_attempts_verified":requester_attempts.len(),
        "lookup":if expected.operation_id.is_some() { "expected_operation_id" } else { "unique_completed_write_in_deployment" },
        "requester_and_decider_verified":true, "actors_distinct":true,
        "intent_digest_consistent":true, "domain_value_revision_verified":false,
        "exact_response_receipt_verified":expected.receipt_id.is_some(),
        "metadata_and_actor_identifiers_redacted":true, "append_invoked":false,
        "audit_owner_source":"59ecc79e38de72891e6a164d7e74617c618038f8"
    }))
}

async fn run() -> Result<Value> {
    let mut args = std::env::args().skip(1);
    let input: OwnerInput =
        serde_json::from_slice(&std::fs::read(args.next().ok_or("input file required")?)?)?;
    let expected: Expected = serde_json::from_slice(&std::fs::read(
        args.next().ok_or("expectations file required")?,
    )?)?;
    if args.next().is_some()
        || !input.audit_uri_file.is_absolute()
        || !input.database_uri_file.is_absolute()
        || input.deployment.is_empty()
    {
        return Err("invalid inspection arguments".into());
    }
    let database = std::fs::read_to_string(&input.audit_uri_file)?;
    let config = AuditLogConfig::new(
        DATABASE_REFERENCE,
        vec![PRODUCER.into()],
        vec![READER.into()],
    )?
    .with_reader_scopes(
        READER,
        vec![AuditReadScope {
            kind: "deployment".into(),
            id: input.deployment.clone(),
        }],
    )?;
    let plan = AppComposition::new(
        vec![
            PluginInstancePlan::new("audit", PACKAGE_ID)
                .with_configuration(serde_json::to_string(&config)?)
                .with_requirement(CapabilityRequirementPlan::one(
                    secrets::CAPABILITY_ID,
                    secrets::DESCRIPTOR_VERSION,
                ))
                .with_capability(CapabilityEndpointPlan::new(
                    audit::CAPABILITY_ID,
                    audit::DESCRIPTOR_VERSION,
                    [
                        audit::APPEND_EVENT_OPERATION,
                        audit::GET_EVENT_OPERATION,
                        audit::LIST_EVENTS_OPERATION,
                    ],
                )),
            PluginInstancePlan::new("secret", SECRET_PACKAGE).with_capability(
                CapabilityEndpointPlan::new(
                    secrets::CAPABILITY_ID,
                    secrets::DESCRIPTOR_VERSION,
                    [secrets::RESOLVE_OPERATION],
                ),
            ),
            PluginInstancePlan::new(READER, READER_PACKAGE).with_requirement(
                CapabilityRequirementPlan::one(audit::CAPABILITY_ID, audit::DESCRIPTOR_VERSION),
            ),
        ],
        vec![
            CapabilityBinding::new(
                "audit",
                secrets::CAPABILITY_ID,
                secrets::DESCRIPTOR_VERSION,
                "secret",
            ),
            CapabilityBinding::new(
                READER,
                audit::CAPABILITY_ID,
                audit::DESCRIPTOR_VERSION,
                "audit",
            ),
        ],
    )
    .resolve()?;
    let events = Rc::new(RefCell::new(None));
    let reader = Reader {
        deployment: input.deployment.clone(),
        operation: expected.operation_id.clone(),
        events: events.clone(),
    };
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let app = Kernel::start_native(
                plan,
                TokioDriver::new(),
                NativePluginRegistry::new()
                    .with_linked_factories()
                    .with_factory(SecretFactory(Rc::new(DatabaseSecret(
                        database.trim().into(),
                    ))))
                    .with_factory(reader),
            )
            .await
            .map_err(|_| "supplemental owner reader unavailable")?;
            let observed = events.borrow_mut().take().ok_or("no owner event response");
            let shutdown = app.shutdown(Duration::from_secs(5)).await;
            if shutdown != ShutdownOutcome::Clean {
                return Err("supplemental reader shutdown failed".into());
            }
            let observed = observed?;
            let correlated = correlate_business_receipt(&observed, &expected, &input).await?;
            let mut proof = verify(&observed, &correlated, &input.deployment)?;
            proof["lookup"] = json!("actual_owner_receipt_matched_qualified_business_state");
            proof["domain_value_revision_verified"] = json!(true);
            proof["receipt_matched_actual_business_owner"] = json!(true);
            proof["exact_response_receipt_verified"] = json!(expected.receipt_id.is_some());
            proof["business_owner_api"] = json!("lenso_ops_reference_pg::PgStateHandle::receipt");
            proof["business_owner_source_sha256"] = json!(format!(
                "{:x}",
                Sha256::digest(include_bytes!("../../storage/native-pg/src/lib.rs"))
            ));
            proof["inspection_runtime"] = json!({
                "source":"immutable_registry", "app_plan":"0.4.6", "kernel":"0.3.11",
                "native_adapter":"0.3.18", "runner":"0.2.19"
            });
            proof["real_database"] = json!(true);
            proof["shutdown"] = json!("clean");
            Ok(proof)
        })
        .await
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(proof) => println!("{proof}"),
        Err(error) => {
            eprintln!(
                "Supplemental Audit owner proof failed [{}]",
                failure_stage(error.as_ref())
            );
            std::process::exit(1);
        }
    }
}

fn failure_stage(error: &dyn Error) -> &'static str {
    match error.to_string().as_str() {
        "supplemental owner reader unavailable" => "reader_startup",
        "no owner event response" => "owner_response",
        "supplemental reader shutdown failed" => "reader_shutdown",
        "business Owner binding unavailable" => "business_binding",
        "business Owner receipt unavailable" => "business_receipt_read",
        "completed audit operation has no business Owner receipt" => "business_receipt_absent",
        "audit and business Owner receipt mismatch" => "business_receipt_mismatch",
        "expected unique actual business Owner receipt" => "business_receipt_selection",
        "expected unique persisted completed write" => "audit_write_selection",
        "expected five unique persisted audit phases" => "audit_phase_count",
        "persisted source, references, actor kind or outcome mismatch" => "audit_source",
        "persisted human attribution mismatch" => "audit_actor",
        "persisted phase state or decision mismatch" => "audit_phase_state",
        _ => "other_private_failure",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_do_not_export_unknown_error_details() {
        let error: Box<dyn Error> =
            "postgresql://private-user:private-token@localhost/private".into();
        assert_eq!(failure_stage(error.as_ref()), "other_private_failure");
    }

    #[test]
    fn requester_attempt_is_distinct_from_the_real_decider_and_never_adds_a_dispatch() {
        let (mut events, expected) = sample();
        let mut attempted = events[1].clone();
        attempted["id"] = json!("self-attempt");
        attempted["actor_id"] = json!(expected.requester_subject);
        events.push(attempted);
        let proof = verify(&decode(events.clone()), &expected, "deployment-one").unwrap();
        assert_eq!(proof["event_count"], 6);
        assert_eq!(proof["additional_requester_attempts_verified"], 1);
        let mut duplicate_dispatch = events[3].clone();
        duplicate_dispatch["id"] = json!("duplicate-dispatch");
        events.push(duplicate_dispatch);
        assert!(verify(&decode(events), &expected, "deployment-one").is_err());
    }

    fn sample() -> (Vec<Value>, Expected) {
        let expected = Expected {
            operation_id: Some("op-one".into()),
            intent_digest: Some("a".repeat(64)),
            receipt_id: Some("primary-state:1:domain-digest".into()),
            requester_subject: "requester".into(),
            decider_subject: "decider".into(),
            value: 47,
            revision: 1,
        };
        let events = ["attempt", "human_attempt", "human_decided", "dispatch", "completed"]
            .into_iter()
            .map(|phase| {
                let human = phase.starts_with("human_");
                json!({
                    "action":format!("management.{phase}"),
                    "actor_display":null,"actor_id":if human { "decider" } else { "requester" },
                    "actor_kind":"user","causation_id":null,"correlation_id":"op-one",
                    "created_at":"2026-09-30T00:00:00Z","event_name":"management-operation",
                    "id":format!("event-{phase}"),
                    "metadata":{
                        "entry_id":"state.update","intent_digest":"a".repeat(64),
                        "state":match phase { "dispatch"=>"executing", "completed"=>"succeeded", _=>"pending_approval" },
                        "decision":if human { json!("approved") } else { Value::Null },
                        "receipt":if phase=="completed" { json!("primary-state:1:domain-digest") } else { Value::Null }
                    },
                    "occurred_at":"2026-09-30T00:00:00Z","outcome":"success","reason":null,
                    "request_id":"op-one","resource_display":null,"resource_id":"op-one",
                    "resource_type":"management-operation","scope_display":null,
                    "scope_id":"deployment-one","scope_module":null,"scope_type":"deployment",
                    "severity":"info","source_instance":PRODUCER,"story_id":null
                })
            })
            .collect();
        (events, expected)
    }

    fn decode(events: Vec<Value>) -> Events {
        serde_json::from_value(json!(events)).expect("valid generated event wire")
    }

    #[test]
    fn complete_exact_wire_is_classified_without_exporting_actor_or_metadata() {
        let (events, expected) = sample();
        let proof = verify(&decode(events), &expected, "deployment-one").unwrap();
        assert_eq!(proof["event_count"], 5);
        assert_eq!(proof["exact_response_receipt_verified"], true);
        assert!(proof.get("metadata").is_none());
        assert!(proof.get("requester_subject").is_none());
        assert!(proof.get("decider_subject").is_none());
    }

    #[test]
    fn wrong_source_actor_digest_scope_receipt_or_phase_cannot_qualify() {
        for case in 0..8 {
            let (mut events, expected) = sample();
            match case {
                0 => events[0]["source_instance"] = json!("other-producer/default"),
                1 => events[2]["actor_id"] = json!("requester"),
                2 => events[2]["metadata"]["intent_digest"] = json!("b".repeat(64)),
                3 => events[2]["scope_id"] = json!("other-deployment"),
                4 => events[4]["metadata"]["receipt"] = json!("another-receipt"),
                5 => {
                    events.pop();
                }
                6 => events.push(events[4].clone()),
                7 => events[3]["outcome"] = json!("failure"),
                _ => unreachable!(),
            }
            assert!(
                verify(&decode(events), &expected, "deployment-one").is_err(),
                "case {case}"
            );
        }
    }
}
