//! Window-level modal surface shared by node settings editors.
use crate::controls as ui;
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    design::CUE_UI,
};
use gpui::{
    div, prelude::*, px, rgb, rgba, App, Context, FocusHandle, FontWeight, MouseButton, Window,
};
use std::rc::Rc;

pub struct ModalState {
    pub focus: FocusHandle,
    active: Option<&'static str>,
    previous: Option<FocusHandle>,
}
impl ModalState {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            active: None,
            previous: None,
        }
    }
    pub fn sync(&mut self, active: Option<&'static str>, window: &mut Window, cx: &mut App) {
        if active == self.active {
            return;
        }
        if active.is_some() {
            if self.active.is_none() {
                self.previous = window.focused(cx);
            }
            window.focus(&self.focus, cx);
        } else if let Some(previous) = self.previous.take() {
            window.focus(&previous, cx);
        } else {
            window.blur();
        }
        self.active = active;
    }
}

/// Keep forward and reverse keyboard navigation within an active modal group.
pub fn cycle_focus(focus: &FocusHandle, backwards: bool, window: &mut Window, cx: &mut App) {
    if backwards {
        window.focus_prev(cx);
    } else {
        window.focus_next(cx);
    }
    if focus.contains_focused(window, cx) {
        return;
    }
    window.focus(focus, cx);
    window.focus_next(cx);
    if backwards {
        let first = window.focused(cx);
        let mut last = first.clone();
        for _ in 0..256 {
            window.focus_next(cx);
            if !focus.contains_focused(window, cx)
                || first.as_ref().is_some_and(|f| f.is_focused(window))
            {
                break;
            }
            last = window.focused(cx);
        }
        if let Some(last) = last {
            window.focus(&last, cx);
        }
    }
}

pub fn modal<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    body: impl IntoElement,
    footer: impl IntoElement,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    modal_surface(
        id,
        title,
        None,
        body,
        Some(footer.into_any_element()),
        notice,
        focus,
        window,
        cx,
        dismissible,
        close,
    )
}
pub fn detail_modal<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    body: impl IntoElement,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    modal_surface(
        id,
        title,
        None,
        body,
        None,
        notice,
        focus,
        window,
        cx,
        dismissible,
        close,
    )
}

struct TitleAction {
    editor: Option<gpui::AnyElement>,
    action: gpui::AnyElement,
}

pub fn detail_modal_with_title_action<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    title_editor: Option<gpui::AnyElement>,
    title_action: impl IntoElement,
    body: impl IntoElement,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    modal_surface(
        id,
        title,
        Some(TitleAction {
            editor: title_editor,
            action: title_action.into_any_element(),
        }),
        body,
        None,
        notice,
        focus,
        window,
        cx,
        dismissible,
        close,
    )
}

pub fn modal_preview<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    body: impl IntoElement,
    footer: Option<gpui::AnyElement>,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    preview_height: Option<f32>,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    modal_preview_with_title_action(
        id,
        title,
        None,
        body,
        footer,
        notice,
        focus,
        window,
        cx,
        dismissible,
        preview_height,
        close,
    )
}

