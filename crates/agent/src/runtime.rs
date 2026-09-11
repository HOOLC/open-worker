//! Embeddable Agent assembly. The host owns Tokio, logging, signals and transport.
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use crate::{
    application::Agent,
    provider::{AgentModelPort, FakeProvider, ProviderRouter},
    session::{
        ports::{
            ModelExecutor, SystemClock, SystemFileSystem, SystemIdGenerator, SystemProcessSpawner,
        },
        query::FileSessionQuery,
        service::{ServiceDependencies, ServiceOptions, SessionService},
        tools::{register_builtin_tools, BuiltinToolDependencies, ToolRegistry},
        StreamStore,
    },
    ProfileStore,
};

/// Host-supplied configuration; no CLI parsing, listeners or global environment mutation.
pub struct AgentOptions {
    /// Optional host resolver; receives a runtime session ID.
    pub skill_sources: Option<crate::skills::SkillSources>,
    pub skill_source_manager: Option<crate::skills::SkillSourceManager>,
    pub data_root: PathBuf,
    pub fake_agent: bool,
    pub no_streaming: bool,
    pub context: zork_config::ContextConfig,
    pub environment: BTreeMap<String, String>,
    /// Register host capabilities here before starting. Builtins are added at startup.
    pub tools: Arc<ToolRegistry>,
    pub service: ServiceOptions,
    /// None disables periodic status refresh. The first refresh runs immediately.
    pub profile_refresh_interval: Option<Duration>,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            data_root: PathBuf::new(),
            skill_sources: None,
            skill_source_manager: None,
            fake_agent: false,
            no_streaming: false,
            context: Default::default(),
            environment: BTreeMap::new(),
            tools: Arc::new(ToolRegistry::default()),
            service: ServiceOptions::default(),
            profile_refresh_interval: Some(Duration::from_secs(60)),
        }
    }
}

/// Owns all Agent background services. Call `shutdown().await` before stopping Tokio.
/// Drop only schedules best-effort cleanup; it cannot provide a graceful shutdown guarantee.
/// Agent handles and event streams must also be dropped to release the data-directory lock.
pub struct AgentRuntime {
    agent: Agent,
    refresh: Option<tokio::task::JoinHandle<()>>,
    profile_watch: Option<zork_notify::files::FileWatch>,
    stopped: bool,
}

impl AgentRuntime {
    /// Must be called inside a Tokio runtime. Opening storage is synchronous.
    pub fn start(mut options: AgentOptions) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !options.data_root.as_os_str().is_empty(),
            "data_root is required"
        );
        anyhow::ensure!(
            options.profile_refresh_interval != Some(Duration::ZERO),
            "profile refresh interval must be positive"
        );
        tokio::runtime::Handle::try_current()?;
        let root = options.data_root.clone();
        let sources = options.skill_sources.unwrap_or_else(|| {
            Arc::new(move |_| {
                let config = if zork_config::config_path(&root).try_exists()? {
                    zork_config::load_config(&root)?.skills
                } else {
                    zork_config::SkillsConfig::default()
                };
                config.sources(&root, &[])
            })
        });
        let store = Arc::new(StreamStore::open(&options.data_root)?);
        crate::skills::management::provision_bundled(&options.data_root)?;
        crate::skills::bundled::register(&options.tools, options.data_root.clone())?;
        crate::skills::register_tools(&options.tools, sources.clone())?;
        crate::skills::management::register(
            &options.tools,
            sources.clone(),
            options.skill_source_manager,
        )?;
        options.service.runner.skill_sources = Some(sources);
        let query = Arc::new(FileSessionQuery::open(&options.data_root));
        let provider: Arc<dyn ModelExecutor> = if options.fake_agent {
            Arc::new(FakeProvider)
        } else {
            Arc::new(ProviderRouter::new())
        };
        let profiles = Arc::new(ProfileStore::open(
            options.data_root,
            options.fake_agent,
            options.no_streaming,
        ));
        let model = Arc::new(AgentModelPort::new(provider, profiles.clone()));
        let clock = Arc::new(SystemClock);
        register_builtin_tools(
            &options.tools,
            BuiltinToolDependencies {
                environment: options.environment,
                query: query.clone(),
                clock: clock.clone(),
                files: Arc::new(SystemFileSystem),
                processes: Arc::new(SystemProcessSpawner),
            },
        )?;
        let profile_options = ProfileStore::runner_options(&profiles);
        options.service.runner.input_budget = profile_options.input_budget;
        options.service.runner.max_output_tokens = profile_options.max_output_tokens;
        options.service.runner.context = options.context;
        let service = SessionService::start(
            ServiceDependencies {
                store,
                query,
                model,
                tools: options.tools,
                clock,
                ids: Arc::new(SystemIdGenerator),
            },
            options.service,
        );
        let configuration = zork_notify::Notifier::default();
        let profile_watch = options
            .profile_refresh_interval
            .filter(|_| !options.fake_agent)
            .map(|_| profiles.watch_profiles(configuration.clone()))
            .transpose()?;
        let mut configured = configuration.subscribe();
        let refresh = options.profile_refresh_interval.filter(|_| !options.fake_agent).map(|interval| {
            let profiles = profiles.clone();
            tokio::spawn(async move {
                let mut retry = zork_notify::retry::Retry::default();
                loop {
                    configured.checkpoint();
                    let (external, mut failed) = match profiles.has_external_statuses() {
                        Ok(value) => (value, false),
                        Err(error) => { tracing::error!(%error, "profile configuration read failed"); (false, true) },
                    };
                    if external {
                        if let Err(error) = profiles.refresh_all_statuses().await {
                            failed = true;
                            tracing::error!(%error, "external profile status collection failed");
                        }
                    }
                    // Only the collector samples external query-only providers.
                    // Internal readers subscribe to committed results. With no
                    // configured accounts even this collector has no clock.
                    if !failed { retry.reset(); }
                    tokio::select! {
                        _ = retry.wait(), if failed => {},
                        changed = configured.changed() => if changed.is_err() { return; },
                        _ = tokio::time::sleep(interval), if external && !failed => {},
                    }
                }
            })
        });
        Ok(Self {
            agent: Agent { service, profiles },
            refresh,
            profile_watch,
            stopped: false,
        })
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    /// Idempotent. Stops refresh, cancels running work, and joins core background tasks.
    pub async fn shutdown(&mut self) {
        if self.stopped {
            return;
        }
        if let Some(task) = self.refresh.take() {
            task.abort();
            let _ = task.await;
        }
        self.profile_watch.take();
        self.agent.service.shutdown().await;
        self.stopped = true;
    }
}

impl Drop for AgentRuntime {
    fn drop(&mut self) {
        if let Some(task) = self.refresh.take() {
            task.abort();
        }
        if !self.stopped {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let service = self.agent.service.clone();
                runtime.spawn(async move {
                    service.shutdown().await;
                });
            }
        }
    }
}
