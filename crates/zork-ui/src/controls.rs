//! Shared compact native controls and approved Zork identity assets.
pub use super::modal::{detail_modal, detail_modal_with_title_action, modal, ModalState};
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    components::text_input::ComposerInput,
    design::{TextRole, CUE_UI, FORM, INTERACTION},
};
use gpui::{div, prelude::*, px, rgb, svg, AnimationExt, Div, Entity, FontWeight, Stateful};

pub fn icon(path: &'static str, size: f32) -> gpui::Svg {
    svg()
        .path(path)
        .size(px(size))
        .flex_shrink_0()
        .text_color(rgb(CUE_UI.palette.muted))
}
/// Shared desktop geometry, also exercised by the offscreen visual checks.
pub const SETTINGS_COLUMN_WIDTH: f32 = 790.;
pub const SETTINGS_GUTTER: f32 = 34.;
pub const DIALOG_WIDTH: f32 = 540.;
pub const CONTROL_HEIGHT: f32 = 32.;
pub const FIELD_HEIGHT: f32 = CONTROL_HEIGHT;
pub const BUTTON_HEIGHT: f32 = CONTROL_HEIGHT;
pub const BUTTON_FOCUS_BACKGROUND: u32 = INTERACTION.primary_hover;
pub const DROPDOWN_HEIGHT: f32 = CONTROL_HEIGHT;
pub const BUTTON_RADIUS: f32 = 999.;
pub const FIELD_RADIUS: f32 = 12.;
pub const CARD_RADIUS: f32 = 20.;
pub const ICON_BUTTON_RADIUS: f32 = 12.;
pub const BUTTON_PADDING_X: f32 = 16.;
pub const MODAL_RADIUS: f32 = 32.;
pub const MENU_RADIUS: f32 = 20.;
pub const MENU_OUTSET: f32 = 4.;
pub const MENU_GAP: f32 = 6.;
pub const FIELD_HOVER_BORDER: u32 = FORM.hover_border;
pub const FIELD_FOCUS_BORDER: u32 = FORM.focus_border;

pub fn settings_content(content: impl IntoElement) -> impl IntoElement {
    div()
        .id("desktop-settings-column")
        .w_full()
        .max_w(px(SETTINGS_COLUMN_WIDTH))
        .mx_auto()
        .px(px(SETTINGS_GUTTER))
        .pt(px(30.))
        .pb_8()
        .child(content)
        .automation(AutomationRole::Status, "设置内容列")
}
pub fn text_role(text: impl Into<gpui::SharedString>, role: TextRole) -> Div {
    let (size, line_height, weight) = role.metrics();
    div()
        .text_size(px(size))
        .line_height(px(line_height))
        .font_weight(FontWeight(weight as f32))
        .text_color(rgb(match role {
            TextRole::Description | TextRole::Metadata | TextRole::Label => CUE_UI.palette.muted,
            _ => CUE_UI.palette.text,
        }))
        .child(text.into())
}
pub fn page_title(title: impl Into<gpui::SharedString>) -> Div {
    text_role(title, TextRole::PageTitle)
}
pub fn action_link(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .min_h(px(CONTROL_HEIGHT))
        .flex()
        .items_center()
        .text_size(px(12.))
        .text_color(rgb(if enabled {
            CUE_UI.palette.muted
        } else {
            CUE_UI.palette.subtle
        }))
        .when(enabled, |v| {
            v.focusable()
                .tab_stop(true)
                .cursor_pointer()
                .hover(|v| v.text_color(rgb(CUE_UI.palette.text)).underline())
                .focus_visible(|v| v.text_color(rgb(CUE_UI.palette.text)).underline())
                .active(|v| v.text_color(rgb(CUE_UI.palette.text)))
        })
        .when(!enabled, |v| v.cursor_default())
        .child(text.into())
}
pub fn page_action(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
) -> Stateful<Div> {
    button(id, "", false, true)
        .h(px(BUTTON_HEIGHT))
        .text_size(px(11.))
        .rounded(px(BUTTON_RADIUS))
        .pl(px(12.))
        .pr(px(BUTTON_PADDING_X))
        .gap_1()
        .child(icon("icons/plus.svg", 14.))
        .child(text.into())
}

