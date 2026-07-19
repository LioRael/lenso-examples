use lenso_example_support_system::run_m4_smoke;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let evidence = run_m4_smoke().await?;
    println!("M4_SMOKE_EVIDENCE={}", serde_json::to_string(&evidence)?);
    Ok(())
}
