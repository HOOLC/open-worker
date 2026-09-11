#[path = "../../../zork-gui/src/components/activity.rs"]
pub mod activity;
#[path = "../../../zork-gui/src/components/brand.rs"]
pub mod brand;
#[path = "../../../zork-gui/src/components/message.rs"]
pub mod message;
#[path = "../../../zork-gui/src/components/selection.rs"]
pub mod selection;
#[path = "../../../zork-gui/src/components/text_input.rs"]
pub mod text_input;
pub fn init(cx: &mut gpui::App) {
    text_input::init(cx);
}
