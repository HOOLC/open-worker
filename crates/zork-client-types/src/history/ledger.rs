//! Incremental execution ledger. The live owner is the core History controller;
//! this pure reducer is also used by the complete-snapshot fixture API.
use super::{number, string, summary, Entry, Record};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Bound::{Excluded, Included, Unbounded},
    sync::Arc,
};

/// Stable sort order: measured time, then the first occurrence in ledger order.
/// Positions do not change when an earlier page is inserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryOrder(i64, i64, usize);

pub struct ProjectedEntry {
    pub order: EntryOrder,
    pub entry: Entry,
}

#[derive(Default)]
struct Evidence {
    // Part zero is the record itself; subsequent parts are its invocations.
    records: BTreeMap<(i64, usize), Arc<Record>>,
    waits: BTreeSet<i64>,
}

/// Records must be immutable, unique, and ordered within each page. Deduplication
/// and authoritative page/cursor ownership remain with the core controller.
#[derive(Default)]
pub struct Ledger {
    first: i64,
    next: i64,
    entries: HashMap<String, Evidence>,
    models: BTreeMap<i64, Option<String>>,
    starts: BTreeMap<i64, String>,
    waits: BTreeMap<i64, Arc<Record>>,
    wakes: BTreeMap<i64, Arc<Record>>,
    deadlines: HashMap<String, BTreeMap<i64, Arc<Record>>>,
}

impl Ledger {
    /// Return only entries whose evidence or cross-event dependencies changed.
    /// An earlier model selection can affect a range of existing starts; a wait
    /// boundary affects at most its preceding wait and the new wait itself.
    pub fn ingest(&mut self, records: &[Arc<Record>], older: bool) -> Vec<ProjectedEntry> {
        let previous_first = self.first;
        let count = i64::try_from(records.len()).expect("history page fits i64");
        let first = if older {
            self.first = self
                .first
                .checked_sub(count)
                .expect("history position overflow");
            self.first
        } else {
            let first = self.next;
            self.next = self
                .next
                .checked_add(count)
                .expect("history position overflow");
            first
        };
        let mut dirty = HashSet::new();
        for (offset, record) in records.iter().enumerate() {
            let position = first + offset as i64;
            let v = &record.event;
            let kind = v["kind"].as_str().unwrap_or("unknown");
            if matches!(kind, "session_created" | "selection_changed") {
                let model = v["selection"]["model"].as_str().map(str::to_owned);
                self.models.insert(position, model);
                continue;
            }
            if let Some(id) = primary_id(record) {
                if kind == "step_started" {
                    self.starts.insert(position, id.clone());
                }
                self.entries
                    .entry(id.clone())
                    .or_default()
                    .records
                    .insert((position, 0), record.clone());
                dirty.insert(id);
            }
            if kind == "step_completed" {
                for (part, call) in v["invocations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let id = format!("tool:{}", string(call, "invocation_id"));
                    self.entries
                        .entry(id.clone())
                        .or_default()
                        .records
                        .insert((position, part + 1), record.clone());
                    dirty.insert(id);
                }
            }
            if is_wait(record) {
                let id = wait_id(record);
                self.entries
                    .get_mut(&id)
                    .expect("wait has a tool entry")
                    .waits
                    .insert(position);
                self.waits.insert(position, record.clone());
                dirty.insert(id);
                self.dirty_previous_wait(position, &mut dirty);
            } else if wake(record).is_some() {
                self.wakes.insert(position, record.clone());
                self.dirty_previous_wait(position, &mut dirty);
            } else if kind == "deadline_reached"
                && v["deadline"]["kind"] == "wait"
                && number(v, "reached_at_ms").is_some()
            {
                let id = format!("tool:{}", string(&v["deadline"], "invocation_id"));
                self.deadlines
                    .entry(id.clone())
                    .or_default()
                    .insert(position, record.clone());
                if self
                    .waits
                    .range(..position)
                    .next_back()
                    .is_some_and(|(_, wait)| wait_id(wait) == id)
                {
                    self.dirty_previous_wait(position, &mut dirty);
                }
            }
        }
        // Only a prepended selection can change an already loaded start's
        // model. Its effect ends at the first existing selection. Resolve this
        // once after indexing the complete page, even if it has many selections.
        // The previously unselected prefix already has model=None, so a missing
        // or explicit-null selection cannot invalidate that prefix.
        if older
            && self
                .models
                .range(..previous_first)
                .next_back()
                .is_some_and(|(_, model)| model.is_some())
        {
            let end = self
                .models
                .range(previous_first..)
                .next()
                .map_or(Unbounded, |(&position, _)| Excluded(position));
            dirty.extend(
                self.starts
                    .range((Included(previous_first), end))
                    .map(|(_, id)| id.clone()),
            );
        }
        let mut changes = dirty
            .into_iter()
            .map(|id| self.project(&id))
            .collect::<Vec<_>>();
        changes.sort_unstable_by_key(|change| change.order);
        changes
    }

