use std::collections::HashSet;
use zork_client_types::history::Entry;
use zork_observe::ListEdit;

#[derive(Default)]
pub(super) struct HistoryChange {
    pub edits: Vec<ListEdit<Entry>>,
    pub ids: HashSet<String>,
    pub structure: bool,
}

fn value_bytes(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    std::mem::size_of::<Value>()
        + match value {
            Value::String(text) => text.capacity(),
            Value::Array(items) => items.iter().map(value_bytes).sum(),
            Value::Object(fields) => fields
                .iter()
                .map(|(key, value)| {
                    key.capacity() + 4 * std::mem::size_of::<usize>() + value_bytes(value)
                })
                .sum(),
            _ => 0,
        }
}

impl HistoryChange {
    pub fn bytes(&self) -> usize {
        self.ids.iter().map(String::capacity).sum::<usize>()
            + self
                .edits
                .iter()
                .flat_map(|edit| edit.insert.iter())
                .map(|entry| {
                    std::mem::size_of::<Entry>()
                        + entry.id.capacity()
                        + entry.action.capacity()
                        + entry.summary.capacity()
                        + entry.state.capacity()
                        + entry.raw.iter().map(value_bytes).sum::<usize>()
                        + entry.usage.as_ref().map(value_bytes).unwrap_or(0)
                        + entry.model.as_ref().map(String::capacity).unwrap_or(0)
                        + entry
                            .outcome_summary
                            .as_ref()
                            .map(String::capacity)
                            .unwrap_or(0)
                })
                .sum::<usize>()
    }
}
