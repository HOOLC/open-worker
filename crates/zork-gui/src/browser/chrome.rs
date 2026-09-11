//! Native implementation of the user's browser reference: tabs, address bar,
//! and a quiet empty page. Controls use the source icon family and Zork tokens.
use super::{BrowserPanel, BrowserVisibility};
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    desktop::ui,
};
use gpui::{div, prelude::*, px, rgb, Context, Div, FontWeight, Stateful};
use zork_client_core::desktop::browser_engine::Action;
use zork_ui::{components::tooltip, design::CUE_UI};

const TAB_WIDTH: f32 = 156.;
const CONTROL_SIZE: f32 = 28.;
const TAB_ROW_HEIGHT: f32 = CUE_UI.thread.header_height;
const ADDRESS_ROW_HEIGHT: f32 = 40.;
const MENU_SHADOW_COLOR: u32 = 0x24272B14;

fn icon_button(id: impl Into<gpui::ElementId>, path: &'static str, enabled: bool) -> Stateful<Div> {
    ui::icon_button_sized(id, enabled, ui::IconButtonSize::Compact)
        .child(ui::icon(path, 16.).text_color(rgb(CUE_UI.palette.subtle)))
}
fn hint(
    button: Stateful<Div>,
    id: &'static str,
    label: &'static str,
    enabled: bool,
) -> impl IntoElement {
    tooltip::hint(
        button.automation_enabled(enabled, AutomationRole::Button, label),
        id,
        label,
    )
}
fn menu_item(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    enabled: bool,
) -> Stateful<Div> {
    ui::icon_button(id, enabled)
        .w_full()
        .justify_start()
        .px_3()
        .gap_2()
        .child(ui::icon(icon, 16.))
        .child(label)
}

