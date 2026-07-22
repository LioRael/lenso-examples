use std::{env, fs, path::PathBuf};

use lenso_service::{
    CoordinationOperationSubject, CoordinationOutageEvidence, CoordinationResumeState,
    DeploymentPlan, DeterministicCoordinationAuthorityProvider,
    Ed25519OperatorObservationAuthorityProvider, approve_coordination_resume,
    resume_protected_operation,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    outage_proof: CoordinationOutageEvidence,
    deployment_subject: DeploymentPlan,
    coordination_revision: u64,
}

fn main() -> anyhow::Result<()> {
    let input_path = env::args().nth(1).map(PathBuf::from).ok_or_else(|| {
        anyhow::anyhow!("usage: support-system-m5-coordination-resume <input.json>")
    })?;
    let input: Input = serde_json::from_slice(&fs::read(input_path)?)?;
    let outage_provider = DeterministicCoordinationAuthorityProvider::new([(
        "data-plane-probe:lenso-m5-kind",
        "ephemeral-m5-outage-observation-key",
    )]);
    let approval_provider = DeterministicCoordinationAuthorityProvider::new([(
        "coordination-authority:lenso-m5-kind",
        "ephemeral-m5-coordination-approval-key",
    )]);
    let operator_provider =
        Ed25519OperatorObservationAuthorityProvider::from_base64_public_keys([(
            "kubernetes-api:lenso-m5-kind",
            env::var("M5_OPERATOR_OBSERVATION_PUBLIC_KEY")?,
        )])
        .map_err(anyhow::Error::msg)?;
    let subject = CoordinationOperationSubject::DeploymentMutation(input.deployment_subject);
    let approval = approve_coordination_resume(
        &input.outage_proof,
        "protected-operation:deployment:outage-proof-1",
        &subject,
        input.coordination_revision,
        "coordination-authority:lenso-m5-kind",
        &outage_provider,
        &operator_provider,
        &approval_provider,
    )
    .map_err(|issues| anyhow::anyhow!("resume approval blocked: {issues:?}"))?;
    let mut state = CoordinationResumeState::default();
    let first = resume_protected_operation(
        &mut state,
        &input.outage_proof,
        &approval,
        &subject,
        input.coordination_revision,
        &outage_provider,
        &operator_provider,
        &approval_provider,
    )
    .map_err(|issues| anyhow::anyhow!("protected operation resume blocked: {issues:?}"))?;
    let replay = resume_protected_operation(
        &mut state,
        &input.outage_proof,
        &approval,
        &subject,
        input.coordination_revision,
        &outage_provider,
        &operator_provider,
        &approval_provider,
    )
    .map_err(|issues| anyhow::anyhow!("protected operation replay blocked: {issues:?}"))?;
    anyhow::ensure!(
        first == replay,
        "resume replay returned a different receipt"
    );
    anyhow::ensure!(
        state.receipts.len() == 1,
        "resume replay duplicated its receipt"
    );
    println!(
        "M5_COORDINATION_RESUME={}",
        serde_json::to_string(&json!({
            "approval": approval,
            "firstReceipt": first,
            "replayReceipt": replay,
            "receiptCount": state.receipts.len(),
            "duplicateEffects": false,
        }))?
    );
    Ok(())
}
