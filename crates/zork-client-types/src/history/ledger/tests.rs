use super::*;
use serde_json::json;

fn record(id: impl ToString, event: serde_json::Value) -> Arc<Record> {
    Arc::new(Record {
        event_id: id.to_string(),
        event,
        metadata: Default::default(),
    })
}

#[derive(Default)]
struct Model {
    ledger: Ledger,
    records: Vec<Arc<Record>>,
    rows: HashMap<String, ProjectedEntry>,
}
impl Model {
    fn ingest(&mut self, page: &[Arc<Record>], older: bool) -> usize {
        let changes = self.ledger.ingest(page, older);
        let touched = changes.len();
        for change in changes {
            self.rows.insert(change.entry.id.clone(), change);
        }
        if older {
            self.records.splice(0..0, page.iter().cloned());
        } else {
            self.records.extend_from_slice(page);
        }
        let mut ordered = self.rows.values().collect::<Vec<_>>();
        ordered.sort_by_key(|row| row.order);
        let actual = ordered.iter().map(|row| &row.entry).collect::<Vec<_>>();
        let expected = reference::entries_iter(self.records.iter().map(Arc::as_ref));
        assert_eq!(
            actual,
            expected.iter().collect::<Vec<_>>(),
            "{} records, older={older}",
            self.records.len()
        );
        touched
    }
}

#[test]
fn earlier_selection_orphan_calls_and_waits_join_without_replaying_other_rows() {
    let all = [
        record(
            "select",
            json!({"kind":"selection_changed","selection":{"model":"before"}}),
        ),
        record(
            "start",
            json!({"kind":"step_started","step_id":"s","purpose":"compaction","started_at_ms":10}),
        ),
        record(
            "call",
            json!({"kind":"step_completed","step_id":"s","completed_at_ms":20,"invocations":[{"invocation_id":"w","tool":"wait","started_at_ms":20,"arguments":{"seconds":5}},{"invocation_id":"x","tool":"shell.run","started_at_ms":20,"arguments":{"command":"ls"}}]}),
        ),
        record(
            "wait",
            json!({"kind":"tool_result","result":{"invocation_id":"w","tool":"wait","outcome":"succeeded","finished_at_ms":21,"data":{"until_ms":50}}}),
        ),
        record(
            "result",
            json!({"kind":"tool_result","result":{"invocation_id":"x","tool":"shell.run","outcome":"failed","finished_at_ms":25,"data":{"stderr":"failed"}}}),
        ),
        record(
            "stale",
            json!({"kind":"deadline_reached","deadline":{"kind":"wait","invocation_id":"other"},"reached_at_ms":50}),
        ),
        record(
            "wake",
            json!({"kind":"step_started","step_id":"next","started_at_ms":40,"consumed_inputs":["input"]}),
        ),
        record(
            "late",
            json!({"kind":"deadline_reached","deadline":{"kind":"wait","invocation_id":"w"},"reached_at_ms":50}),
        ),
    ];
    let mut model = Model::default();
    model.ingest(&all[4..6], false);
    model.ingest(&all[2..4], true);
    model.ingest(&all[6..], false);
    model.ingest(&all[1..2], true);
    model.ingest(&all[..1], true);
    assert_eq!(model.rows["model:s"].entry.model.as_deref(), Some("before"));
    assert_eq!(model.rows["tool:w"].entry.end, Some(40));
    assert_eq!(model.rows["tool:x"].entry.summary, "ls");
    let quiet = model.ingest(
        &[record("ignored", json!({"kind":"context_configured"}))],
        false,
    );
    assert_eq!(quiet, 0);
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed >> 24
}