    fn dirty_previous_wait(&self, position: i64, dirty: &mut HashSet<String>) {
        if let Some((&start, record)) = self.waits.range(..position).next_back() {
            let id = wait_id(record);
            let between = (Excluded(start), Excluded(position));
            if self.wakes.range(between).next().is_some()
                || self
                    .deadlines
                    .get(&id)
                    .is_some_and(|items| items.range(between).next().is_some())
            {
                return;
            }
            dirty.insert(id);
        }
    }

    fn project(&self, id: &str) -> ProjectedEntry {
        let evidence = &self.entries[id];
        let mut entry = None;
        for (&(position, part), record) in &evidence.records {
            if part == 0 {
                let model = (record.event["kind"] == "step_started")
                    .then(|| {
                        self.models
                            .range(..position)
                            .next_back()
                            .and_then(|(_, model)| model.clone())
                    })
                    .flatten();
                apply_primary(&mut entry, id, record, model);
            } else {
                let call = &record.event["invocations"][part - 1];
                let entry =
                    entry.get_or_insert_with(|| empty(id, 2, string(call, "tool"), "running"));
                entry.start = number(call, "started_at_ms");
                entry.summary = summary(&call["arguments"]);
                entry.raw.insert(0, call.clone());
            }
        }
        let mut entry = entry.expect("entry has evidence");
        // Match the complete replay's second pass: all base facts are applied
        // before wait overrides and their wake records, including orphan calls.
        for &position in &evidence.waits {
            let record = &self.waits[&position];
            entry.start = number(&record.event["result"], "finished_at_ms").or(entry.start);
            entry.end = None;
            entry.state = "running".into();
            let after = (Excluded(position), Unbounded);
            let next_wait = self.waits.range(after).next().map(|(&p, r)| (p, r, true));
            let next_wake = self.wakes.range(after).next().map(|(&p, r)| (p, r, false));
            let deadline = self
                .deadlines
                .get(id)
                .and_then(|items| items.range(after).next())
                .map(|(&p, r)| (p, r, false));
            if let Some((_, end, interrupted)) = [next_wait, next_wake, deadline]
                .into_iter()
                .flatten()
                .min_by_key(|(p, _, _)| *p)
            {
                if interrupted {
                    entry.end = number(&end.event["result"], "finished_at_ms");
                    entry.state = "interrupted".into();
                } else {
                    let (at, state) = wake(end).unwrap_or_else(|| {
                        (number(&end.event, "reached_at_ms").unwrap(), "succeeded")
                    });
                    entry.end = Some(at);
                    entry.state = state.into();
                    entry
                        .raw
                        .push(serde_json::to_value(end.as_ref()).expect("history record is JSON"));
                }
            }
        }
        let &(position, part) = evidence.records.first_key_value().unwrap().0;
        ProjectedEntry {
            order: EntryOrder(
                entry.start.or(entry.end).unwrap_or(i64::MAX),
                position,
                part,
            ),
            entry,
        }
    }
}

