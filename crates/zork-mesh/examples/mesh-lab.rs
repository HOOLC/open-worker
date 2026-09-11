//! Embedded node/QUIC/eBPF library integration, using isolated throwaway identities.
//! cargo run -p zork-mesh --example mesh-lab
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;
use zork_config::MeshConfig;
use zork_mesh::managed;
use zork_mesh::{
    bridge::{self, BridgeAuth},
    node::MeshNode,
};

struct Node {
    child: managed::Runtime,
    root: PathBuf,
    control: MeshNode,
    origin: String,
    port: u16,
}

fn config(port: u16) -> MeshConfig {
    MeshConfig {
        enabled: true,
        offline: true,
        bind: Some(format!("127.0.0.1:{port}")),
        ..Default::default()
    }
}

async fn start(data: &Path) -> Result<Node> {
    let port = std::net::UdpSocket::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let child = managed::start(data, &config(port)).await?;
    let control = child.node();
    let origin = control.identity().await?;
    ensure!(
        !control.data_dir().join("control.sock").exists(),
        "local control socket created"
    );
    Ok(Node {
        root: data.to_path_buf(),
        child,
        control,
        origin,
        port,
    })
}

async fn trust(a: &Node, b: &Node) -> Result<()> {
    a.control
        .trust(
            &b.origin,
            "isolated Zork test peer",
            Some(&format!("127.0.0.1:{}", b.port)),
        )
        .await
}

#[tokio::main]
async fn main() -> Result<()> {
    let root = tempfile::Builder::new().prefix("zm-").tempdir_in("/tmp")?;
    // Keep diagnostics on a failed run; no private data is printed.
    let root = root.keep();
    println!("isolated lab: {}", root.display());
    let mut a = start(&root.join("a")).await?;
    let mut b = start(&root.join("b")).await?;
    let mut stranger = start(&root.join("stranger")).await?;
    trust(&a, &b).await?;
    trust(&b, &a).await?;
    trust(&stranger, &b).await?; // Deliberately unilateral: B must reject it.

    a.control.add_api_source("zork-lab").await?;
    let bytes = "不可变任务产物\n".repeat(16384).into_bytes();
    let object = a
        .control
        .put("zork-lab", "artifacts/example/v1", &bytes)
        .await?;
    ensure!(
        b.control.read(&object).await? == bytes,
        "remote bytes mismatch"
    );
    let mut forged = object.clone();
    forged.root = "00".repeat(32);
    ensure!(
        b.control.read(&forged).await.is_err(),
        "accepted incorrect root"
    );
    b.control.pin(&object).await?;
    println!("PASS: mutually trusted nodes transfer verified bytes; incorrect root refused");

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let ingress_port = listener.local_addr()?.port();
    let token = "c".repeat(64); // Disposable test credential, local activation only.
    let expected = a.origin.clone();
    let serving = tokio::spawn(bridge::serve(
        listener,
        Arc::new(BridgeAuth::new(token.clone())?),
        move |peer, payload| {
            let expected = expected.clone();
            async move {
                ensure!(peer.origin == expected, "unpaired peer");
                Ok(json!({ "v": 1, "kind": "hello", "authenticated_peer": peer, "echo": payload }))
            }
        },
    ));
    let source_dir = root.join("b-control");
    std::fs::create_dir_all(&source_dir)?;
    let c = root.join("bridge.c");
    std::fs::write(&c, bridge::source_for_port(ingress_port)?)?;
    let elf = source_dir.join("mesh.sock");
    std::fs::write(
        &elf,
        synch_cc::compile_file(&c, &[("synch.h", synch_sock::sdk::HEADER)], &[])?,
    )?;
    b.control
        .add_filesystem_source("zork-control", &source_dir)
        .await?;
    b.control.activate_bridge(token.clone(), 32).await?;
    b.control.publish().await?;

    let payload =
        json!({"v":1,"kind":"hello","from_node":"key:forged","padding":"x".repeat(70000)});
    let hello = a
        .control
        .exchange(&b.origin, &payload)
        .await
        .context("authenticated OpenSocket exchange")?;
    ensure!(
        hello["authenticated_peer"]["origin"] == a.origin && hello["echo"] == payload,
        "identity or fragmented payload mismatch"
    );
    ensure!(
        stranger
            .control
            .exchange(&b.origin, &json!({"kind":"hello"}))
            .await
            .is_err(),
        "untrusted node admitted"
    );
    // A local process cannot bypass the bridge using the wrong prelude secret.
    let mut direct = tokio::net::TcpStream::connect(("127.0.0.1", ingress_port)).await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    direct
        .write_all(format!("ZORKMESH1\n{}\n", "d".repeat(64)).as_bytes())
        .await?;
    let mut probe = [0_u8; 1];
    let rejected = tokio::time::timeout(Duration::from_secs(3), direct.read(&mut probe)).await?;
    ensure!(!matches!(rejected, Ok(n) if n > 0), "local secret bypass");
    println!("PASS: real eBPF/OpenSocket bridge preserves authenticated identity and fragmented frames; stranger and false local credential refused");

    b.child.shutdown().await?;
    ensure!(
        b.control.identity().await.is_err(),
        "stopped node handle remained active"
    );
    b.child = managed::start(&b.root, &config(b.port)).await?;
    b.control = b.child.node();
    ensure!(
        b.control.identity().await? == b.origin,
        "identity changed on restart"
    );
    for _ in 0..20 {
        if a.control
            .exchange(&b.origin, &json!({"kind":"hello"}))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    ensure!(
        a.control
            .exchange(&b.origin, &json!({"kind":"hello"}))
            .await?["authenticated_peer"]["origin"]
            == a.origin,
        "socket failed after node restart"
    );
    ensure!(
        b.control.read(&object).await? == bytes,
        "pinned bytes lost after restart"
    );
    println!("PASS: library node restart retains identity, activation and pinned content");

    // Revocation must be checked on a fresh invocation, even with a cached connection.
    b.control.untrust(&a.origin).await?;
    ensure!(
        a.control
            .exchange(&b.origin, &json!({"kind":"hello"}))
            .await
            .is_err(),
        "revoked peer admitted"
    );
    println!("PASS: revoked peer cannot open a new invocation");
    serving.abort();
    a.child.shutdown().await?;
    b.child.shutdown().await?;
    stranger.child.shutdown().await?;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"synch":"0.1.8","checks": ["verified_file", "wrong_hash", "authenticated_bridge", "fragmented_payload", "untrusted_peer", "local_secret", "restart", "revocation"]}),
        )?,
    )?;
    println!(
        "all mesh foundation checks passed; evidence: {}",
        root.join("result.json").display()
    );
    Ok(())
}
