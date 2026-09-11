use serde_json::{json, Value};
use zork_client_types::history::Record;

pub fn record(id: impl ToString, event: Value) -> Record {
    Record {
        event_id: id.to_string(),
        event,
        metadata: Default::default(),
    }
}

pub fn fixture(i: usize) -> Record {
    let group = i / 10;
    let at = i as i64 * 10;
    let step = format!("s{group}");
    let call = format!("c{group}");
    let event = match i % 10 {
        0 => json!({"kind":"selection_changed","selection":{"model":format!("m{}", group % 3)}}),
        1 => json!({"kind":"step_started","step_id":step,"started_at_ms":at}),
        2 => {
            json!({"kind":"step_completed","step_id":step,"completed_at_ms":at,"assistant_text":"mixed history body ".repeat(32),"usage":{"input_tokens":1024,"output_tokens":256},"invocations":[{"invocation_id":call,"tool":"shell.run","started_at_ms":at,"arguments":{"command":"rg -n projection crates"}}]})
        }
        3 => {
            json!({"kind":"tool_result","result":{"invocation_id":call,"tool":"shell.run","outcome":"succeeded","finished_at_ms":at,"data":{"stdout":"command output\n".repeat(32)}}})
        }
        4 => {
            json!({"kind":"input_appended","input":{"content":"follow up request ".repeat(16),"received_at_ms":at}})
        }
        5 => json!({"kind":"step_started","step_id":format!("open{group}"),"started_at_ms":at}),
        6 => {
            json!({"kind":"step_completed","step_id":format!("w{group}"),"completed_at_ms":at,"invocations":[{"invocation_id":format!("wait{group}"),"tool":"wait","started_at_ms":at,"arguments":{"seconds":1}}]})
        }
        7 => {
            json!({"kind":"tool_result","result":{"invocation_id":format!("wait{group}"),"tool":"wait","outcome":"succeeded","finished_at_ms":at,"data":{"until_ms":at+1000}}})
        }
        8 => {
            json!({"kind":"step_started","step_id":format!("next{group}"),"started_at_ms":at,"consumed_inputs":["new"]})
        }
        _ => json!({"kind":"turn_finished","finished_at_ms":at}),
    };
    record(format!("r{i}"), event)
}
