//! Timeline layout and platform-local clock formatting. Business event projection is shared.
#[cfg(test)]
use serde_json::Value;
pub use zork_client_types::history::{activity, entries, usage, Entry, Page, Record};
#[derive(Debug)]
pub struct Span {
    pub index: usize,
    pub track: usize,
    pub start: i64,
    pub end: i64,
}
#[derive(Debug)]
pub struct Timeline {
    pub start: i64,
    pub end: i64,
    pub spans: Vec<Span>,
    pub tracks: [usize; 3],
    pub unknown: usize,
}
pub fn timeline<'a>(entries: impl IntoIterator<Item = &'a Entry>, now: i64) -> Timeline {
    let entries = entries.into_iter().collect::<Vec<_>>();
    let mut t = Timeline {
        start: i64::MAX,
        end: 0,
        spans: vec![],
        tracks: [1; 3],
        unknown: 0,
    };

    for (index, e) in entries.iter().enumerate() {
        let Some(start) = e.start.or(e.end) else {
            t.unknown += 1;
            continue;
        };
        let end = e.end.unwrap_or(if e.state == "running" {
            now.max(start)
        } else {
            start
        });
        t.start = t.start.min(start);
        t.end = t.end.max(end);
        t.spans.push(Span {
            index,
            track: 0,
            start,
            end,
        });
    }
    for lane in 0..3 {
        let mut order: Vec<usize> = t
            .spans
            .iter()
            .enumerate()
            .filter(|(_, s)| entries[s.index].lane == lane)
            .map(|(i, _)| i)
            .collect();
        order.sort_by_key(|i| (t.spans[*i].start, t.spans[*i].end));
        let mut ends: Vec<(i64, bool)> = vec![];
        for i in order {
            let span = &mut t.spans[i];
            let track = ends
                .iter()
                .position(|(end, point)| *end < span.start || (*end == span.start && !*point))
                .unwrap_or(ends.len());
            let next = (span.end, span.start == span.end);
            if track == ends.len() {
                ends.push(next);
            } else {
                ends[track] = next;
            }
            span.track = track;
        }
        t.tracks[lane] = ends.len().max(1);
    }
    if t.spans.is_empty() {
        t.start = 0;
    }
    t.end = t.end.max(t.start.saturating_add(1));
    t
}
pub fn duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{}s", (ms as f64 / 100.).round() / 10.)
    } else {
        format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}
#[cfg(target_arch = "wasm32")]
pub fn now() -> i64 {
    js_sys::Date::now() as i64
}
#[cfg(not(target_arch = "wasm32"))]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn utc_clock(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return "—".into() };
    let seconds = ms.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}
