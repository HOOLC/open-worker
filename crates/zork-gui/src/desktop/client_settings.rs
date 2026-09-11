//! Device-local appearance and account preferences.
use super::{ui, DesktopRoot};
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    i18n::Locale,
};
use gpui::{div, prelude::*, px, rgb, Context, Div};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Appearance,
    Notifications,
    Account,
}
#[derive(Default)]
pub struct State {
    pub page: Page,
    pub locale: Locale,
    pub message_preview_height: u32,
    pub(super) message_preview_drag: Option<(f32, u32)>,
    pub(super) message_preview_draft: Option<u32>,
    pub account_available: bool,
    pub notification_permission: super::notifications::Permission,
    pub notification_error: Option<String>,
    pub notification_busy: bool,
}

pub(crate) use zork_client_core::preferences::{MESSAGE_PREVIEW_MAX, MESSAGE_PREVIEW_MIN};

fn dragged_preview_height(start: u32, delta: f32) -> u32 {
    (start as f32 + delta)
        .round()
        .clamp(MESSAGE_PREVIEW_MIN as f32, MESSAGE_PREVIEW_MAX as f32) as u32
}

pub(crate) fn load_message_preview_height(store: &super::store::ClientStore) -> u32 {
    zork_client_core::preferences::read(store).message_preview_height
}

pub(crate) fn message_preview_limit(height: u32, available: f32) -> f32 {
    if height == 0 {
        (available * 0.45).clamp(80., 240.)
    } else {
        height as f32
    }
}

impl DesktopRoot {
    fn save_message_preview_height(&mut self, height: u32, cx: &mut Context<Self>) {
        self.client_settings.message_preview_draft = None;
        self.client_settings.message_preview_drag = None;
        if let Err(error) = self.source.save_message_preview_height(height) {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    pub(super) fn drag_message_preview(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some((start_y, start_height)) = self.client_settings.message_preview_drag {
            let height = dragged_preview_height(start_height, y - start_y);
            if self.client_settings.message_preview_draft != Some(height) {
                self.client_settings.message_preview_draft = Some(height);
                cx.notify();
            }
        }
    }

    pub(super) fn finish_message_preview_drag(&mut self, cx: &mut Context<Self>) {
        if self.client_settings.message_preview_drag.is_some() {
            if let Some(height) = self.client_settings.message_preview_draft {
                self.save_message_preview_height(height, cx);
            }
        }
    }

    fn client_locale(&self) -> Locale {
        self.client_settings.locale
    }
    pub(super) fn render_client_settings_navigation(
        &self,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let locale = self.client_locale();
        let mut pages = vec![
            (Page::Appearance, "client_appearance"),
            (Page::Notifications, "client_notifications"),
        ];
        if self.client_settings.account_available || self.identity.is_some() {
            pages.push((Page::Account, "client_account"));
        }
        self.settings_tabs
            .section(
                "client-settings-heading",
                locale.text("client_settings_title"),
            )
            .children(pages.into_iter().map(|(page, key)| {
                self.settings_tabs
                    .tab(key.into(), selected && self.client_settings.page == page)
                    .child(locale.text(key))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.management_tab = 4;
                        view.client_settings.page = page;
                        if page == Page::Notifications {
                            view.refresh_notification_permission(cx);
                        }
                        cx.notify();
                    }))
                    .automation(AutomationRole::Button, locale.text(key))
            }))
    }

    pub(super) fn render_client_settings(&self, cx: &mut Context<Self>) -> Div {
        let locale = self.client_locale();
        let t = |key| locale.text(key);
        let p = crate::design::CUE_UI.palette;
        let state = &self.client_settings;
        let title = match state.page {
            Page::Appearance => "client_appearance",
            Page::Notifications => "client_notifications",
            Page::Account => "client_account",
        };
        let content = match state.page {
            Page::Appearance => {
                let height = state.message_preview_draft.unwrap_or_else(|| {
                    if state.message_preview_height == 0 {
                        240
                    } else {
                        state.message_preview_height
                    }
                });
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(zork_ui::settings::row(
                        t("client_message_preview_height"),
                        t("client_message_preview_height_detail"),
                        ui::button(
                            "message-preview-reset",
                            t("client_message_preview_auto"),
                            false,
                            true,
                        )
                        .on_click(
                            cx.listener(|view, _, _, cx| view.save_message_preview_height(0, cx)),
                        )
                        .automation(AutomationRole::Button, t("client_message_preview_auto")),
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(p.muted))
                            .child(format!("{height} px")),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .border_1()
                            .border_color(rgb(p.border))
                            .rounded_lg()
                            .overflow_hidden()
                            .child(
                                div()
                                    .h(px(height as f32))
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .p_4()
                                            .text_size(px(13.))
                                            .line_height(px(20.))
                                            .child(t("client_message_preview_sample").repeat(8)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("message-preview-resize")
                                    .h(px(20.))
                                    .flex_shrink_0()
                                    .w_full()
                                    .border_t_1()
                                    .border_color(rgb(p.border))
                                    .cursor_row_resize()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div().w(px(40.)).h(px(3.)).rounded_full().bg(rgb(p.muted)),
                                    )
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |view, event: &gpui::MouseDownEvent, _, cx| {
                                                view.client_settings.message_preview_drag =
                                                    Some((event.position.y.as_f32(), height));
                                                view.client_settings.message_preview_draft =
                                                    Some(height);
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .automation(
                                        AutomationRole::Button,
                                        t("client_message_preview_drag"),
                                    ),
                            ),
                    )
            }
            Page::Notifications => self.render_notification_settings(cx),
            Page::Account => self.render_account(cx),
        };
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(ui::page_title(t(title)))
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zork_client_core::preferences::MESSAGE_PREVIEW_HEIGHT_KEY;
    #[test]
    fn message_preview_preferences_survive_reopening_and_reject_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        {
            let store = super::super::store::ClientStore::open(directory.path()).unwrap();
            assert_eq!(load_message_preview_height(&store), 0);
            store
                .put("device", MESSAGE_PREVIEW_HEIGHT_KEY, &277_u32)
                .unwrap();
        }
        let store = super::super::store::ClientStore::open(directory.path()).unwrap();
        assert_eq!(load_message_preview_height(&store), 277);
        store
            .put("device", MESSAGE_PREVIEW_HEIGHT_KEY, &99999_u32)
            .unwrap();
        assert_eq!(load_message_preview_height(&store), 0);
        store
            .put("device", MESSAGE_PREVIEW_HEIGHT_KEY, &"invalid")
            .unwrap();
        assert_eq!(load_message_preview_height(&store), 0);
    }

    #[test]
    fn message_preview_default_preserves_adaptive_height() {
        assert_eq!(message_preview_limit(0, 100.), 80.);
        assert_eq!(message_preview_limit(0, 400.), 180.);
        assert_eq!(message_preview_limit(0, 1000.), 240.);
        assert_eq!(message_preview_limit(480, 1000.), 480.);
    }

    #[test]
    fn preview_drag_is_continuous_and_bounded() {
        assert_eq!(dragged_preview_height(240, 37.), 277);
        assert_eq!(dragged_preview_height(240, -1000.), 80);
        assert_eq!(dragged_preview_height(240, 1000.), 720);
    }
}
