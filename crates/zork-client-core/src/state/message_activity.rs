//! Ephemeral per-conversation delivery activity, separate from durable history.
//! Subscribers retain a sequence cursor, so coalesced snapshots preserve the
//! arrival count without retaining an unbounded queue of animation payloads.
use serde::Serialize;
use std::{collections::VecDeque, sync::Arc};

const RECENT_ARRIVALS: usize = 32;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MessageActivity {
    pub(super) sequence: u64,
    recent: Arc<VecDeque<(u64, String)>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MessageArrivals {
    pub count: u64,
    pub ids: Vec<String>,
}

impl MessageActivity {
    pub(super) fn record(&mut self, id: String) {
        self.sequence += 1;
        let recent = Arc::make_mut(&mut self.recent);
        recent.push_back((self.sequence, id));
        while recent.len() > RECENT_ARRIVALS {
            recent.pop_front();
        }
    }

    pub(super) fn since(&self, sequence: u64) -> MessageArrivals {
        MessageArrivals {
            count: self.sequence.saturating_sub(sequence),
            ids: self
                .recent
                .iter()
                .filter(|(at, _)| *at > sequence)
                .map(|(_, id)| id.clone())
                .collect(),
        }
    }
}
