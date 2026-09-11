//! Read-only session aggregates delivered by the session SSE snapshot/update
//! protocol. This module performs no history requests or client persistence.
use super::HistoryRuntime;
use crate::api::{AgentStatus, ParticipantStatus};
use serde::Deserialize;
use zork_agent_api::SessionAggregates;
use zork_client_types::history::{usage::UsageSummary, Entry};

#[derive(Clone, Debug, Default)]
pub struct SessionOverview {
    pub loaded: bool,
    pub error: Option<String>,
    pub runtime: Option<HistoryRuntime>,
    pub aggregates: SessionAggregates,
    pub cursor: Option<String>,
}
impl PartialEq for SessionOverview {
    fn eq(&self, other: &Self) -> bool {
        self.loaded == other.loaded
            && self.error == other.error
            && self.runtime == other.runtime
            && self.aggregates == other.aggregates
    }
}
impl SessionOverview {
    #[cfg(feature = "headless-bench")]
    pub fn fixture(history: &super::HistoryData) -> Self {
        let usage = UsageSummary::new(history.entries.iter());
        let recent = history
            .highlights()
            .map(|entry| zork_agent_api::ExecutionActivity {
                id: entry.id.clone(),
                lane: entry.lane,
                tool: entry.action.clone(),
                action: entry.action.clone(),
                state: entry.state.clone(),
                finished_at_ms: entry.end.unwrap_or_default(),
                error: matches!(entry.state.as_str(), "failed" | "timed_out").then(|| {
                    entry
                        .outcome_summary
                        .clone()
                        .unwrap_or_else(|| entry.summary.clone())
                }),
            })
            .collect();
        Self {
            loaded: true,
            runtime: history.runtime.clone(),
            aggregates: SessionAggregates {
                complete: true,
                usage: zork_agent_api::SessionUsage {
                    input: usage.input,
                    output: usage.output,
                    cached: usage.cached,
                    reported_steps: usage.reported_steps as u64,
                    cache_reported_steps: usage.cache_reported_steps as u64,
                    cache_input: usage.cache_input(),
                },
                recent,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    pub fn usage(&self) -> UsageSummary {
        let usage = &self.aggregates.usage;
        UsageSummary::from_totals(
            usage.input,
            usage.output,
            usage.cached,
            usage.reported_steps.try_into().unwrap_or(usize::MAX),
            usage.cache_reported_steps.try_into().unwrap_or(usize::MAX),
            usage.cache_input,
        )
    }
    pub fn highlights(&self) -> impl Iterator<Item = Entry> + '_ {
        self.aggregates.recent.iter().take(2).map(|activity| Entry {
            id: activity.id.clone(),
            lane: activity.lane,
            action: if activity.tool.is_empty() {
                activity.action.clone()
            } else {
                activity.tool.clone()
            },
            summary: activity.action.clone(),
            start: None,
            end: Some(activity.finished_at_ms),
            state: activity.state.clone(),
            raw: vec![],
            usage: None,
            model: None,
            outcome_summary: activity.error.clone(),
        })
    }
    pub(crate) fn unavailable() -> Self {
        Self {
            loaded: true,
            error: Some("session snapshot unavailable".into()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub cursor: Option<String>,
    pub runtime: HistoryRuntime,
    pub aggregates: SessionAggregates,
}
impl SessionSnapshot {
    pub(crate) fn overview(self) -> SessionOverview {
        SessionOverview {
            loaded: true,
            error: None,
            cursor: self.cursor,
            runtime: Some(self.runtime),
            aggregates: self.aggregates,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct InitialSessionSnapshot {
    pub session_id: String,
    pub status: Option<AgentStatus>,
    pub execution: Option<SessionSnapshot>,
    #[serde(default)]
    pub participants: Vec<ParticipantStatus>,
}
