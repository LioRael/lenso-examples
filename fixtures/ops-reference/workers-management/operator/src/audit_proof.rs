use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const PRODUCER: &str = "example.ops-management/default";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub deployment: String,
    pub operation_id: String,
    pub intent_digest: String,
    pub receipt_id: String,
    pub requester_subject: String,
    pub decider_subject: String,
}

pub fn verify(events: &[Value], expected: &Expected) -> Result<Value, ()> {
    if ![
        &expected.deployment,
        &expected.operation_id,
        &expected.receipt_id,
        &expected.requester_subject,
        &expected.decider_subject,
    ]
    .into_iter()
    .all(|s| crate::label(s))
        || expected.requester_subject == expected.decider_subject
        || expected.intent_digest.len() != 64
        || !expected
            .intent_digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(());
    }
    let mut approved = BTreeSet::new();
    let mut requester_attempts = BTreeSet::new();
    let required = BTreeSet::from([
        "management.attempt",
        "management.human_attempt",
        "management.human_decided",
        "management.dispatch",
        "management.completed",
    ]);
    for event in events {
        let action = event["action"].as_str().ok_or(())?;
        let metadata = event.get("metadata").and_then(Value::as_object).ok_or(())?;
        let human = action.starts_with("management.human_");
        let decider = human && event["actor_id"] == expected.decider_subject;
        let actor = if decider {
            expected.decider_subject.as_str()
        } else {
            expected.requester_subject.as_str()
        };
        if event["source_instance"] != PRODUCER
            || event["event_name"] != "management-operation"
            || event["actor_kind"] != "user"
            || event["actor_id"].as_str() != Some(actor)
            || event["scope_type"] != "deployment"
            || event["scope_id"] != expected.deployment
            || event["resource_type"] != "management-operation"
            || event["resource_id"] != expected.operation_id
            || event["correlation_id"] != expected.operation_id
            || event["request_id"] != expected.operation_id
            || event["outcome"] != "success"
            || event["metadata"]["entry_id"] != "state.update"
            || event["metadata"]["intent_digest"] != expected.intent_digest
        {
            return Err(());
        }
        if human && !decider {
            if action != "management.human_attempt"
                || !requester_attempts.insert(event["id"].as_str().ok_or(())?)
            {
                return Err(());
            }
        } else if !required.contains(action) || !approved.insert(action) {
            return Err(());
        }
        let state = match action {
            "management.dispatch" => "executing",
            "management.completed" => "succeeded",
            _ => "pending_approval",
        };
        if metadata.get("state") != Some(&json!(state))
            || metadata.get("decision")
                != Some(&if human {
                    json!("approved")
                } else {
                    Value::Null
                })
            || metadata.get("receipt")
                != Some(&if action == "management.completed" {
                    json!(expected.receipt_id)
                } else {
                    Value::Null
                })
        {
            return Err(());
        }
    }
    if approved != required {
        return Err(());
    }
    Ok(json!({
        "schema":"lenso.ops-workers-audit-proof.v1", "status":"passed",
        "proof_layer":"supplemental_owner_kernel_reader", "operation_id":expected.operation_id,
        "deployment":expected.deployment, "source_instance":PRODUCER,
        "event_count":events.len(), "actions":required,
        "additional_requester_attempts_verified":requester_attempts.len(),
        "requester_and_decider_verified":true, "actors_distinct":true,
        "intent_digest_consistent":true, "exact_response_receipt_verified":true,
        "domain_value_revision_verified":false, "append_invoked":false,
        "metadata_and_actor_identifiers_redacted":true,
        "audit_owner_source":"64068969061c4bbcd98dbecdba1afbde6ba0cffa",
        "inspection_boundary":"Separate fixed, deployment-scoped Kernel reader; actual Owner typed list port; no setup, append or SQL. Opaque receipt compared with supplied actual ordinary-App response.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<Value>, Expected) {
        let expected = Expected {
            deployment: "deployment-one".into(),
            operation_id: "operation-one".into(),
            intent_digest: "a".repeat(64),
            receipt_id: "primary-state:1:receipt".into(),
            requester_subject: "alice".into(),
            decider_subject: "bob".into(),
        };
        let events = ["attempt","human_attempt","human_decided","dispatch","completed"]
            .into_iter().map(|phase| {
                let human = phase.starts_with("human_");
                json!({"id":format!("event-{phase}"),"action":format!("management.{phase}"),
                    "source_instance":PRODUCER,"event_name":"management-operation","actor_kind":"user",
                    "actor_id":if human {"bob"} else {"alice"},"scope_type":"deployment","scope_id":"deployment-one",
                    "resource_type":"management-operation","resource_id":"operation-one",
                    "correlation_id":"operation-one","request_id":"operation-one","outcome":"success",
                    "metadata":{"entry_id":"state.update","intent_digest":expected.intent_digest,
                        "state":match phase {"dispatch"=>"executing","completed"=>"succeeded",_=>"pending_approval"},
                        "decision":if human {json!("approved")} else {Value::Null},
                        "receipt":if phase=="completed" {json!(expected.receipt_id)} else {Value::Null}}})
            }).collect();
        (events, expected)
    }

    #[test]
    fn requester_attempt_is_not_the_distinct_human_decision() {
        let (mut events, expected) = sample();
        let mut attempted = events[1].clone();
        attempted["id"] = json!("self-attempt");
        attempted["actor_id"] = json!("alice");
        events.push(attempted);
        let proof = verify(&events, &expected).unwrap();
        assert_eq!(proof["event_count"], 6);
        assert!(proof.get("metadata").is_none());
        events[2]["actor_id"] = json!("alice");
        assert!(verify(&events, &expected).is_err());
    }

    #[test]
    fn duplicate_dispatch_or_different_business_receipt_is_not_a_proof() {
        let (mut events, expected) = sample();
        events[4]["metadata"]["receipt"] = json!("another-receipt");
        assert!(verify(&events, &expected).is_err());
        let (mut events, expected) = sample();
        events[0]["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("receipt");
        assert!(verify(&events, &expected).is_err());
        let (mut events, expected) = sample();
        events.push(events[3].clone());
        assert!(verify(&events, &expected).is_err());
    }
}