pub fn heading(
    title: impl Into<gpui::SharedString>,
    description: impl Into<gpui::SharedString>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pb_6()
        .child(
            div()
                .text_size(px(22.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(rgb(CUE_UI.palette.muted))
                .child(description.into()),
        )
}
pub fn label(text: impl Into<gpui::SharedString>) -> Div {
    text_role(text, TextRole::Label)
}
pub fn form_field(title: impl Into<gpui::SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w_full()
        .min_w_0()
        .child(label(title))
        .child(control)
}
pub fn section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .py_5()
        .border_t_1()
        .border_color(rgb(CUE_UI.palette.border))
}
/// Composite disabled paint once against the canvas. Applying GPUI opacity to
/// each overlapping primitive separately darkens the border and label twice.
fn disabled_color(color: u32) -> u32 {
    (0..3).fold(0, |value, index| {
        let shift = (2 - index) * 8;
        let foreground = ((color >> shift) & 255) as f32;
        let background = ((CUE_UI.palette.canvas >> shift) & 255) as f32;
        value | (((foreground * 0.4 + background * 0.6).round() as u32) << shift)
    })
}
fn button_base(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    primary: bool,
    enabled: bool,
) -> Stateful<Div> {
    button_base_with_hover(id, text, primary, enabled, None)
}
fn button_base_with_hover(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    primary: bool,
    enabled: bool,
    hover: Option<(u32, f32)>,
) -> Stateful<Div> {
    let text = text.into();
    let id = id.into();
    let hover_id = format!("button-hover-{id:?}");
    let group: gpui::SharedString = format!("action-{id:?}").into();
    let pressed = if primary {
        INTERACTION.primary_pressed
    } else {
        INTERACTION.neutral_pressed
    };
    let p = CUE_UI.palette;
    let paint = |color| {
        if enabled {
            color
        } else {
            disabled_color(color)
        }
    };
    div()
        .id(id)
        .group(group.clone())
        .relative()
        .h(px(BUTTON_HEIGHT))
        .flex_shrink_0()
        .px(px(BUTTON_PADDING_X))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded(px(BUTTON_RADIUS))
        .border_1()
        .border_color(rgb(paint(if primary { p.text } else { p.border_strong })))
        .bg(rgb(paint(if primary { p.text } else { p.elevated })))
        .text_color(rgb(paint(if primary { p.elevated } else { p.text })))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .when(enabled, |v| {
            v.focusable()
                .tab_stop(true)
                .cursor_pointer()
                .focus_visible(move |v| v.border_color(rgb(INTERACTION.focus_border)))
                .active(move |v| v.bg(rgb(pressed)))
        })
        .when(!enabled, |v| v.cursor_default())
        .when_some(hover.filter(|_| enabled), |v, (color, radius)| {
            v.child(crate::components::motion::HoverFill {
                id: hover_id.into(),
                color,
                radius,
                pressed: Some((group, pressed)),
            })
        })
        .when(!text.is_empty(), |v| v.child(text))
}
pub fn button(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    primary: bool,
    enabled: bool,
) -> Stateful<Div> {
    button_base_with_hover(
        id,
        text,
        primary,
        enabled,
        Some((
            if primary {
                BUTTON_FOCUS_BACKGROUND
            } else {
                INTERACTION.neutral_hover
            },
            BUTTON_RADIUS,
        )),
    )
}
/// A pending request stays scoped to the action that started it.
pub fn busy_button(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    primary: bool,
    enabled: bool,
    busy: bool,
) -> Stateful<Div> {
    let id = id.into();
    let indicator_id = format!("{id:?}-loading");
    button(id, "", primary, enabled && !busy)
        .child(div().when(busy, |v| v.opacity(0.)).child(text.into()))
        .when(busy, |v| {
            v.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::components::loading::indicator(indicator_id, 14.).without_delay(),
                    ),
            )
        })
}
/// Standard and compact actions share their outline with the feedback layer.
#[derive(Clone, Copy)]
pub enum IconButtonSize {
    Standard,
    Compact,
    Small,
}
impl IconButtonSize {
    pub const fn extent(self) -> f32 {
        match self {
            Self::Standard => 32.,
            Self::Compact => 28.,
            Self::Small => 24.,
        }
    }
    pub const fn radius(self) -> f32 {
        match self {
            Self::Standard => ICON_BUTTON_RADIUS,
            Self::Compact => 10.,
            Self::Small => 8.,
        }
    }
}

