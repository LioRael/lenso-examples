use std::{env, fs, path::PathBuf};

use lenso_service::{
    CoordinationOutageClaims, DeterministicCoordinationAuthorityProvider,
    attest_coordination_outage,
};

fn main() -> anyhow::Result<()> {
    let input_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: support-system-m5-attest-outage <claims.json>"))?;
    let claims: CoordinationOutageClaims = serde_json::from_slice(&fs::read(input_path)?)?;
    let provider = DeterministicCoordinationAuthorityProvider::new([(
        "data-plane-probe:lenso-m5-kind",
        "ephemeral-m5-outage-observation-key",
    )]);
    let observation =
        attest_coordination_outage(claims, "data-plane-probe:lenso-m5-kind", &provider)
            .map_err(|issue| anyhow::anyhow!("outage attestation blocked: {issue:?}"))?;
    println!(
        "M5_OUTAGE_OBSERVATION={}",
        serde_json::to_string(&observation)?
    );
    Ok(())
}
