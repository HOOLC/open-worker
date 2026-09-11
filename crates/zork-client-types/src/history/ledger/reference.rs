// Independent full-replay oracle captured before the incremental migration.
use super::super::{number, string, summary, Entry, Record};
use std::collections::HashMap;

/// Replay borrowed records without materializing/copying an immutable cache.
pub fn entries_iter<'a>(records: impl Iterator<Item = &'a Record> + Clone) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut selected_model: Option<String> = None;
    for record in records.clone() {
        let v = &record.event;
        let kind = v["kind"].as_str().unwrap_or("unknown");
        if matches!(kind, "session_created" | "selection_changed") {
            selected_model = v["selection"]["model"].as_str().map(str::to_owned);
            continue;
        }
        let (id, lane, action, detail, start, end, state) = match kind {
            "input_appended" => (
                record.event_id.clone(),
                0,
                "input".into(),
                string(&v["input"], "content"),
                number(&v["input"], "received_at_ms"),
                number(&v["input"], "received_at_ms"),
                "received".into(),
            ),
            "step_started" => (
                format!("model:{}", string(v, "step_id")),
                1,
                string(v, "purpose"),
                String::new(),
                number(v, "started_at_ms"),
                None,
                "running".into(),
            ),
            "step_completed" => (
                format!("model:{}", string(v, "step_id")),
                1,
                string(v, "purpose"),
                string(v, "assistant_text"),
                None,
                number(v, "completed_at_ms"),
                "succeeded".into(),
            ),
            "step_failed" => (
                format!("model:{}", string(v, "step_id")),
                1,
                "conversation".into(),
                string(&v["error"], "message"),
                None,
                number(v, "failed_at_ms"),
                "failed".into(),
            ),
            "step_interrupted" => (
                format!("model:{}", string(v, "step_id")),
                1,
                "conversation".into(),
                string(v, "reason"),
                None,
                number(v, "interrupted_at_ms"),
                "interrupted".into(),
            ),
            "tool_result" => {
                let r = &v["result"];
                (
                    format!("tool:{}", string(r, "invocation_id")),
                    2,
                    string(r, "tool"),
                    summary(&r["data"]),
                    None,
                    number(r, "finished_at_ms"),
                    string(r, "outcome"),
                )
            }
            "session_created"
            | "selection_changed"
            | "context_configured"
            | "turn_started"
            | "turn_finished"
            | "auto_wait_ended"
            | "tool_cancel_requested"
            | "turn_cancel_requested"
            | "deadline_reached" => continue,
            _ => (
                record.event_id.clone(),
                0,
                kind.into(),
                summary(v),
                ["applied_at_ms", "failed_at_ms", "occurred_at_ms"]
                    .iter()
                    .find_map(|k| number(v, k)),
                None,
                "notice".into(),
            ),
        };
        let ix = *ids.entry(id.clone()).or_insert_with(|| {
            let ix = entries.len();
            entries.push(Entry {
                id,
                lane,
                action: action.clone(),
                summary: String::new(),
                start: None,
                end: None,
                state: String::new(),
                raw: vec![],
                usage: None,
                model: None,
                outcome_summary: None,
            });
            ix
        });
        let e = &mut entries[ix];
        if kind == "step_started" {
            e.model = selected_model.clone();
        }
        if kind == "tool_result" {
            e.outcome_summary = Some(summary(&v["result"]["data"]));
        }
        if start.is_some() {
            e.start = start;
        }
        if end.is_some() {
            e.end = end;
        }
        if e.state != "succeeded"
            && e.state != "failed"
            && e.state != "cancelled"
            && e.state != "interrupted"
            && e.state != "timed_out"
            || state != "running"
        {
            e.state = state;
        }
        if !detail.is_empty() && !(lane == 2 && e.raw.iter().any(|r| r.get("arguments").is_some()))
        {
            e.summary = detail;
        }
        if action != "conversation" && !action.is_empty() {
            e.action = action;
        }
        if let Some(usage) = v.get("usage").filter(|v| !v.is_null()).or_else(|| {
            (kind == "step_failed")
                .then(|| &v["error"]["usage"])
                .filter(|v| !v.is_null())
        }) {
            e.usage = Some(usage.clone());
        }
        e.raw
            .push(serde_json::to_value(record).expect("history record is JSON"));
        if kind == "step_completed" {
            for call in v["invocations"].as_array().into_iter().flatten() {
                let id = format!("tool:{}", string(call, "invocation_id"));
                let ix = *ids.entry(id.clone()).or_insert_with(|| {
                    let ix = entries.len();
                    entries.push(Entry {
                        id,
                        lane: 2,
                        action: string(call, "tool"),
                        summary: String::new(),
                        start: None,
                        end: None,
                        state: "running".into(),
                        raw: vec![],
                        usage: None,
                        model: None,
                        outcome_summary: None,
                    });
                    ix
                });
                let e = &mut entries[ix];
                e.start = number(call, "started_at_ms");
                e.summary = summary(&call["arguments"]);
                e.raw.insert(0, call.clone());
            }
        }
    }
    project_wait_spans(records, &ids, &mut entries);
    // Sort by measured start, falling back to observed completion for orphan results.
    entries.sort_by_key(|e| e.start.or(e.end).unwrap_or(i64::MAX));
    entries
}

/// `wait` returns its deadline immediately. Its tool result is not the end of
/// the session pause. Join the later observed wake/deadline to the same span.
fn project_wait_spans<'a>(
    records: impl Iterator<Item = &'a Record>,
    ids: &HashMap<String, usize>,
    entries: &mut [Entry],
) {
    let mut active: Option<usize> = None;
    for record in records {
        let v = &record.event;
        let kind = v["kind"].as_str().unwrap_or_default();
        if kind == "tool_result"
            && v["result"]["tool"] == "wait"
            && v["result"]["outcome"] == "succeeded"
            && v["result"]["data"]["until_ms"].as_i64().is_some()
        {
            let key = format!("tool:{}", string(&v["result"], "invocation_id"));
            if let Some(&index) = ids.get(&key) {
                let start = number(&v["result"], "finished_at_ms");
                if let Some(previous) = active.replace(index) {
                    entries[previous].end = start;
                    entries[previous].state = "interrupted".into();
                }
                entries[index].start = start.or(entries[index].start);
                entries[index].end = None;
                entries[index].state = "running".into();
            }
            continue;
        }
        let Some(index) = active else { continue };
        let wake = match kind {
            "deadline_reached"
                if v["deadline"]["kind"] == "wait"
                    && format!("tool:{}", string(&v["deadline"], "invocation_id"))
                        == entries[index].id =>
            {
                number(v, "reached_at_ms").map(|time| (time, "succeeded"))
            }
            "step_started"
                if v["consumed_inputs"]
                    .as_array()
                    .is_some_and(|inputs| !inputs.is_empty()) =>
            {
                number(v, "started_at_ms").map(|time| (time, "succeeded"))
            }
            "turn_finished" => number(v, "finished_at_ms").map(|time| (time, "interrupted")),
            _ => None,
        };
        if let Some((time, state)) = wake {
            entries[index].end = Some(time);
            entries[index].state = state.into();
            entries[index]
                .raw
                .push(serde_json::to_value(record).expect("history record is JSON"));
            active = None;
        }
    }
}
