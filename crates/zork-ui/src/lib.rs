//! Shared GPUI component implementation for the desktop app, browser stories and design examples.
//! Applications provide data and actions; this package does not own Gateway, persistence or account access.
pub mod assets;
pub mod automation;
pub mod comments;
pub mod components;
pub mod controls;
pub mod design;
pub mod history;
pub mod modal;
pub mod navigation;
pub mod network;
pub mod settings;

#[cfg(feature = "stories")]
mod form_story;
#[cfg(feature = "stories")]
mod interaction_story;
#[cfg(feature = "stories")]
pub mod stories;
