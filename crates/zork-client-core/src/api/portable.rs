//! Public, offline Gateway adapter for portable core contracts and UI fixtures.
#[path = "profile_types.rs"]
mod profile_types;
pub use profile_types::{ProfileInfo, ProfileModel, ProfileQuota};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub trait Executor {
    fn wait(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + 'static>>;
    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + 'static>>);
}
type Fixture = (Value, Value, Arc<dyn Executor>);
thread_local! {static FIXTURE: RefCell<Option<Fixture>> = const {RefCell::new(None)};}
pub fn configure_fixture(fixture: Value, providers: Value, executor: Arc<dyn Executor>) {
    FIXTURE.with(|slot| *slot.borrow_mut() = Some((fixture, providers, executor)));
}
pub(crate) struct ClientTask {
    abort: futures_util::future::AbortHandle,
    finished: Arc<AtomicBool>,
}
impl ClientTask {
    pub fn abort(&self) {
        self.abort.abort();
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}
pub struct GatewayClient {
    data: Mutex<Value>,
    providers: Value,
    executor: Arc<dyn Executor>,
}
impl GatewayClient {
    pub fn new(_: &str, _: Option<String>) -> Self {
        let (fixture, providers, executor) = FIXTURE
            .with(|fixture| fixture.borrow().clone())
            .expect("configure the core fixture adapter before opening views");
        let id = fixture["profile"]["profile_id"].as_str().unwrap();
        Self {
            data: Mutex::new(
                json!({"profiles":{id:fixture["profile"]},"agents":fixture["agents"]}),
            ),
            providers,
            executor,
        }
    }
    pub(crate) async fn wait(&self, duration: Duration) {
        self.executor.wait(duration).await;
    }
    pub(crate) fn spawn<F: Future<Output = ()> + 'static>(&self, future: F) -> ClientTask {
        let (abort, registration) = futures_util::future::AbortHandle::new_pair();
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        self.executor.spawn(Box::pin(async move {
            let _ = futures_util::future::Abortable::new(future, registration).await;
            done.store(true, Ordering::Release);
        }));
        ClientTask { abort, finished }
    }
    pub async fn list_profiles(&self) -> anyhow::Result<Vec<ProfileInfo>> {
        let d = self.data.lock().unwrap();
        Ok(d["profiles"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .collect())
    }
    pub async fn node_request(
        &self,
        method: http::Method,
        path: String,
        body: Option<Value>,
    ) -> anyhow::Result<Value> {
        let mut d = self.data.lock().unwrap();
        let body = body.unwrap_or(Value::Null);
        match path.as_str() {
            "/v1/node/providers" => return Ok(self.providers.clone()),
            "/v1/node/mesh" => return Ok(json!({"origin":"storybook","enabled":false})),
            "/v1/node/agents" => {
                if method == http::Method::POST {
                    d["agents"].as_array_mut().unwrap().push(body.clone());
                    return Ok(body);
                }
                return Ok(json!({"items":d["agents"]}));
            }
            _ => {}
        }
        if path.starts_with("/v1/node/auth") {
            anyhow::bail!("组件展台不连接真实账号；可以操作字段与选择控件。")
        }
        if let Some(tail) = path.strip_prefix("/v1/node/profiles/") {
            let (id, action) = tail.split_once('/').unwrap_or((tail, ""));
            if action == "name" && method == http::Method::PUT {
                d["profiles"][id]["name"] = body["name"].clone();
            }
            if action == "models/refresh" && method == http::Method::POST {
                let models = d["profiles"][id]["models"].as_array_mut().unwrap();
                let mut added = 0;
                if !models.iter().any(|m| m["id"] == "updated-model") {
                    let mut model=models.first().cloned().unwrap_or_else(||json!({"api":"openai-completions","thinking":["off"],"default_thinking":"off","capabilities":{"input":["text"]},"limits":{"context_window_tokens":128000,"max_output_tokens":8192}}));
                    model["id"] = json!("updated-model");
                    model["default"] = json!(false);
                    model["enabled"] = json!(true);
                    models.push(model);
                    added = 1;
                }
                return Ok(
                    json!({"profile":d["profiles"][id],"added":added,"configured":added,"truncated":false}),
                );
            }
            if action == "models/enabled" && method == http::Method::PUT {
                let model = d["profiles"][id]["models"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|m| m["id"] == body["model_id"])
                    .ok_or_else(|| anyhow::anyhow!("model not found"))?;
                anyhow::ensure!(
                    body["enabled"] != true || model["limits"].is_object(),
                    "model limits are missing"
                );
                model["enabled"] = body["enabled"].clone();
            }
            if action == "models" && method == http::Method::PUT {
                anyhow::ensure!(
                    body.get("expected_models")
                        .is_none_or(|expected| *expected == d["profiles"][id]["models"]),
                    "model_configuration_changed"
                );
                d["profiles"][id]["models"] = body["models"].clone();
            }
            if action == "refresh" && method == http::Method::POST {
                d["profiles"][id]["checkedAt"] = json!(chrono::Utc::now().to_rfc3339());
                // Deterministic usage changes let the offline story verify refresh rendering.
                if let Some(used) =
                    d["profiles"][id]["rateLimits"]["rateLimits"]["primary"]["usedPercent"].as_f64()
                {
                    d["profiles"][id]["rateLimits"]["rateLimits"]["primary"]["usedPercent"] =
                        json!((used + 1.).min(100.));
                }
            }
            if action == "discovered-models" {
                return Ok(
                    json!({"supported":true,"items":[{"id":"fixture-model"}],"truncated":false}),
                );
            }
            return Ok(d["profiles"][id].clone());
        }
        if let Some(tail) = path.strip_prefix("/v1/node/agents/") {
            let (id, action) = tail.split_once('/').unwrap_or((tail, ""));
            if let Some(agent) = d["agents"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|a| a["id"] == id)
            {
                if action == "model" {
                    agent["profile_id"] = body["profile_id"].clone();
                    agent["model"] = body["model"].clone();
                    agent["thinking"] = body["thinking"].clone();
                }
                if action == "avatar" {
                    agent["avatar"] = body["avatar"].clone();
                }
                if action == "grants" {
                    agent["allowed_leaders"] = body["allowed_leaders"].clone();
                }
                return Ok(agent.clone());
            }
        }
        anyhow::bail!("此操作不在组件展台数据适配器范围内")
    }
}
