use std::path::PathBuf;

use anyhow::Context as _;
use lenso_service::{
    CanaryPlan, CanaryState, DeploymentObservation, DeterministicReliabilityObservationProvider,
    ReliabilityObservation, evaluate_canary, seal_reliability_observation,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    plan: CanaryPlan,
    deployment_observation: DeploymentObservation,
    observations: Vec<ReliabilityObservation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    state: CanaryState,
}

fn main() -> anyhow::Result<()> {
    let input_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: support-system-m5-canary-step <input.json>")?;
    let input: Input = serde_json::from_slice(
        &std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?,
    )
    .context("decode canary step input")?;
    let provider = DeterministicReliabilityObservationProvider::new([(
        "kubernetes-http-reliability-adapter",
        "ephemeral-m5-reliability-key",
    )]);
    let mut state = CanaryState::new(input.plan.plan_id.clone());
    for observation in input.observations {
        let sealed = seal_reliability_observation(
            &input.plan,
            &input.deployment_observation,
            &provider,
            observation,
        )
        .map_err(|issue| anyhow::anyhow!(issue.message))?;
        let _decision = evaluate_canary(&mut state, &input.plan, sealed, &provider);
    }
    println!(
        "M5_CANARY_STEP={}",
        serde_json::to_string(&Output { state })?
    );
    Ok(())
}
