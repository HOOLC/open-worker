//! Compact read model for aggregate consumers. Past events are read through
//! the separate, explicitly requested history API; SSE carries future events.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionUsage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub reported_steps: u64,
    pub cache_reported_steps: u64,
    pub cache_input: u64,
}
impl SessionUsage {
    pub fn cache_hit_rate(&self) -> Option<f64> {
        (self.cache_input > 0).then(|| self.cached as f64 / self.cache_input as f64)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionActivity {
    pub id: String,
    pub lane: usize,
    pub tool: String,
    pub action: String,
    pub state: String,
    pub finished_at_ms: i64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionAggregates {
    /// False for snapshots made before aggregate fields were recorded, or when
    /// a malformed terminal event cannot safely be counted. Never backfill by
    /// fetching the entire execution history for a UI aggregate request.
    pub complete: bool,
    pub usage: SessionUsage,
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub last_run: Option<ExecutionRun>,
    /// At most two completed execution summaries, newest first, bounded text.
    pub recent: Vec<ExecutionActivity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    /// Opaque append-log boundary of the state from which this snapshot was
    /// projected. It is not a UI subscription revision.
    pub cursor: Option<String>,
    pub server_time_ms: i64,
    pub runtime: super::HistoryRuntime,
    pub aggregates: SessionAggregates,
    pub execution: SessionExecution,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionRun {
    pub turn_id: String,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ExecutionTarget {
    Agent(String),
    Task(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionTool {
    pub invocation_id: String,
    pub tool: String,
    pub started_at_ms: i64,
    pub action: String,
    pub labels: std::collections::BTreeMap<String, String>,
    pub detail: String,
    pub target: Option<ExecutionTarget>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionStep {
    pub step_id: String,
    pub started_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ExecutionWait {
    pub deadline_ms: i64,
    pub tools: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionExecution {
    pub generation: u64,
    pub status: super::SessionStatus,
    pub active_step: Option<ExecutionStep>,
    pub active_turn: Option<ExecutionRun>,
    pub tools: Vec<ExecutionTool>,
    pub waiting: Option<ExecutionWait>,
    pub failure: Option<String>,
}
