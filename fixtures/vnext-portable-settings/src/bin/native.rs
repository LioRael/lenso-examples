#[cfg(not(target_arch = "wasm32"))]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use lenso_kernel::Kernel;
    use lenso_runner::TokioDriver;
    use lenso_web_ingress_plugin::WebIngressFactory;
    use portable_settings_fixture::{composition, postgres::Postgres};
    use std::{
        io::{self, Write},
        rc::Rc,
        time::Duration,
    };

    let store = Rc::new(Postgres::new(std::env::var("SETTINGS_DATABASE_URL")?));
    if std::env::args().any(|arg| arg == "--initialize") {
        store.initialize().await.map_err(|e| format!("{e:?}"))?;
        return Ok(());
    }
    tokio::task::LocalSet::new()
        .run_until(async move {
            #[cfg(unix)]
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let ingress = WebIngressFactory::new();
            let app = Kernel::start_native(
                composition::plan().map_err(|e| format!("{e:?}"))?,
                TokioDriver::new(),
                composition::registry(store, ingress.clone()),
            )
            .await
            .map_err(|e| format!("{e:?}"))?;
            println!(
                "http://{}",
                ingress.local_address().ok_or("ingress has no listener")?
            );
            io::stdout().flush()?;
            tokio::select! {
                result = tokio::signal::ctrl_c() => { result?; }
                () = async {
                    #[cfg(unix)]
                    terminate.recv().await;
                    #[cfg(not(unix))]
                    std::future::pending::<()>().await;
                } => {}
            }
            let shutdown = app.shutdown(Duration::from_secs(2)).await;
            if !matches!(shutdown, lenso_kernel::ShutdownOutcome::Clean) {
                return Err("fixture shutdown was not clean".into());
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
}

#[cfg(target_arch = "wasm32")]
fn main() {}
