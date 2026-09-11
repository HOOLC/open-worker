//! Headless access client used by scripts/test-shared-services.py.
//! Opens the same loopback bridge as the native browsers; stdin EOF closes it.
use anyhow::{Context, Result};
use std::io::Write;
use zork_mesh::{
    managed,
    services::{LocalService, ServiceLink},
};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = std::path::PathBuf::from(args.next().context("client data directory required")?);
    let link = ServiceLink::parse(&args.next().context("service link required")?)?;
    let config = zork_config::load_config(&root)?.mesh;
    let mut runtime = managed::start_client(&root, &config).await?;
    let result = async {
        let node = runtime.node();
        managed::configure(&root, &config, &node).await?;
        let mut services = vec![LocalService::open(node.clone(), &link).await?];
        println!("{}", serde_json::json!({"url":services[0].url}));
        std::io::stdout().flush()?;
        loop {
            let line = tokio::task::spawn_blocking(|| {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).map(|_| line)
            })
            .await??;
            if line.is_empty() {
                break;
            }
            let request: serde_json::Value = serde_json::from_str(&line)?;
            let link = ServiceLink::parse(request["open"].as_str().context("open link required")?)?;
            let service = LocalService::open(node.clone(), &link).await?;
            println!("{}", serde_json::json!({"url":service.url}));
            std::io::stdout().flush()?;
            services.push(service);
        }
        drop(services);
        Ok::<_, anyhow::Error>(())
    }
    .await;
    runtime.shutdown().await?;
    result
}