fn modal_preview_with_title_action<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    title_action: Option<TitleAction>,
    body: impl IntoElement,
    footer: Option<gpui::AnyElement>,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    preview_height: Option<f32>,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    let id: gpui::SharedString = id.into();
    let title = title.into();
    let (title_editor, title_action) = title_action
        .map(|item| (item.editor, Some(item.action)))
        .unwrap_or((None, None));
    let has_footer = footer.is_some();
    let p = CUE_UI.palette;
    let mut viewport = window.viewport_size();
    if let Some(height) = preview_height {
        viewport.height = px(height);
    }
    let close = Rc::new(close);
    let escape_close = close.clone();
    let tab_focus = focus.clone();
    let panel = div()
        .id(id.clone())
        .occlude()
        .track_focus(focus)
        .tab_group()
        .tab_index(0)
        .tab_stop(false)
        .w(px(ui::DIALOG_WIDTH).min(viewport.width - px(40.)))
        .max_w_full()
        .max_h(viewport.height - px(40.))
        .flex()
        .flex_col()
        .rounded(px(ui::MODAL_RADIUS))
        .border_0()
        .bg(rgb(p.canvas))
        .shadow(vec![gpui::BoxShadow::new(
            px(0.),
            px(8.),
            rgba(0x24272B1C).into(),
        )
        .blur_radius(px(20.))])
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .capture_key_down(
            cx.listener(move |_view, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "tab" => {
                        cycle_focus(&tab_focus, event.keystroke.modifiers.shift, window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }),
        )
        .on_key_down(
            cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if dismissible {
                        escape_close(view, window, cx);
                    }
                    cx.stop_propagation();
                }
            }),
        )
        .child(
            div()
                .h(px(76.))
                .flex_shrink_0()
                .px(px(24.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .mr_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(title_editor.unwrap_or_else(|| {
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(20.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title.clone())
                                .into_any_element()
                        }))
                        .when_some(title_action, |v, action| v.child(action)),
                )
                .child(
                    ui::icon_button(format!("{id}-close"), dismissible)
                        .w(px(ui::CONTROL_HEIGHT))
                        .h(px(ui::CONTROL_HEIGHT))
                        .px_0()
                        .border_0()
                        .child(ui::icon("icons/x.svg", 12.))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            if dismissible {
                                close(view, window, cx);
                            }
                        }))
                        .automation_enabled(
                            dismissible,
                            AutomationRole::Button,
                            format!("关闭{title}"),
                        ),
                ),
        )
        .when_some(notice, |v, notice| {
            v.child(div().px(px(24.)).pb_3().child(ui::feedback(notice)))
        })
        .child(
            div()
                .id(format!("{id}-body"))
                .min_h_0()
                .max_h(viewport.height - px(if has_footer { 200. } else { 130. }))
                .overflow_y_scroll()
                .px(px(24.))
                .pt_1()
                .pb(px(if has_footer { 12. } else { 24. }))
                .child(body),
        )
        .when_some(footer, |v, footer| {
            v.child(
                div()
                    .id(format!("{id}-footer"))
                    .flex_shrink_0()
                    .px(px(24.))
                    .pt_5()
                    .pb_6()
                    .child(footer)
                    .automation(AutomationRole::Status, "弹窗操作区"),
            )
        })
        .automation(AutomationRole::Status, title.to_string());
    panel.into_any_element()
}

fn modal_surface<V: 'static>(
    id: impl Into<gpui::SharedString>,
    title: impl Into<gpui::SharedString>,
    title_action: Option<TitleAction>,
    body: impl IntoElement,
    footer: Option<gpui::AnyElement>,
    notice: Option<String>,
    focus: &FocusHandle,
    window: &Window,
    cx: &Context<V>,
    dismissible: bool,
    close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    let id: gpui::SharedString = id.into();
    let close = Rc::new(close);
    let panel_close = close.clone();
    let backdrop_close = close;
    let viewport = window.viewport_size();
    let panel = modal_preview_with_title_action(
        id.clone(),
        title,
        title_action,
        body,
        footer,
        notice,
        focus,
        window,
        cx,
        dismissible,
        None,
        move |v, w, cx| panel_close(v, w, cx),
    );
    let card_bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
    let measured = card_bounds.clone();
    let layer_id = format!("{id}-blur");
    gpui::deferred(
        gpui::anchored()
            .position(gpui::point(px(0.), px(0.)))
            .child(
                div()
                    .id(format!("{id}-backdrop"))
                    .occlude()
                    .w(viewport.width)
                    .h(viewport.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgba(
                        if crate::components::modal_backdrop::available(window) {
                            0
                        } else {
                            0x00000059
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, window, cx| {
                            if dismissible {
                                backdrop_close(view, window, cx);
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div().relative().child(panel).child(
                            gpui::canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
                                .absolute()
                                .inset_0(),
                        ),
                    )
                    .child(crate::components::modal_backdrop::ModalBackdrop {
                        id: layer_id,
                        card: card_bounds,
                    }),
            ),
    )
    .with_priority(100)
    .into_any_element()
}
