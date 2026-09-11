//! Shared activity presentation. Platform adapters render this snapshot; they do
//! not implement action retention, request phases, or throughput calculations.
use crate::api::AgentStatus;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const ACTION_GRACE_MS: i64 = 3_000;
const RATE_WINDOW_MS: i64 = 2_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Detail {
    Action {
        text: String,
    },
    Requesting,
    Streaming {
        tokens_per_second: u64,
        estimated: bool,
    },
    Processing,
    Waiting {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Presentation {
    pub detail: Detail,
}

impl Serialize for Presentation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut value = serializer.serialize_struct("Presentation", 3)?;
        value.serialize_field("detail", &self.detail)?;
        value.serialize_field("label_zh", &self.label("zh-CN"))?;
        value.serialize_field("label_en", &self.label("en"))?;
        value.end()
    }
}

impl Presentation {
    pub fn label(&self, locale: &str) -> String {
        let zh = locale.starts_with("zh");
        let detail = match &self.detail {
            Detail::Action { text } => text.clone(),
            Detail::Requesting => if zh { "请求中" } else { "Requesting" }.into(),
            Detail::Processing => if zh { "处理中" } else { "Processing" }.into(),
            Detail::Streaming {
                tokens_per_second,
                estimated,
            } => {
                format!(
                    "{}{tokens_per_second} token/s",
                    if *estimated { "≈" } else { "" }
                )
            }
            Detail::Waiting { reason } => {
                if zh {
                    format!("等待中 · {reason}")
                } else {
                    format!("Waiting · {reason}")
                }
            }
        };
        detail
    }
}

#[derive(Default)]
pub struct Activity {
    action: String,
    request: Option<(String, i64)>,
    first_output: Option<i64>,
    // Fixed time buckets, bounded independently of stream chunk frequency.
    samples: VecDeque<(i64, u64)>,
}

impl Activity {
    pub fn begin_request(&mut self, id: String, now: i64) {
        self.request = Some((id, now));
        self.first_output = None;
        self.samples.clear();
    }

    pub fn end_request(&mut self, id: &str) {
        if self.request.as_ref().is_some_and(|r| r.0 == id) {
            self.request = None;
        }
    }

    pub fn requesting(&self) -> bool {
        self.request.is_some()
    }

    /// Absolute next presentation deadline. No polling while idle or waiting
    /// for first output after the action grace has elapsed.
    pub fn next_wake(&self, now: i64) -> Option<i64> {
        let (_, started) = self.request.as_ref()?;
        let expires = started.saturating_add(ACTION_GRACE_MS);
        if self.first_output.is_none() && !self.action.is_empty() && now < expires {
            return Some(expires);
        }
        if self.first_output.is_some()
            && self
                .samples
                .back()
                .is_some_and(|(at, _)| now <= at.saturating_add(RATE_WINDOW_MS + 250))
        {
            Some((now.div_euclid(250) + 1) * 250)
        } else {
            None
        }
    }