pub fn icon_button(id: impl Into<gpui::ElementId>, enabled: bool) -> Stateful<Div> {
    icon_button_sized(id, enabled, IconButtonSize::Standard)
}
pub fn icon_button_sized(
    id: impl Into<gpui::ElementId>,
    enabled: bool,
    size: IconButtonSize,
) -> Stateful<Div> {
    button_base_with_hover(
        id,
        "",
        false,
        enabled,
        Some((INTERACTION.neutral_hover, size.radius())),
    )
    .size(px(size.extent()))
    .gap_0()
    .px_0()
    .border_1()
    .border_color(gpui::rgba(0))
    .rounded(px(size.radius()))
    .bg(gpui::rgba(0))
    .when(!enabled, |v| v.opacity(0.4))
}
/// Quiet text actions use the same feedback as adjacent icon actions.
pub fn quiet_button(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    enabled: bool,
    size: IconButtonSize,
) -> Stateful<Div> {
    icon_button_sized(id, enabled, size)
        .w_auto()
        .px_2()
        .gap_1()
        .child(text.into())
}
/// A single-selection control with one shared capsule and a selected segment.
pub fn choice_group(id: impl Into<gpui::ElementId>, selected: usize, count: usize) -> Div {
    let count = count.max(1) as f32;
    div()
        .relative()
        .flex()
        .gap(px(2.))
        .p(px(4.))
        .rounded_full()
        .bg(rgb(CUE_UI.palette.prompt))
        .child(
            div().absolute().inset(px(4.)).child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(gpui::relative(1. / count))
                    .pr(px(2. * (count - 1.) / count))
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .bg(rgb(CUE_UI.palette.canvas))
                            .shadow(vec![gpui::BoxShadow::new(
                                px(0.),
                                px(1.),
                                gpui::rgba(0x24272B14).into(),
                            )
                            .blur_radius(px(3.))]),
                    )
                    .with_spring(
                        id,
                        crate::components::motion::spring(selected as f32),
                        move |v, t| v.left(gpui::relative(t / count)).ml(px(t * 2. / count)),
                    ),
            ),
        )
}
pub fn segment(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    selected: bool,
    enabled: bool,
) -> Stateful<Div> {
    choice(id, text, selected, enabled)
        .border_1()
        .border_color(gpui::rgba(0))
        .justify_center()
        .bg(gpui::rgba(0))
}
pub fn choice(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    selected: bool,
    enabled: bool,
) -> Stateful<Div> {
    let text = text.into();
    let p = CUE_UI.palette;
    div()
        .id(id)
        .when(enabled, |v| {
            v.focusable()
                .tab_stop(true)
                .cursor_pointer()
                .focus_visible(|v| v.border_color(rgb(INTERACTION.focus_border)))
                .active(|v| v.bg(rgb(INTERACTION.neutral_pressed)))
        })
        .px(px(BUTTON_PADDING_X))
        .h(px(CONTROL_HEIGHT))
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(BUTTON_RADIUS))
        .border_1()
        .border_color(rgb(p.border_strong))
        .bg(rgb(if selected { p.selected } else { p.elevated }))
        .text_color(rgb(if selected { p.text } else { p.muted }))
        .text_size(px(12.))
        .when(!enabled, |v| v.opacity(0.45).cursor_default())
        .when(enabled && !selected, |v| {
            v.hover(|v| v.bg(rgb(INTERACTION.neutral_hover)))
        })
        .child(text.clone())
}
pub fn field(
    id: impl Into<gpui::ElementId>,
    text: &'static str,
    input: &Entity<ComposerInput>,
    cx: &gpui::App,
) -> Div {
    field_with_error(id, text, input, None, cx)
}
/// Standard input surface, also usable in an inline title without an extra label.
/// Callers attach automation after adding their own keyboard interactions.
pub fn input_control(
    id: impl Into<gpui::ElementId>,
    input: &Entity<ComposerInput>,
    invalid: bool,
    cx: &gpui::App,
) -> Stateful<Div> {
    let handle = input.read(cx).focus_handle();
    let focus = input.clone();
    let p = CUE_UI.palette;
    div()
        .id(id)
        .h(px(FIELD_HEIGHT))
        .track_focus(&handle)
        .focus(move |v| {
            v.border_color(rgb(if invalid {
                FORM.error_focus_border
            } else {
                FIELD_FOCUS_BORDER
            }))
            .bg(rgb(if invalid {
                FORM.error_surface
            } else {
                p.elevated
            }))
        })
        .px_3()
        .py(px(5.))
        .text_size(px(13.))
        .line_height(px(20.))
        .w_full()
        .border_1()
        .border_color(rgb(if invalid {
            FORM.error_border
        } else {
            p.border_strong
        }))
        .rounded(px(FIELD_RADIUS))
        .bg(rgb(if invalid {
            FORM.error_surface
        } else {
            p.elevated
        }))
        .when(!invalid, |v| {
            v.hover(|v| v.border_color(rgb(FIELD_HOVER_BORDER)))
        })
        .child(input.clone())
        .on_click(move |_, w, cx| w.focus(&focus.read(cx).focus_handle(), cx))
}