fn primary_id(record: &Record) -> Option<String> {
    let v = &record.event;
    Some(match v["kind"].as_str().unwrap_or("unknown") {
        "step_started" | "step_completed" | "step_failed" | "step_interrupted" => {
            format!("model:{}", string(v, "step_id"))
        }
        "tool_result" => format!("tool:{}", string(&v["result"], "invocation_id")),
        "session_created"
        | "selection_changed"
        | "context_configured"
        | "turn_started"
        | "turn_finished"
        | "auto_wait_ended"
        | "tool_cancel_requested"
        | "turn_cancel_requested"
        | "deadline_reached" => return None,
        _ => record.event_id.clone(),
    })
}

fn empty(id: &str, lane: usize, action: String, state: &str) -> Entry {
    Entry {
        id: id.into(),
        lane,
        action,
        summary: String::new(),
        start: None,
        end: None,
        state: state.into(),
        raw: vec![],
        usage: None,
        model: None,
        outcome_summary: None,
    }
}

fn apply_primary(entry: &mut Option<Entry>, id: &str, record: &Record, model: Option<String>) {
    let v = &record.event;
    let kind = v["kind"].as_str().unwrap_or("unknown");
    let (lane, action, detail, start, end, state) = match kind {
        "input_appended" => (
            0,
            "input".into(),
            string(&v["input"], "content"),
            number(&v["input"], "received_at_ms"),
            number(&v["input"], "received_at_ms"),
            "received".into(),
        ),
        "step_started" => (
            1,
            string(v, "purpose"),
            String::new(),
            number(v, "started_at_ms"),
            None,
            "running".into(),
        ),
        "step_completed" => (
            1,
            string(v, "purpose"),
            string(v, "assistant_text"),
            None,
            number(v, "completed_at_ms"),
            "succeeded".into(),
        ),
        "step_failed" => (
            1,
            "conversation".into(),
            string(&v["error"], "message"),
            None,
            number(v, "failed_at_ms"),
            "failed".into(),
        ),
        "step_interrupted" => (
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
                2,
                string(r, "tool"),
                summary(&r["data"]),
                None,
                number(r, "finished_at_ms"),
                string(r, "outcome"),
            )
        }
        _ => (
            0,
            kind.into(),
            summary(v),
            ["applied_at_ms", "failed_at_ms", "occurred_at_ms"]
                .iter()
                .find_map(|key| number(v, key)),
            None,
            "notice".into(),
        ),
    };
    let e = entry.get_or_insert_with(|| empty(id, lane, action.clone(), ""));
    if kind == "step_started" {
        e.model = model;
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
    if !matches!(
        e.state.as_str(),
        "succeeded" | "failed" | "cancelled" | "interrupted" | "timed_out"
    ) || state != "running"
    {
        e.state = state;
    }
    if !detail.is_empty() && !(lane == 2 && e.raw.iter().any(|r| r.get("arguments").is_some())) {
        e.summary = detail;
    }
    if action != "conversation" && !action.is_empty() {
        e.action = action;
    }
    if let Some(usage) = v.get("usage").filter(|value| !value.is_null()).or_else(|| {
        (kind == "step_failed")
            .then(|| &v["error"]["usage"])
            .filter(|value| !value.is_null())
    }) {
        e.usage = Some(usage.clone());
    }
    e.raw
        .push(serde_json::to_value(record).expect("history record is JSON"));
}

fn is_wait(record: &Record) -> bool {
    let v = &record.event;
    v["kind"] == "tool_result"
        && v["result"]["tool"] == "wait"
        && v["result"]["outcome"] == "succeeded"
        && number(&v["result"]["data"], "until_ms").is_some()
}
fn wait_id(record: &Record) -> String {
    format!("tool:{}", string(&record.event["result"], "invocation_id"))
}
fn wake(record: &Record) -> Option<(i64, &'static str)> {
    let v = &record.event;
    match v["kind"].as_str() {
        Some("step_started")
            if v["consumed_inputs"]
                .as_array()
                .is_some_and(|items| !items.is_empty()) =>
        {
            number(v, "started_at_ms").map(|at| (at, "succeeded"))
        }
        Some("turn_finished") => number(v, "finished_at_ms").map(|at| (at, "interrupted")),
        _ => None,
    }
}

#[cfg(test)]
#[path = "ledger/reference.rs"]
mod reference;
#[cfg(test)]
#[path = "ledger/tests.rs"]
mod tests;