impl BrowserPanel {
    pub(super) fn render_tabs(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = CUE_UI.palette;
        let locale = self.locale;
        let active = self.active_id();
        let blank_tab =
            active.is_none() && (self.active_native.is_none() || self.blank.contains(&self.host));
        let tab_count = self.native_pages.len() + self.tabs.len() + usize::from(blank_tab);
        let tab_width = ((self.width - 168. - 4. * tab_count.saturating_sub(1) as f32)
            / tab_count.max(1) as f32)
            .clamp(96., TAB_WIDTH);
        div()
            .h(px(TAB_ROW_HEIGHT))
            .border_color(rgb(p.border))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pl_2()
            .pr(px(56.))
            .child(
                div()
                    .id("browser-tabs")
                    .min_w_0()
                    .max_w(px((self.width - 168.).max(80.)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_x_scroll()
                    .children(self.native_pages.clone().into_iter().map(|page| {
                        let selected = self.active_native.as_ref() == Some(&page.id);
                        let id = page.id.clone();
                        let close = page.id.clone();
                        div()
                            .id(format!("page-tab-{id}"))
                            .w(px(tab_width))
                            .h(px(CONTROL_SIZE))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pl_2()
                            .pr_1()
                            .rounded(px(8.))
                            .text_size(px(13.))
                            .text_color(rgb(if selected { p.text } else { p.muted }))
                            .when(selected, |v| {
                                v.bg(rgb(p.prompt)).font_weight(FontWeight::MEDIUM)
                            })
                            .hover(|v| v.bg(rgb(p.prompt)))
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .on_click(
                                cx.listener(move |v, _, _, cx| {
                                    v.select_native_page(id.clone(), cx)
                                }),
                            )
                            .child(ui::icon(page.icon, 16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(page.title.clone()),
                            )
                            .child(
                                icon_button(format!("page-close-{close}"), "cue/x.svg", true)
                                    .size(px(24.))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        cx.stop_propagation();
                                        v.close_native_page(&close, cx);
                                        cx.emit(super::NativePageClosed(close.clone()));
                                    }))
                                    .automation(
                                        AutomationRole::Button,
                                        format!(
                                            "{} {}",
                                            locale.text("browser_close_tab"),
                                            page.title
                                        ),
                                    ),
                            )
                            .automation(AutomationRole::Button, page.title)
                    }))
                    .children(self.tabs.clone().into_iter().map(|tab| {
                        let id = tab.id.clone();
                        let close = id.clone();
                        let selected = self.active_native.is_none() && active.as_ref() == Some(&id);
                        let title = if tab.title.is_empty() {
                            tab.url
                        } else {
                            tab.title
                        };
                        div()
                            .id(format!("browser-tab-{id}"))
                            .w(px(tab_width))
                            .h(px(CONTROL_SIZE))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pl_2()
                            .pr_1()
                            .rounded(px(8.))
                            .text_size(px(13.))
                            .text_color(rgb(if selected { p.text } else { p.muted }))
                            .when(selected, |v| {
                                v.bg(rgb(p.prompt)).font_weight(FontWeight::MEDIUM)
                            })
                            .hover(|v| v.bg(rgb(p.prompt)))
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(|v| v.bg(rgb(p.selected)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |v, _, _, cx| v.select(id.clone(), cx)))
                            .child(
                                ui::icon("browser/globe.svg", 16.).text_color(rgb(if selected {
                                    p.text
                                } else {
                                    p.muted
                                })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(title.clone()),
                            )
                            .child(
                                icon_button(format!("browser-close-{close}"), "cue/x.svg", true)
                                    .size(px(24.))
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        cx.stop_propagation();
                                        v.command(
                                            Action::Close {
                                                tab_id: close.clone(),
                                            },
                                            cx,
                                        );
                                    }))
                                    .automation(
                                        AutomationRole::Button,
                                        format!("{} {title}", locale.text("browser_close_tab")),
                                    ),
                            )
                            .automation(
                                AutomationRole::Button,
                                format!("{} {title}", locale.text("browser_tab")),
                            )
                    }))
                    .when(blank_tab, |v| {
                        v.child(
                            div()
                                .id("browser-blank-tab")
                                .w(px(tab_width))
                                .h(px(CONTROL_SIZE))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .pl_2()
                                .pr_1()
                                .rounded(px(8.))
                                .when(self.active_native.is_none(), |v| v.bg(rgb(p.prompt)))
                                .cursor_pointer()
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.show_web_page(cx);
                                    cx.notify();
                                }))
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(ui::icon("browser/globe.svg", 16.).text_color(rgb(p.text)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(locale.text("browser_new_tab")),
                                )
                                .child(
                                    icon_button("browser-close-blank", "cue/x.svg", true)
                                        .size(px(24.))
                                        .on_click(cx.listener(|v, _, _, cx| {
                                            cx.stop_propagation();
                                            v.blank.remove(&v.host);
                                            if let Some(page) = v.native_pages.last() {
                                                v.select_native_page(page.id.clone(), cx);
                                            } else if let Some(tab) = v.tabs.last() {
                                                v.select(tab.id.clone(), cx);
                                            } else {
                                                v.toggle(cx);
                                            }
                                        }))
                                        .automation(
                                            AutomationRole::Button,
                                            locale.text("browser_close_tab"),
                                        ),
                                ),
                        )
                    }),
            )
            .child(hint(
                icon_button("browser-new-tab", "cue/plus.svg", true).on_click(cx.listener(
                    |v, _, window, cx| {
                        v.show_web_page(cx);
                        v.stop_viewport();
                        v.worker.clear_selection(&v.host);
                        v.blank.insert(v.host.clone());
                        v.selected.remove(&v.host);
                        v.frame = None;
                        v.sequence = 0;
                        v.generation += 1;
                        v.inspecting = false;
                        v.menu_open = false;
                        v.error = None;
                        v.sync_address(cx);
                        window.focus(&v.address.read(cx).focus_handle(), cx);
                        cx.notify();
                    },
                )),
                "browser-new-tab",
                locale.text("browser_new_tab"),
                true,
            ))
            .child(div().flex_1())
            .child(hint(
                icon_button(
                    "browser-expand",
                    if self.expanded {
                        "browser/restore.svg"
                    } else {
                        "browser/expand.svg"
                    },
                    true,
                )
                .on_click(cx.listener(|v, _, _, cx| {
                    v.expanded = !v.expanded;
                    v.menu_open = false;
                    cx.emit(BrowserVisibility);
                    cx.notify();
                })),
                "browser-expand",
                locale.text(if self.expanded {
                    "browser_restore"
                } else {
                    "browser_expand"
                }),
                true,
            ))
    }

    pub(super) fn render_navigation(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = CUE_UI.palette;
        let locale = self.locale;
        let has_tab = self.active().is_some();
        let loading = self.active().is_some_and(|tab| tab.loading);
        let invalid = self.error.is_some();
        let address_focus = self.address.read(cx).focus_handle();
        div()
            .h(px(ADDRESS_ROW_HEIGHT))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_color(rgb(p.border))
            .child(hint(
                icon_button("browser-back", "cue/arrow-left.svg", has_tab).when(has_tab, |v| {
                    v.on_click(cx.listener(|v, _, _, cx| v.nav("back", cx)))
                }),
                "browser-back",
                locale.text("browser_back"),
                has_tab,
            ))
            .child(hint(
                icon_button("browser-forward", "cue/arrow-right.svg", has_tab).when(has_tab, |v| {
                    v.on_click(cx.listener(|v, _, _, cx| v.nav("forward", cx)))
                }),
                "browser-forward",
                locale.text("browser_forward"),
                has_tab,
            ))
            .child(hint(
                icon_button(
                    "browser-reload",
                    if loading {
                        "cue/x.svg"
                    } else {
                        "cue/reload.svg"
                    },
                    true,
                )
                .on_click(cx.listener(move |v, _, _, cx| {
                    if has_tab {
                        v.nav(if loading { "stop" } else { "reload" }, cx);
                    } else {
                        v.error = None;
                        cx.notify();
                    }
                })),
                "browser-reload",
                locale.text(if loading {
                    "browser_stop"
                } else {
                    "browser_reload"
                }),
                true,
            ))
            .child(
                div()
                    .id("browser-address")
                    .flex_1()
                    .min_w_0()
                    .h(px(CONTROL_SIZE))
                    .flex()
                    .items_center()
                    .pl_2()
                    .pr_1()
                    .gap_1()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(if invalid { p.danger } else { p.border_strong }))
                    .bg(rgb(p.canvas))
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .track_focus(&address_focus)
                    .focus(move |v| {
                        v.border_color(rgb(if invalid {
                            p.danger
                        } else {
                            ui::FIELD_FOCUS_BORDER
                        }))
                    })
                    .when(!invalid, |v| {
                        v.hover(|v| v.border_color(rgb(ui::FIELD_HOVER_BORDER)))
                    })
                    .on_click(cx.listener(|v, _, window, cx| {
                        v.menu_open = false;
                        window.focus(&v.address.read(cx).focus_handle(), cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(20.))
                            .overflow_hidden()
                            .child(self.address.clone()),
                    )
                    .child(
                        icon_button("browser-go", "browser/go.svg", true)
                            .size(px(22.))
                            .on_click(cx.listener(|v, _, _, cx| v.submit_address(cx)))
                            .automation(AutomationRole::Button, locale.text("browser_go")),
                    )
                    .automation(AutomationRole::TextInput, locale.text("browser_address")),
            )
            .child(hint(
                icon_button("browser-downloads", "browser/download.svg", true).on_click(
                    cx.listener(|v, _, _, cx| {
                        v.menu_open = false;
                        if let Ok(rx) = v.worker.submit(|b| b.open_downloads()) {
                            v.await_result(rx, |_, _, _| {}, cx);
                        }
                    }),
                ),
                "browser-downloads",
                locale.text("browser_downloads"),
                true,
            ))
            .child(
                icon_button("browser-more", "browser/more.svg", true)
                    .when(self.menu_open, |v| v.bg(rgb(p.prompt)))
                    .on_click(cx.listener(|v, _, window, cx| {
                        v.menu_open = !v.menu_open;
                        if v.menu_open {
                            window.focus(&v.menu_focus, cx);
                        } else {
                            window.focus(&v.focus, cx);
                        }
                        cx.notify();
                    }))
                    .automation(AutomationRole::Button, locale.text("browser_more")),
            )
    }

    pub(super) fn render_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = CUE_UI.palette;
        let locale = self.locale;
        let granted = self.grants.contains_key(&self.host);
        let grant_available = self.connection.is_some();
        let has_tab = self.active().is_some();
        let can_inspect = has_tab && self.frame.is_some();
        let grant_label = locale.text(if self.grant_connected {
            "browser_granted"
        } else if granted {
            "browser_connecting"
        } else {
            "browser_grant"
        });
        div()
            .id("browser-menu")
            .absolute()
            .top(px(TAB_ROW_HEIGHT + ADDRESS_ROW_HEIGHT + 4.))
            .right_2()
            .w(px((self.width - 16.).min(248.)))
            .flex()
            .flex_col()
            .gap_1()
            .p_1()
            .occlude()
            .rounded(px(ui::MENU_RADIUS))
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.canvas))
            .text_size(px(12.))
            .shadow(vec![gpui::BoxShadow::new(
                px(0.),
                px(4.),
                gpui::rgba(MENU_SHADOW_COLOR).into(),
            )
            .blur_radius(px(12.))])
            .track_focus(&self.menu_focus)
            .on_mouse_down_out(cx.listener(|v, e: &gpui::MouseDownEvent, _, cx| {
                if e.button == gpui::MouseButton::Left
                    && e.position.y > px(TAB_ROW_HEIGHT + ADDRESS_ROW_HEIGHT)
                {
                    v.menu_open = false;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|v, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    v.menu_open = false;
                    window.focus(&v.focus, cx);
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(
                menu_item(
                    "browser-agent-grant",
                    "icons/permission.svg",
                    grant_label,
                    grant_available,
                )
                .child(div().flex_1())
                .when(granted, |v| v.child(ui::icon("icons/check.svg", 14.)))
                .when(grant_available, |v| {
                    v.on_click(cx.listener(|v, _, _, cx| {
                        if v.grants.remove(&v.host).is_none() {
                            if let Some((client, session)) = &v.connection {
                                v.grants.insert(
                                    v.host.clone(),
                                    super::super::bridge::Grant::start(
                                        v.worker.clone(),
                                        client.clone(),
                                        session.clone(),
                                        v.host.clone(),
                                        cx,
                                    ),
                                );
                            }
                        }
                        v.menu_open = false;
                        cx.notify();
                    }))
                })
                .automation_enabled(
                    grant_available,
                    AutomationRole::Button,
                    format!("{grant_label} · {}", locale.text("browser_grant_hint")),
                ),
            )
            .child(
                menu_item(
                    "browser-inspect",
                    "icons/review.svg",
                    locale.text(if self.inspecting {
                        "browser_inspect_cancel"
                    } else {
                        "browser_inspect"
                    }),
                    can_inspect,
                )
                .when(can_inspect, |v| {
                    v.on_click(cx.listener(|v, _, window, cx| {
                        v.inspecting = !v.inspecting;
                        v.menu_open = false;
                        if v.inspecting {
                            window.focus(&v.focus, cx);
                        }
                        cx.notify();
                    }))
                })
                .automation_enabled(
                    can_inspect,
                    AutomationRole::Button,
                    locale.text("browser_inspect_hint"),
                ),
            )
            .child(div().h(px(1.)).mx_2().my_1().bg(rgb(p.border)))
            .child(
                menu_item(
                    "browser-handoff",
                    "browser/go.svg",
                    locale.text("browser_handoff"),
                    has_tab,
                )
                .when(has_tab, |v| {
                    v.on_click(cx.listener(|v, _, window, cx| {
                        v.grants.remove(&v.host);
                        v.menu_open = false;
                        v.inspecting = false;
                        window.focus(&v.focus, cx);
                        cx.notify();
                    }))
                })
                .automation_enabled(
                    has_tab,
                    AutomationRole::Button,
                    locale.text("browser_handoff"),
                ),
            )
    }

    pub(super) fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = CUE_UI.palette;
        let loading = self.active().is_some() || self.busy > 0;
        if !loading && !self.applications.is_empty() {
            let applications = self.applications.clone();
            let locale = self.locale;
            return div()
                .absolute()
                .inset_0()
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(ui::text_role(
                    locale.text("applications"),
                    zork_ui::design::TextRole::SectionTitle,
                ))
                .child(
                    gpui::uniform_list(
                        "published-applications",
                        applications.len(),
                        cx.processor(move |_: &mut Self, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| {
                                    let app = &applications[index];
                                    let url = app.page.url.clone();
                                    let meta = if app.offline {
                                        format!(
                                            "{} · {}",
                                            app.device_name,
                                            locale.text("application_offline")
                                        )
                                    } else if app.page.description.is_empty() {
                                        app.device_name.clone()
                                    } else {
                                        app.page.description.clone()
                                    };
                                    div().h(px(56.)).pb_2().child(
                                        zork_ui::components::attachments::content_row(
                                            format!("application-{}", app.page.id),
                                            "browser/globe.svg",
                                            app.page.title.clone(),
                                            meta,
                                            cx,
                                            move |panel, cx| {
                                                panel.open_shared_link(url.clone(), cx)
                                            },
                                        ),
                                    )
                                })
                                .collect()
                        }),
                    )
                    .flex_1()
                    .min_h_0()
                    .w_full(),
                );
        }
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .w_full()
                    .max_w(px(280.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .when(loading, |v| {
                        v.child(zork_ui::components::loading::indicator(
                            "browser-loading",
                            24.,
                        ))
                    })
                    .when(!loading, |v| {
                        v.child(ui::icon("browser/globe.svg", 32.).text_color(rgb(p.muted)))
                    })
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text))
                            .child(self.locale.text(if loading {
                                "browser_loading"
                            } else {
                                "applications"
                            })),
                    )
                    .when(!loading, |v| {
                        v.child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(20.))
                                .text_center()
                                .text_color(rgb(p.muted))
                                .child(self.locale.text("applications_empty")),
                        )
                    }),
            )
    }
}