pub fn field_with_error(
    id: impl Into<gpui::ElementId>,
    text: &'static str,
    input: &Entity<ComposerInput>,
    error: Option<String>,
    cx: &gpui::App,
) -> Div {
    let id: gpui::ElementId = id.into();
    let error_id = format!("{id:?}-error");
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(label(text))
        .child(
            input_control(id, input, error.is_some(), cx)
                .automation(AutomationRole::TextInput, text),
        )
        .when_some(error, |v, error| {
            v.child(
                div()
                    .id(error_id)
                    .mt(px(-3.))
                    .flex()
                    .items_start()
                    .gap(px(5.))
                    .text_size(px(11.))
                    .line_height(px(17.))
                    .text_color(rgb(CUE_UI.palette.danger))
                    .child(
                        div()
                            .w(px(12.))
                            .h(px(17.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .child(
                                icon("icons/attention.svg", 12.)
                                    .text_color(rgb(CUE_UI.palette.danger)),
                            ),
                    )
                    .child(error.clone())
                    .automation(AutomationRole::Status, error),
            )
        })
}
pub fn feedback(message: String) -> Div {
    status_notice(message, NoticeKind::Info)
}
pub fn avatar(id: &str, name: &str, size: f32) -> Div {
    let hash = id
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (fill, ink) =
        crate::design::LEADER_AVATAR_COLORS[hash % crate::design::LEADER_AVATAR_COLORS.len()];
    let initial = name
        .trim()
        .chars()
        .next()
        .unwrap_or('L')
        .to_uppercase()
        .collect::<String>();
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(fill))
        .text_color(rgb(ink))
        .text_size(px(size * 0.38))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(initial)
}

/// The stored key is shared by creation, settings and conversation views.
pub const AGENT_AVATARS: [(&str, &str, &str); 12] = [
    ("cat", "小猫", "avatars/cat.svg"),
    ("bunny", "小兔", "avatars/bunny.svg"),
    ("bear", "小熊", "avatars/bear.svg"),
    ("fox", "狐狸", "avatars/fox.svg"),
    ("panda", "熊猫", "avatars/panda.svg"),
    ("chick", "小鸡", "avatars/chick.svg"),
    ("dog", "小狗", "avatars/dog.svg"),
    ("owl", "猫头鹰", "avatars/owl.svg"),
    ("koala", "考拉", "avatars/koala.svg"),
    ("penguin", "企鹅", "avatars/penguin.svg"),
    ("deer", "小鹿", "avatars/deer.svg"),
    ("octopus", "章鱼", "avatars/octopus.svg"),
];
/// Portrait only, for members sitting directly on the composer material.
pub fn agent_portrait(avatar: Option<&str>, size: f32) -> Div {
    let (key, _, _) = AGENT_AVATARS
        .iter()
        .find(|(key, _, _)| Some(*key) == avatar)
        .unwrap_or(&AGENT_AVATARS[0]);
    div()
        .size(px(size))
        .flex_shrink_0()
        .child(gpui::img(format!("avatars/portraits/{key}.svg")).size(px(size)))
}

pub fn agent_avatar(avatar: Option<&str>, size: f32) -> Div {
    let (_, _, path) = AGENT_AVATARS
        .iter()
        .find(|(key, _, _)| Some(*key) == avatar)
        .unwrap_or(&AGENT_AVATARS[0]);
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded(px((size * 0.375).min(12.)))
        .border_1()
        .border_color(rgb(CUE_UI.palette.border))
        .overflow_hidden()
        .bg(rgb(CUE_UI.palette.canvas))
        .child(gpui::img(*path).size(px((size - 2.).max(1.))))
}

/// A single-selection field with a window-clamped overlay; choices never reflow the form.
pub fn dropdown<V: 'static>(
    id: impl Into<gpui::SharedString>,
    label: String,
    options: Vec<(String, String, bool)>,
    open: bool,
    enabled: bool,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<V>,
    set_open: impl Fn(&mut V, bool, &mut gpui::Context<V>) + 'static,
    choose: impl Fn(&mut V, usize, &mut gpui::Context<V>) + 'static,
) -> gpui::AnyElement {
    dropdown_with_icons(
        id,
        label,
        options,
        open,
        enabled,
        None,
        vec![],
        window,
        cx,
        set_open,
        choose,
    )
}
pub fn dropdown_with_icons<V: 'static>(
    id: impl Into<gpui::SharedString>,
    label: String,
    options: Vec<(String, String, bool)>,
    open: bool,
    enabled: bool,
    leading: Option<&'static str>,
    option_icons: Vec<Option<&'static str>>,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<V>,
    set_open: impl Fn(&mut V, bool, &mut gpui::Context<V>) + 'static,
    choose: impl Fn(&mut V, usize, &mut gpui::Context<V>) + 'static,
) -> gpui::AnyElement {
    let id = id.into();
    let p = CUE_UI.palette;
    let count = options.len();
    let selected_index = options.iter().position(|option| option.2).unwrap_or(0);
    let trigger_state =
        window.use_keyed_state(format!("{id}-focus"), cx, |_, cx| cx.focus_handle());
    let trigger_focus = trigger_state.read(cx).clone();
    let options_state = window.use_keyed_state(format!("{id}-option-focus"), cx, |_, _| {
        Vec::<gpui::FocusHandle>::new()
    });
    options_state.update(cx, |handles, cx| {
        while handles.len() < count {
            handles.push(cx.focus_handle());
        }
    });
    let option_focus = options_state.read(cx).clone();

    let set_open = std::rc::Rc::new(set_open);
    let toggle = set_open.clone();
    let choose = std::rc::Rc::new(choose);
    let click_trigger = trigger_focus.clone();
    let keyboard_open = set_open.clone();
    let key_handles = option_focus.clone();
    let menu_close = set_open.clone();
    let menu_trigger = trigger_focus.clone();
    let menu_handles = option_focus.clone();

    let menu_max_width = f32::from(window.viewport_size().width) - 24.;
    let measured = window.use_keyed_state(format!("{id}-width"), cx, |_, _| 0f32);
    let menu_width = *measured.read(cx);
    div()
        .relative()
        .w_full()
        .h(px(DROPDOWN_HEIGHT))
        .child(
            gpui::canvas(
                move |bounds, _, cx| {
                    measured.update(cx, |width, cx| {
                        let next = f32::from(bounds.size.width);
                        if (*width - next).abs() > 0.1 {
                            *width = next;
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            button_base(id.clone(), "", false, enabled)
                .track_focus(&trigger_focus)
                .on_key_down(
                    cx.listener(move |v, event: &gpui::KeyDownEvent, window, cx| {
                        if !enabled {
                            return;
                        }
                        if matches!(event.keystroke.key.as_str(), "down" | "up") && count > 0 {
                            keyboard_open(v, true, cx);
                            let index = if event.keystroke.key == "up" {
                                count - 1
                            } else {
                                selected_index.min(count - 1)
                            };
                            let focus = key_handles[index].clone();
                            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
                            cx.stop_propagation();
                        } else if event.keystroke.key == "escape" && open {
                            keyboard_open(v, false, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .w_full()
                .h(px(DROPDOWN_HEIGHT))
                .justify_between()
                .px(px(12.))
                .gap(px(8.))
                .text_size(px(13.))
                .rounded(px(FIELD_RADIUS))
                .border_color(rgb(if open {
                    FIELD_FOCUS_BORDER
                } else {
                    p.border_strong
                }))
                .when(enabled, |v| {
                    v.hover(|v| v.border_color(rgb(FIELD_HOVER_BORDER)))
                        .active(|v| v.bg(rgb(p.elevated)).border_color(rgb(FIELD_FOCUS_BORDER)))
                        .focus_visible(|v| v.border_color(rgb(FIELD_FOCUS_BORDER)))
                })
                .when_some(leading, |v, path| {
                    v.child(
                        gpui::img(path)
                            .size(px(18.))
                            .flex_shrink_0()
                            .when(!enabled, |v| v.opacity(0.4)),
                    )
                })
                .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                .child(icon("icons/chevron-down.svg", 12.).with_spring(
                    "select-chevron",
                    crate::components::motion::spring(if open { 1. } else { 0. }),
                    |v, t| {
                        v.with_transformation(gpui::Transformation::rotate(gpui::radians(
                            std::f32::consts::PI * t,
                        )))
                    },
                ))
                .on_click(cx.listener(move |v, _, window, cx| {
                    if enabled {
                        toggle(v, !open, cx);
                        window.focus(&click_trigger, cx);
                    }
                }))
                .automation_enabled(enabled, AutomationRole::Button, label),
        )
        .when(open && menu_width > 0., |root| {
            root.child(
                gpui::deferred(
                    gpui::anchored()
                        .offset(gpui::point(px(-MENU_OUTSET), px(MENU_GAP)))
                        .snap_to_window_with_margin(px(12.))
                        .child(
                            div()
                                .id(format!("{id}-menu"))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .occlude()
                                .on_key_down(cx.listener(
                                    move |v, event: &gpui::KeyDownEvent, window, cx| {
                                        if event.keystroke.key == "escape" {
                                            menu_close(v, false, cx);
                                            window.focus(&menu_trigger, cx);
                                            cx.stop_propagation();
                                            return;
                                        }
                                        if count == 0 {
                                            return;
                                        }
                                        let current = menu_handles
                                            .iter()
                                            .take(count)
                                            .position(|focus| focus.is_focused(window))
                                            .unwrap_or(0);
                                        let next = match event.keystroke.key.as_str() {
                                            "down" => Some((current + 1) % count),
                                            "up" => Some((current + count - 1) % count),
                                            "home" => Some(0),
                                            "end" => Some(count - 1),
                                            _ => None,
                                        };
                                        if let Some(next) = next {
                                            window.focus(&menu_handles[next], cx);
                                            cx.stop_propagation();
                                        }
                                    },
                                ))
                                .w(px((menu_width + MENU_OUTSET * 2.).min(menu_max_width)))
                                .max_h(px(218.))
                                .overflow_y_scroll()
                                .p_2()
                                .rounded(px(MENU_RADIUS))
                                .border_1()
                                .border_color(rgb(p.border))
                                .bg(rgb(p.canvas))
                                .shadow_md()
                                .on_mouse_down_out(
                                    cx.listener(move |v, _, _, cx| set_open(v, false, cx)),
                                )
                                .children(options.into_iter().enumerate().map(
                                    |(index, (id, label, selected))| {
                                        let choose = choose.clone();
                                        let return_focus = trigger_focus.clone();
                                        let icon_path = option_icons.get(index).copied().flatten();
                                        button_base(id, "", false, enabled)
                                            .track_focus(&option_focus[index])
                                            .w_full()
                                            .h(px(32.))
                                            .text_size(px(13.))
                                            .line_height(px(20.))
                                            .gap(px(8.))
                                            .rounded(px(FIELD_RADIUS))
                                            .border_1()
                                            .border_color(gpui::rgba(0))
                                            .px_2()
                                            .justify_between()
                                            .bg(rgb(if selected { p.selected } else { p.canvas }))
                                            .when(enabled, |v| {
                                                v.hover(move |v| {
                                                    v.bg(rgb(if selected {
                                                        p.selected
                                                    } else {
                                                        p.sidebar_hover
                                                    }))
                                                })
                                            })
                                            .when_some(icon_path, |v, path| {
                                                v.child(
                                                    gpui::img(path)
                                                        .size(px(18.))
                                                        .flex_shrink_0()
                                                        .when(!enabled, |v| v.opacity(0.4)),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .child(label.clone()),
                                            )
                                            .when(selected, |v| {
                                                v.child(icon("icons/check.svg", 12.))
                                            })
                                            .on_click(cx.listener(move |v, _, window, cx| {
                                                if enabled {
                                                    choose(v, index, cx);
                                                    window.focus(&return_focus, cx);
                                                }
                                            }))
                                            .automation_enabled(
                                                enabled,
                                                AutomationRole::Option,
                                                label,
                                            )
                                    },
                                ))
                                .automation(AutomationRole::Status, "下拉菜单")
                                .map(|menu| {
                                    crate::components::motion::enter_instrumented(
                                        menu,
                                        "menu-enter",
                                        4.,
                                    )
                                }),
                        ),
                )
                .with_priority(200),
            )
        })
        .into_any_element()
}

pub fn provider_path(provider: &str) -> &'static str {
    match provider {
        "openai" => "providers/openai.svg",
        "anthropic" => "providers/anthropic.svg",
        "github-copilot" => "providers/githubcopilot.svg",
        "kimi" | "kimi-coding" => "providers/kimi.svg",
        "openrouter" => "providers/openrouter.svg",
        "opencode-go" => "providers/opencode.svg",
        "xai" => "providers/xai.svg",
        _ => "providers/compatible.svg",
    }
}
pub fn provider_icon(provider: &str, size: f32) -> gpui::Div {
    let path = provider_path(provider);
    gpui::div()
        .size(px(size))
        .flex_shrink_0()
        .child(gpui::img(path).size(px(size)))
}

#[derive(Clone)]
pub struct OpenAgent {
    pub id: String,
}
/// Controlled switch with the same compact geometry in native and Web renderers.
pub fn switch<V: 'static>(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
    checked: bool,
    enabled: bool,
    focus: &gpui::FocusHandle,
    cx: &gpui::Context<V>,
    on_change: impl Fn(&mut V, bool, &mut gpui::Context<V>) + 'static,
) -> impl IntoElement {
    let p = CUE_UI.palette;
    let paint = |color| {
        if enabled {
            color
        } else {
            disabled_color(color)
        }
    };
    let off_color = paint(FORM.switch_off);
    let on_color = paint(p.success);
    let change = std::rc::Rc::new(on_change);
    let click_focus = focus.clone();
    let label: gpui::SharedString = label.into();
    div()
        .id(id)
        .w(px(48.))
        .h(px(CONTROL_HEIGHT))
        .relative()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(gpui::rgba(0))
        .when(enabled, |v| {
            v.focusable()
                .track_focus(focus)
                .tab_stop(true)
                .cursor_pointer()
                .focus_visible(|s| s.border_color(rgb(INTERACTION.focus_border)))
                .hover(|s| s.bg(rgb(INTERACTION.neutral_hover)))
                .active(|s| s.bg(rgb(INTERACTION.neutral_pressed)))
        })
        .when(!enabled, |v| v.cursor_default())
        .on_click(cx.listener(move |v, _, window, cx| {
            cx.stop_propagation();
            if enabled {
                window.focus(&click_focus, cx);
                change(v, !checked, cx);
            }
        }))
        .child(
            div()
                .relative()
                .w(px(41.))
                .h(px(24.))
                .rounded_full()
                .bg(rgb(paint(if checked {
                    p.success
                } else {
                    FORM.switch_off
                })))
                .child(
                    div()
                        .absolute()
                        .top(px(3.))
                        .left(px(3.))
                        .size(px(18.))
                        .rounded_full()
                        .bg(rgb(p.canvas))
                        .with_spring(
                            "switch-thumb",
                            crate::components::motion::spring(if checked { 20. } else { 3. }),
                            |v, x| v.left(px(x)),
                        ),
                )
                .with_spring(
                    "switch-color",
                    crate::components::motion::spring(if checked { 1. } else { 0. }),
                    move |v, t| v.bg(crate::components::motion::mix_rgb(off_color, on_color, t)),
                ),
        )
        .automation_enabled(
            enabled,
            AutomationRole::Option,
            format!("{}：{}", label, if checked { "开启" } else { "关闭" }),
        )
}

pub fn avatar_picker<V: 'static>(
    prefix: impl Into<gpui::SharedString>,
    selected: &str,
    enabled: bool,
    cx: &gpui::Context<V>,
    choose: impl Fn(&mut V, &'static str, &mut gpui::Context<V>) + 'static,
) -> Div {
    let prefix = prefix.into();
    let p = CUE_UI.palette;
    let choose = std::rc::Rc::new(choose);
    div().flex().flex_col().gap_2().child(label("头像")).child(
        div()
            .flex()
            .flex_wrap()
            .max_w(px(232.))
            .gap_2()
            .children(AGENT_AVATARS.into_iter().map(|(key, title, _)| {
                let choose = choose.clone();
                let active = selected == key;
                div()
                    .id(format!("{prefix}-{key}"))
                    .size(px(CONTROL_HEIGHT))
                    .p(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(FIELD_RADIUS))
                    .bg(rgb(if active { p.selected } else { p.elevated }))
                    .when(enabled, |v| {
                        v.focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .focus_visible(|v| v.bg(rgb(p.sidebar_hover)))
                            .hover(|v| v.bg(rgb(p.sidebar_hover)))
                    })
                    .when(!enabled, |v| v.opacity(0.4).cursor_default())
                    .child(agent_avatar(Some(key), 24.))
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if enabled {
                            choose(v, key, cx);
                        }
                    }))
                    .automation_enabled(
                        enabled,
                        AutomationRole::Option,
                        if active {
                            format!("{title} · 已选择")
                        } else {
                            title.into()
                        },
                    )
            })),
    )
}

#[derive(Clone, Copy)]
pub enum NoticeKind {
    Info,
    Success,
    Error,
    Warning,
    Loading,
}
pub fn status_notice(message: String, kind: NoticeKind) -> Div {
    let (foreground, background, glyph) = match kind {
        NoticeKind::Info => (
            CUE_UI.palette.text,
            CUE_UI.palette.prompt,
            "icons/attention.svg",
        ),
        NoticeKind::Success => (
            CUE_UI.palette.success,
            FORM.success_surface,
            "icons/completed.svg",
        ),
        NoticeKind::Error => (
            CUE_UI.palette.danger,
            FORM.error_surface,
            "icons/attention.svg",
        ),
        NoticeKind::Warning => (
            CUE_UI.palette.warning,
            FORM.warning_surface,
            "icons/attention.svg",
        ),
        NoticeKind::Loading => (
            CUE_UI.palette.muted,
            CUE_UI.palette.prompt,
            "icons/loader.svg",
        ),
    };
    div()
        .flex()
        .items_start()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(FIELD_RADIUS))
        .bg(rgb(background))
        .text_color(rgb(foreground))
        .text_size(px(12.))
        .line_height(px(20.))
        .child(
            div()
                .w(px(16.))
                .h(px(20.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .child(
                    gpui::svg()
                        .path(glyph)
                        .size(px(16.))
                        .flex_shrink_0()
                        .text_color(rgb(foreground)),
                ),
        )
        .child(message)
}
