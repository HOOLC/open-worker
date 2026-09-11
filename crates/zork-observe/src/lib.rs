//! Versioned, bounded state delivery shared by native and foreign UI adapters.
//!
//! A notification schedules a read; it is neither the data nor an applied
//! cursor. Only acknowledgement advances the consumer's baseline. A source
//! owns publication, subscriptions do not keep its controller alive, and no
//! platform runtime, timer, network or business rule lives here.
mod list;
mod source;
mod value;
pub use value::{ValueSource, ValueSubscription};

pub use list::{List, ListEdit};
pub use source::{
    Batch, BatchId, Change, Changes, Closed, Cursor, JournalLimits, Readiness, ResetReason,
    Snapshot, Source, Subscription, Topics,
};
