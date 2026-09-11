//! Optional instrumentation and physical-input dispatch; no server or transport.
#[doc(hidden)]
pub mod driver;
#[doc(hidden)]
pub mod element;
pub mod protocol;
pub use element::{AutomationElementExt, AutomationRoot};
pub use protocol::AutomationRole;
