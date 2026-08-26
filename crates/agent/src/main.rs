use std::{
    collections::BTreeMap,
    future::{Future, IntoFuture},
    io::Write,
    sync::Arc,
    time::Duration,
};

mod provider;

use axum::{routing::get, Json, Router};
use provider::{FakeProvider, ProviderRouter};
use serde_json::json;
use zork_agent::{
    http,
    session::{
        runtime::{AgentDefinition, Runtime, RuntimeOptions, TimerArm, TimerPort},
        store::JsonlEventStore,
        timer::{SleepTimer, SystemClock},
        tools::{coding_tool_names, WorkspaceToolExecutor},
    },
    ProfileStore,
};

const ENDPOINT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const PROFILE_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();

    let args = zork_config::parse_process_args()?;
    let mut file = zork_config::ensure_layout(&args.data_root)?;
    if let Some(host) = &args.listen_host {
        zork_config::apply_listen(&mut file, host);
    }
    let listen = if file.bind.agent.trim().is_empty() {
        "127.0.0.1:3010".to_owned()
    } else {
        file.bind.agent.clone()
    };
    let listen_addr = listen
        .parse()
        .map_err(|_| format!("invalid agent listen address {listen}"))?;
    let agent_token = args.agent_token.clone();

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    tokio_runtime.block_on(run(
        listen_addr,
        file,
        args.data_root,
        args.fake_agent,
        args.no_streaming,
        agent_token,
    ))
}

async fn run(
    listen_addr: std::net::SocketAddr,
    file: zork_config::FileConfig,
    data_root: std::path::PathBuf,
    fake_agent: bool,
    no_streaming: bool,
    agent_token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions_root = data_root.join("sessions");
    let store = Arc::new(
        tokio::task::spawn_blocking({
            let sessions_root = sessions_root.clone();
            move || JsonlEventStore::open(sessions_root)
        })
        .await??,
    );
    let provider: Arc<dyn zork_agent::session::runtime::ModelExecutor> = if fake_agent {
        Arc::new(FakeProvider)
    } else {
        Arc::new(ProviderRouter::new())
    };
    let tools = Arc::new(WorkspaceToolExecutor::new());
    let clock = Arc::new(SystemClock);
    let (due_tx, mut due_rx) = tokio::sync::mpsc::unbounded_channel::<TimerArm>();
    let timer = Arc::new(SleepTimer::new(clock.clone(), due_tx));
    let profiles = Arc::new(ProfileStore::open(
        data_root.clone(),
        fake_agent,
        no_streaming,
    ));
    let definition = AgentDefinition {
        tools: coding_tool_names(),
        tool_environment: session_tool_environment(&data_root, &file)?,
    };
    let runtime = Runtime::new_with_options(
        store.clone(),
        provider,
        tools,
        RuntimeOptions::defaults(),
        clock,
        timer.clone(),
        profiles.clone(),
        definition,
    );
    let expire = runtime.clone();
    tokio::spawn(async move {
        while let Some(arm) = due_rx.recv().await {
            expire.expire_wait(arm).await;
        }
    });
    runtime.queue_startup_recovery().await?;

    let status_profiles = profiles.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = status_profiles.refresh_all_statuses().await {
                tracing::error!(error = %error, "profile status refresh failed");
            }
            tokio::time::sleep(PROFILE_STATUS_REFRESH_INTERVAL).await;
        }
    });

    let state = http::AppState::new(runtime.clone(), store, profiles, agent_token);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    println!("zork-agent ready http://{}", listener.local_addr()?);
    std::io::stdout().flush()?;
    zork_config::write_ready_pid(&data_root, "zork-agent")?;

    let shutdown_signal = arm_shutdown_signal()?;
    tokio::pin!(shutdown_signal);
    let (shutdown, shutdown_requested) = tokio::sync::oneshot::channel::<()>();
    let serving = axum::serve(listener, with_readyz(state))
        .with_graceful_shutdown(async {
            let _ = shutdown_requested.await;
        })
        .into_future();
    tokio::pin!(serving);
    let result = tokio::select! {
        result = &mut serving => result,
        () = &mut shutdown_signal => {
            let _ = shutdown.send(());
            runtime.shutdown().await;
            match tokio::time::timeout(ENDPOINT_DRAIN_TIMEOUT, &mut serving).await {
                Ok(result) => result,
                Err(_) => Ok(()),
            }
        }
    };
    runtime.shutdown().await;
    timer.shutdown();
    zork_config::clear_ready_pid(&data_root, "zork-agent");
    result?;
    Ok(())
}

fn with_readyz(state: http::AppState) -> Router {
    Router::new()
        .route("/readyz", get(readyz))
        .merge(http::router(state))
}

fn session_tool_environment(
    data_root: &std::path::Path,
    file: &zork_config::FileConfig,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut paths = vec![data_root.join("bin")];
    if let Some(inherited) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    let path = std::env::join_paths(paths)?.to_string_lossy().into_owned();
    Ok(BTreeMap::from([
        (
            "BROKER_API_BASE".to_owned(),
            zork_config::loopback_base_url(&file.bind.runtime),
        ),
        (
            "REPOS_ROOT".to_owned(),
            data_root.join("repos").to_string_lossy().into_owned(),
        ),
        ("PATH".to_owned(), path),
    ]))
}

async fn readyz() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "pid": std::process::id(),
        "service": "zork-agent",
    }))
}

#[cfg(unix)]
fn arm_shutdown_signal() -> Result<impl Future<Output = ()>, std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
    })
}

#[cfg(not(unix))]
fn arm_shutdown_signal() -> Result<impl Future<Output = ()>, std::io::Error> {
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
    })
}
