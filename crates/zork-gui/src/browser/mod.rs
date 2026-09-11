//! Shared native/web page strip backed by the client-owned embedded CEF runtime.
mod bridge;
mod panel;
mod worker;
pub(crate) use panel::PanelMotion;
pub use panel::{
    BrowserPanel, BrowserResized, BrowserSelection, BrowserVisibility, NativePage, NativePageClosed,
};
