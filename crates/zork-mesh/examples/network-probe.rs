//! Exercise a deployed relay and pkarr service with direct IP transports disabled.
//! `network-probe init DIR` creates isolated identities and prints their public keys.
//! `network-probe check DIR RELAY_URL DISCOVERY_URL [IDLE_SECONDS]` tests a round trip.
use anyhow::{ensure, Context, Result};
use iroh::{
    address_lookup::{PkarrPublisher, PkarrResolver},
    endpoint::presets,
    tls::CaTlsConfig,
    Endpoint, RelayMode, RelayUrl, SecretKey,
};
use std::{path::Path, time::Duration};

const ALPN: &[u8] = b"zork/network-probe/1";

fn key(dir: &Path, name: &str, create: bool) -> Result<SecretKey> {
    let path = dir.join(name);
    if create && !path.exists() {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&path)?
            .write_all(&SecretKey::generate().to_bytes())?;
    }
    let bytes: [u8; 32] = std::fs::read(path)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid probe key"))?;
    Ok(SecretKey::from_bytes(&bytes))
}

async fn endpoint(key: SecretKey, relay: RelayUrl, discovery: url::Url) -> Result<Endpoint> {
    Ok(Endpoint::builder(presets::N0)
        .secret_key(key)
        .ca_tls_config(CaTlsConfig::system())
        .alpns(vec![ALPN.to_vec()])
        .clear_ip_transports()
        .clear_address_lookup()
        .relay_mode(RelayMode::Custom([relay].into_iter().collect()))
        .address_lookup(PkarrPublisher::builder(discovery.clone()))
        .address_lookup(PkarrResolver::builder(discovery))
        .bind()
        .await?)
}

async fn check(dir: &Path, relay: RelayUrl, discovery: url::Url, idle: u64) -> Result<()> {
    let a = endpoint(key(dir, "a.key", false)?, relay.clone(), discovery.clone()).await?;
    let b = endpoint(key(dir, "b.key", false)?, relay, discovery).await?;
    tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(a.online(), b.online());
    })
    .await
    .context("relay authentication/online timed out")?;
    println!("Both endpoints online; direct IP transports disabled");
    let server = tokio::spawn({
        let b = b.clone();
        async move {
            let conn = b.accept().await.context("endpoint closed")?.await?;
            for _ in 0..2 {
                let (mut send, mut recv) = conn.accept_bi().await?;
                let bytes = recv.read_to_end(4 * 1024 * 1024).await?;
                send.write_all(blake3::hash(&bytes).as_bytes()).await?;
                send.finish()?;
            }
            conn.closed().await;
            Ok::<_, anyhow::Error>(())
        }
    });
    // Only the endpoint ID is provided. The custom discovery must supply its relay address.
    let conn = tokio::time::timeout(Duration::from_secs(60), a.connect(b.id(), ALPN)).await??;
    let bytes: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    for round in 0..2 {
        if round > 0 {
            tokio::time::sleep(Duration::from_secs(idle)).await;
        }
        tokio::time::timeout(Duration::from_secs(60), async {
            let (mut send, mut recv) = conn.open_bi().await?;
            send.write_all(&bytes).await?;
            send.finish()?;
            let digest = recv.read_to_end(32).await?;
            ensure!(
                digest == blake3::hash(&bytes).as_bytes(),
                "payload digest mismatch"
            );
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        println!("Round {}: 4 MiB verified through relay", round + 1);
    }
    conn.close(0u32.into(), b"done");
    server.await??;
    a.close().await;
    b.close().await;
    println!(
        "PASS: discovery lookup, authenticated relay, payload integrity, {idle}s idle recovery"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(
        args.get(2)
            .context("usage: network-probe init|check DIR [RELAY DISCOVERY IDLE]")?,
    );
    match args.get(1).map(String::as_str) {
        Some("init") => {
            std::fs::create_dir_all(dir)?;
            for name in ["a.key", "b.key"] {
                println!("{}", key(dir, name, true)?.public());
            }
            Ok(())
        }
        Some("check") => {
            let relay = args.get(3).context("missing relay URL")?.parse()?;
            let discovery = args.get(4).context("missing discovery URL")?.parse()?;
            let idle = args.get(5).map(|v| v.parse()).transpose()?.unwrap_or(90);
            check(dir, relay, discovery, idle).await
        }
        _ => anyhow::bail!("expected init or check"),
    }
}
