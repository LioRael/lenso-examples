use lenso_example_support_system::run_m5_smoke;

fn main() -> anyhow::Result<()> {
    let evidence = run_m5_smoke()?;
    println!("M5_SMOKE_EVIDENCE={}", serde_json::to_string(&evidence)?);
    Ok(())
}