    /// Output is UTF-8 byte count, never stream chunk count. Until providers
    /// supply incremental token usage, throughput is explicitly approximate.
    pub fn output(&mut self, id: &str, bytes: u64, now: i64) -> bool {
        if bytes == 0 || !self.request.as_ref().is_some_and(|r| r.0 == id) {
            return false;
        }
        let first = self.first_output.is_none();
        self.first_output.get_or_insert(now);
        let bucket = now.div_euclid(250) * 250;
        if let Some((_, count)) = self.samples.back_mut().filter(|(at, _)| *at == bucket) {
            *count = count.saturating_add(bytes);
        } else {
            self.samples.push_back((bucket, bytes));
        }
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| *at < bucket - RATE_WINDOW_MS)
        {
            self.samples.pop_front();
        }
        first
    }

    pub fn observe(&mut self, status: &AgentStatus) {
        match status {
            AgentStatus::Clear
            | AgentStatus::Finished
            | AgentStatus::Interrupted
            | AgentStatus::Failed { .. } => *self = Self::default(),
            AgentStatus::ToolsStarted { calls, .. } | AgentStatus::ToolsWaiting { calls, .. } => {
                for call in calls {
                    if !call.action.is_empty() {
                        self.action = call.action.clone();
                    }
                }
            }
            _ => {}
        }
    }

    pub fn present(&self, status: AgentStatus, now: i64) -> AgentStatus {
        let detail = match &status {
            AgentStatus::Thinking
            | AgentStatus::ToolsStarted { .. }
            | AgentStatus::ToolsWaiting { .. }
            | AgentStatus::ToolFinished { .. } => {
                if let Some((_, started)) = &self.request {
                    if self.first_output.is_none()
                        && !self.action.is_empty()
                        && now.saturating_sub(*started) < ACTION_GRACE_MS
                    {
                        Detail::Action {
                            text: self.action.clone(),
                        }
                    } else if let Some(first) = self.first_output {
                        let bytes: u64 = self
                            .samples
                            .iter()
                            .filter(|(at, _)| *at >= now - RATE_WINDOW_MS)
                            .map(|(_, bytes)| *bytes)
                            .sum();
                        let elapsed = now.saturating_sub(first).clamp(250, RATE_WINDOW_MS) as u64;
                        Detail::Streaming {
                            tokens_per_second: bytes.saturating_mul(1000) / (4 * elapsed),
                            estimated: true,
                        }
                    } else {
                        Detail::Requesting
                    }
                } else if !self.action.is_empty() {
                    Detail::Action {
                        text: self.action.clone(),
                    }
                } else {
                    Detail::Processing
                }
            }
            AgentStatus::Waiting { reason, .. } => Detail::Waiting {
                reason: reason.clone(),
            },
            _ => return status,
        };
        AgentStatus::Live {
            presentation: Presentation { detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn thinking() -> AgentStatus {
        AgentStatus::Thinking
    }
    fn detail(a: &Activity, at: i64) -> Presentation {
        let AgentStatus::Live { presentation } = a.present(thinking(), at) else {
            panic!()
        };
        presentation
    }
    #[test]
    fn action_expires_at_three_seconds_without_another_event() {
        let mut a = Activity {
            action: "读取图片".into(),
            ..Default::default()
        };
        a.begin_request("one".into(), 100);
        assert!(matches!(detail(&a, 3099).detail, Detail::Action { .. }));
        assert_eq!(detail(&a, 3100).detail, Detail::Requesting);
        a.output("one", 400, 9100);
        assert!(matches!(detail(&a, 9350).detail, Detail::Streaming { .. }));
        assert_eq!(
            detail(&a, 12000).detail,
            Detail::Streaming {
                tokens_per_second: 0,
                estimated: true
            }
        );
    }
    #[test]
    fn stale_output_is_ignored_and_new_request_resets_ttft() {
        let mut a = Activity::default();
        a.begin_request("one".into(), 0);
        a.output("one", 100, 100);
        a.begin_request("two".into(), 200);
        a.output("one", 400, 300);
        a.end_request("one");
        assert_eq!(detail(&a, 400).detail, Detail::Requesting);
        a.observe(&AgentStatus::Finished);
        assert!(!a.requesting());
    }
    #[test]
    fn new_action_replaces_old_and_finishing_older_tool_does_not_restore_it() {
        let mut a = Activity::default();
        for (id, action) in [("a", "读取图片"), ("b", "检查图例")] {
            let status =
                serde_json::from_value(serde_json::json!({"state":"tools_started", "calls":[{
                    "tool_call_id":id, "tool_name":"file.read", "goal":"检查图表", "action":action
                }]}))
                .unwrap();
            a.observe(&status);
        }
        a.observe(&AgentStatus::ToolFinished {
            tool_call_id: "a".into(),
        });
        assert_eq!(
            detail(&a, 0).detail,
            Detail::Action {
                text: "检查图例".into()
            }
        );
        a.begin_request("r".into(), 100);
        assert!(matches!(detail(&a, 199).detail, Detail::Action { .. }));
        a.output("r", 100, 200);
        assert!(matches!(detail(&a, 200).detail, Detail::Streaming { .. }));
        assert_eq!(a.next_wake(200), Some(250));
        assert!(matches!(detail(&a, 3099).detail, Detail::Streaming { .. }));
        assert!(matches!(detail(&a, 3100).detail, Detail::Streaming { .. }));
    }
    #[test]
    fn wakeups_stop_after_grace_and_after_output_stalls() {
        let mut a = Activity::default();
        a.begin_request("s".into(), 0);
        assert_eq!(a.next_wake(0), None);
        a.action = "读取图例".into();
        assert_eq!(a.next_wake(100), Some(3000));
        assert_eq!(a.next_wake(3000), None);
        a.output("s", 80, 4000);
        assert_eq!(a.next_wake(4000), Some(4250));
        assert_eq!(a.next_wake(6500), None);
    }
    #[test]
    fn localized_labels_survive_core_snapshot_roundtrip() {
        let mut a = Activity::default();
        a.observe(&AgentStatus::Thinking {});
        a.begin_request("r".into(), 0);
        let value = serde_json::to_value(a.present(thinking(), 0)).unwrap();
        let status: AgentStatus = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(status).unwrap(), value);
        assert_eq!(value["presentation"]["label_zh"], "请求中");
    }
    #[test]
    fn sample_storage_is_bounded_and_chunk_partition_does_not_change_rate() {
        let mut a = Activity::default();
        let mut b = Activity::default();
        a.begin_request("s".into(), 0);
        b.begin_request("s".into(), 0);
        for at in 0..10000 {
            a.output("s", 40, at);
            for _ in 0..10 {
                b.output("s", 4, at);
            }
        }
        assert!(a.samples.len() <= 9);
        assert_eq!(detail(&a, 10000), detail(&b, 10000));
    }
}