#[cfg(not(target_family = "wasm"))]
pub fn clock(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return "—".into() };
    // Local wall time without adding a timezone dependency.
    let seconds = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null() {
        return "—".into();
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}
#[cfg(target_family = "wasm")]
pub fn clock(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return "—".into() };
    let date = js_sys::Date::new(&(ms as f64).into());
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn r(id: &str, event: Value) -> Record {
        Record {
            event_id: id.into(),
            event,
            metadata: Default::default(),
        }
    }
    #[test]
    fn concurrent_same_name_and_paged_orphan_results_join_only_by_id() {
        let result = r(
            "3",
            json!({"kind":"tool_result","result":{"invocation_id":"b","tool":"shell.run","outcome":"succeeded","finished_at_ms":30,"data":{"content":"done"}}}),
        );
        let start = r(
            "2",
            json!({"kind":"step_completed","step_id":"s","completed_at_ms":10,"assistant_text":"thinking","invocations":[{"invocation_id":"a","tool":"shell.run","started_at_ms":10,"arguments":{"command":"pwd"}},{"invocation_id":"b","tool":"shell.run","started_at_ms":11,"arguments":{"command":"ls"}}]}),
        );
        let e = entries(&[result, start]);
        assert_eq!(e.len(), 3);
        let a = e.iter().find(|e| e.id == "tool:a").unwrap();
        let b = e.iter().find(|e| e.id == "tool:b").unwrap();
        assert_eq!(a.state, "running");
        assert_eq!(b.state, "succeeded");
        assert_eq!(b.summary, "ls");
        assert_eq!(b.duration(99), Some(19));
        assert_eq!(timeline(&e, 40).tracks[2], 2);
    }
    #[test]
    fn unknown_start_does_not_invent_duration() {
        let e = entries(&[r(
            "1",
            json!({"kind":"step_completed","step_id":"s","completed_at_ms":30}),
        )]);
        assert_eq!(e[0].duration(40), None);
        assert_eq!(timeline(&e, 40).spans[0].start, 30);
    }
    #[test]
    fn complete_step_preserves_usage_and_measured_duration() {
        let records = [
            r(
                "1",
                json!({"kind":"step_started","step_id":"s","purpose":"compaction","started_at_ms":10}),
            ),
            r(
                "2",
                json!({"kind":"step_completed","step_id":"s","purpose":"compaction","completed_at_ms":50,"assistant_text":"context document","usage":{"input_tokens":20,"output_tokens":5}}),
            ),
        ];
        let e = entries(&records);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].action, "compaction");
        assert_eq!(e[0].duration(80), Some(40));
        assert_eq!(e[0].usage.as_ref().unwrap()["input_tokens"], 20);
        assert_eq!(e[0].raw[0]["event_id"], "1");
    }
    #[test]
    fn model_identity_uses_selection_at_request_start() {
        let all = [
            r(
                "1",
                json!({"kind":"session_created","selection":{"model":"first"}}),
            ),
            r(
                "2",
                json!({"kind":"step_started","step_id":"a","started_at_ms":1}),
            ),
            r(
                "3",
                json!({"kind":"selection_changed","selection":{"model":"second"}}),
            ),
            r(
                "4",
                json!({"kind":"step_completed","step_id":"a","completed_at_ms":2}),
            ),
            r(
                "5",
                json!({"kind":"step_started","step_id":"b","started_at_ms":2}),
            ),
        ];
        let projected = entries(&all);
        assert_eq!(projected[0].model.as_deref(), Some("first"));
        assert_eq!(projected[1].model.as_deref(), Some("second"));
        assert_eq!(timeline(&projected, 3).tracks[1], 1);
        // A page that lacks earlier selection facts must not invent a model name.
        assert_eq!(entries(&all[1..])[0].model, None);
    }
    #[test]
    fn fold_only_idle_boundaries_and_keep_axis_invertible() {
        let records = [
            r(
                "1",
                json!({"kind":"step_started","step_id":"s","started_at_ms":10}),
            ),
            r(
                "2",
                json!({"kind":"step_completed","step_id":"s","completed_at_ms":50}),
            ),
            r(
                "3",
                json!({"kind":"input_appended","input":{"content":"next","received_at_ms":5000}}),
            ),
            r(
                "4",
                json!({"kind":"step_started","step_id":"t","started_at_ms":5000}),
            ),
            r(
                "5",
                json!({"kind":"step_completed","step_id":"t","completed_at_ms":5100}),
            ),
        ];
        let mut e = entries(&records);
        let axis = Axis::new(&e, &timeline(&e, 5100));
        assert_eq!(axis.gaps, vec![(50, 5000)]);
        assert_eq!(axis.position(50), axis.position(5000));
        assert_eq!(axis.time_at(axis.position(5000)), 5000);
        assert_eq!(axis.time_at(1.), 5100);
        e[0].end = None;
        e[0].state = "running".into();
        let axis = Axis::new(&e, &timeline(&e, 5100));
        assert!(axis.gaps.is_empty());
    }

    #[test]
    fn idle_between_any_events_has_zero_width_but_overlap_stays_visible() {
        let t = Timeline {
            start: 0,
            end: 10030,
            spans: vec![
                Span {
                    index: 0,
                    track: 0,
                    start: 0,
                    end: 10,
                },
                Span {
                    index: 1,
                    track: 0,
                    start: 10000,
                    end: 10020,
                },
                Span {
                    index: 2,
                    track: 1,
                    start: 10010,
                    end: 10030,
                },
            ],
            tracks: [1, 1, 2],
            unknown: 0,
        };
        let axis = Axis::new(&[], &t);
        assert_eq!(axis.gaps, vec![(10, 10000)]);
        assert_eq!(axis.duration, 40.);
        assert_eq!(axis.position(10), axis.position(10000));
        assert_eq!(axis.position(5000), axis.position(10));
        assert_eq!(axis.time_at(axis.position(10010)), 10010);
        assert_eq!(axis.time_at(1.), 10030);
    }
}

/// Remove every interval with no active event. Running spans and waits retain
/// their full duration; timestamps remain real wall-clock times.
pub struct Axis {
    pub start: i64,
    pub duration: f64,
    pub gaps: Vec<(i64, i64)>,
}
impl Axis {
    pub fn new<'a>(_entries: impl IntoIterator<Item = &'a Entry>, t: &Timeline) -> Self {
        let mut spans = t.spans.iter().collect::<Vec<_>>();
        spans.sort_unstable_by_key(|s| (s.start, s.end));
        let mut gaps = Vec::new();
        let mut covered_until = t.start;
        for span in spans {
            if span.start > covered_until {
                gaps.push((covered_until, span.start));
            }
            covered_until = covered_until.max(span.end).max(span.start);
        }
        let duration =
            (t.end - t.start - gaps.iter().map(|(a, b)| b - a).sum::<i64>()).max(1) as f64;
        Self {
            start: t.start,
            duration,
            gaps,
        }
    }
    pub fn position(&self, time: i64) -> f64 {
        let removed = self
            .gaps
            .iter()
            .map(|(a, b)| (time - *a).clamp(0, b - a))
            .sum::<i64>();
        (time - self.start - removed) as f64 / self.duration
    }
    pub fn time_at(&self, fraction: f64) -> i64 {
        let active = (fraction * self.duration) as i64;
        let mut removed = 0;
        for (a, b) in &self.gaps {
            if active >= a - self.start - removed {
                removed += b - a;
            } else {
                break;
            }
        }
        self.start + active + removed
    }
}

pub fn axis_clock(ms: i64, span: i64) -> String {
    let time = clock(Some(ms));
    if span < 10_000 {
        format!("{time}.{:03}", ms.rem_euclid(1000))
    } else {
        time
    }
}
