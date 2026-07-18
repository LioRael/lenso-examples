#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let evidence = lenso_example_support_system::run_m3_smoke().await?;
    println!("{}", serde_json::to_string_pretty(&evidence)?);
    Ok(())
}
