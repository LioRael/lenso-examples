#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let service = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing Service"))?;
    let role = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing Workload role"))?;
    lenso_example_support_system::run_workload(&service, &role).await
}