fn mixed(seed: &mut u64, i: usize) -> Arc<Record> {
    let group = next(seed) % 17;
    let step = format!("s{group}");
    let call = format!("c{group}");
    let time = next(seed) % 40; // deliberately tied and out of measured-time order
    let at = if time % 7 == 0 {
        serde_json::Value::Null
    } else {
        json!(time)
    };
    let event = match next(seed) % 16 {
        0 => json!({"kind":"session_created","selection":{"model":format!("m{group}")}}),
        1 => {
            json!({"kind":"selection_changed","selection":{"model":if group % 3 == 0 {None} else {Some(format!("m{group}"))}}})
        }
        2 => json!({"kind":"input_appended","input":{"content":"input","received_at_ms":at}}),
        3 | 4 => {
            json!({"kind":"step_started","step_id":step,"started_at_ms":at,"purpose":"conversation","consumed_inputs":if group%2==0 {vec!["input"]} else {vec![]}})
        }
        5 => {
            json!({"kind":"step_failed","step_id":step,"failed_at_ms":at,"error":{"message":"oops","usage":{"input_tokens":group}}})
        }
        6 => {
            json!({"kind":"step_interrupted","step_id":step,"interrupted_at_ms":at,"reason":"cancel"})
        }
        7 => {
            json!({"kind":"step_completed","step_id":step,"completed_at_ms":at,"purpose":"compaction","assistant_text":"result","usage":{"output_tokens":group},"invocations":[{"invocation_id":call,"tool":"shell.run","started_at_ms":at,"arguments":{"command":"echo same name"}},{"invocation_id":format!("other{group}"),"tool":"shell.run","arguments":{"command":"pwd"}}]})
        }
        8 => {
            json!({"kind":"tool_result","result":{"invocation_id":call,"tool":"shell.run","outcome":"failed","finished_at_ms":at,"data":{"stderr":"error"}}})
        }
        9 => {
            json!({"kind":"tool_result","result":{"invocation_id":call,"tool":"wait","outcome":"succeeded","finished_at_ms":at,"data":{"until_ms":100}}})
        }
        10 => {
            json!({"kind":"deadline_reached","deadline":{"kind":"wait","invocation_id":call},"reached_at_ms":at})
        }
        11 => json!({"kind":"turn_finished","finished_at_ms":at}),
        12 => json!({"kind":"future_event","occurred_at_ms":at,"message":"notice"}),
        13 => json!({"kind":"tool_cancel_requested"}),
        14 => json!({"kind":"step_completed","step_id":step,"completed_at_ms":at}),
        _ => {
            json!({"kind":"tool_result","result":{"invocation_id":call,"tool":"shell.run","outcome":"succeeded","finished_at_ms":at,"data":{"stdout":"late"}}})
        }
    };
    record(format!("record{i}"), event)
}

#[test]
fn arbitrary_completion_order_repeated_ids_and_page_boundaries_match_full_replay() {
    for initial in 1..=12 {
        let mut seed = initial;
        let records = (0..400).map(|i| mixed(&mut seed, i)).collect::<Vec<_>>();
        let mut model = Model::default();
        let mut start = 190;
        let mut end = 205;
        model.ingest(&records[start..end], false);
        while start > 0 || end < records.len() {
            let count = (next(&mut seed) % 13 + 1) as usize;
            if start > 0 && (end == records.len() || next(&mut seed) % 2 == 0) {
                let before = start.saturating_sub(count);
                model.ingest(&records[before..start], true);
                start = before;
            } else {
                let after = (end + count).min(records.len());
                model.ingest(&records[end..after], false);
                end = after;
            }
        }
        assert_eq!(
            super::super::entries_iter(records.iter().map(Arc::as_ref)),
            reference::entries_iter(records.iter().map(Arc::as_ref))
        );
    }
}

#[test]
fn one_append_and_far_completion_touch_only_their_entries_in_large_history() {
    let mut ledger = Ledger::default();
    let records = (0..100_000)
        .map(|i| {
            record(
                i,
                json!({"kind":"step_started","step_id":format!("s{i}"),"started_at_ms":i}),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(ledger.ingest(&records, false).len(), 100_000);
    let changed = ledger.ingest(&[
        record("late", json!({"kind":"step_completed","step_id":"s41","completed_at_ms":100001,"assistant_text":"done"})),
        record("new", json!({"kind":"input_appended","input":{"content":"next","received_at_ms":100001}})),
    ], false);
    assert_eq!(
        changed
            .iter()
            .map(|row| row.entry.id.as_str())
            .collect::<Vec<_>>(),
        ["model:s41", "new"]
    );
    let old = ledger.ingest(
        &[record(
            "older",
            json!({"kind":"input_appended","input":{"content":"old","received_at_ms":-1}}),
        )],
        true,
    );
    assert_eq!(old.len(), 1);
    // A page with several selections must resolve the final boundary before
    // invalidating existing starts. Ending in unknown keeps that prefix intact.
    let unknown = ledger.ingest(
        &[
            record(
                "older-model",
                json!({"kind":"selection_changed","selection":{"model":"unused"}}),
            ),
            record(
                "older-clear",
                json!({"kind":"selection_changed","selection":{"model":null}}),
            ),
        ],
        true,
    );
    assert!(unknown.is_empty());
    // An earlier selection legitimately changes every start before the next
    // selection boundary. This fan-out is actual changed output, not a scan
    // performed for unrelated appends.
    let mut ledger = Ledger::default();
    ledger.ingest(&records, false);
    let models = ledger.ingest(
        &[record(
            "model",
            json!({"kind":"selection_changed","selection":{"model":"historical"}}),
        )],
        true,
    );
    assert_eq!(models.len(), 100_000);
    assert!(models
        .iter()
        .all(|row| row.entry.model.as_deref() == Some("historical")));
}
