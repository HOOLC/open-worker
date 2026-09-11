//! Read-only hover details shared by navigation and component examples.
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    controls as ui,
    design::CUE_UI,
};
use gpui::{div, prelude::*, px, rgb, rgba, Context, FontWeight, Window};

const DETAILS_GAP: f32 = 4.;
use super::motion::Slide;
const CONTENT_SECONDS: f32 = 0.14;
const DISMISS_SECONDS: f32 = 0.12;
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
#[cfg(target_family = "wasm")]
use web_time::Instant;

#[derive(Clone)]
pub struct DetailsTooltip {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub avatar: Option<String>,
    pub description: String,
    pub rows: Vec<(String, String)>,
}
impl DetailsTooltip {
    pub fn content(&self) -> gpui::Div {
        let p = CUE_UI.palette;
        let description: String = self.description.trim().chars().take(320).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(match &self.avatar {
                        Some(avatar) => ui::agent_avatar(Some(avatar), 32.).into_any_element(),
                        None => div()
                            .size(px(32.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(12.))
                            .bg(rgb(p.prompt))
                            .child(ui::icon("icons/checklist.svg", 18.))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(19.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_clamp(2)
                                    .child(self.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .line_height(px(16.))
                                    .text_color(rgb(p.muted))
                                    .child(self.kind.clone()),
                            ),
                    ),
            )
            .when(!description.is_empty(), |v| {
                v.child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(19.))
                        .line_clamp(3)
                        .child(description),
                )
            })
            .child(
                div().flex().flex_col().gap(px(6.)).children(
                    self.rows
                        .iter()
                        .filter(|(_, value)| !value.trim().is_empty())
                        .map(|(label, value)| {
                            div()
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .text_size(px(11.))
                                .line_height(px(17.))
                                .child(
                                    div()
                                        .w(px(60.))
                                        .flex_shrink_0()
                                        .text_color(rgb(p.muted))
                                        .child(label.clone()),
                                )
                                .child(div().flex_1().min_w_0().line_clamp(2).child(value.clone()))
                        }),
                ),
            )
    }
    pub fn card(&self) -> crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>> {
        Self::surface(format!("detail-tooltip-{}", self.key))
            .p(px(16.))
            .child(self.content())
            .automation(
                AutomationRole::Status,
                format!("{}详情：{}", self.kind, self.title),
            )
    }
    fn surface(id: String) -> gpui::Stateful<gpui::Div> {
        let p = CUE_UI.palette;
        div()
            .id(id)
            .occlude()
            .w(px(320.))
            .max_w_full()
            .rounded(px(ui::CARD_RADIUS))
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.canvas))
            .shadow(vec![gpui::BoxShadow::new(
                px(0.),
                px(6.),
                rgba(0x24272B14).into(),
            )
            .blur_radius(px(20.))])
            .flex()
            .flex_col()
    }
}

impl gpui::Render for DetailsTooltip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(format!("detail-tooltip-scroll-{}", self.key))
            .max_w(window.viewport_size().width - px(24.))
            .child(self.card().map_inner(|card| {
                card.max_h(window.viewport_size().height - px(24.))
                    .overflow_y_scroll()
            }))
    }
}

#[derive(Default)]
struct HoverState {
    bounds: gpui::Bounds<gpui::Pixels>,
    open: bool,
    trigger_hover: bool,
    panel_hover: bool,
    timer: Option<gpui::Task<()>>,
}
impl HoverState {
    fn hover(&mut self, hovered: bool, panel: bool, cx: &mut Context<Self>) {
        if panel {
            self.panel_hover = hovered;
        } else {
            self.trigger_hover = hovered;
        }
        let hovered = self.trigger_hover || self.panel_hover;
        self.timer.take();
        if hovered {
            self.open = true;
            cx.notify();
            return;
        }
        // Allow the pointer to cross the gap into the card.
        self.timer = Some(cx.spawn(async move |state, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let _ = state.update(cx, |state, cx| {
                state.open = false;
                cx.notify();
            });
        }));
    }
}

fn approach(value: f32, target: f32, distance: f32) -> f32 {
    value + (target - value).signum() * (target - value).abs().min(distance)
}

#[derive(Default)]
struct PopupPose {
    position: Option<Slide<4>>,
    last_frame: Option<Instant>,
    measured: Option<(String, f32, f32)>,
}

/// A single keyed floating surface, centred above the current trigger. Reuse its
/// key across targets so position and height continue from the last visible frame.
#[derive(gpui::IntoElement)]
pub struct SlidingPopup<F: Fn(&mut Window, &mut gpui::App) -> gpui::AnyElement + 'static> {
    key: String,
    content_key: String,
    anchor: gpui::Bounds<gpui::Pixels>,
    width: f32,
    content: F,
    hover: Option<Box<dyn Fn(&bool, &mut Window, &mut gpui::App)>>,
}
pub fn sliding_popup<F: Fn(&mut Window, &mut gpui::App) -> gpui::AnyElement + 'static>(
    key: impl Into<String>,
    content_key: impl Into<String>,
    anchor: gpui::Bounds<gpui::Pixels>,
    width: f32,
    content: F,
) -> SlidingPopup<F> {
    SlidingPopup {
        key: key.into(),
        content_key: content_key.into(),
        anchor,
        width,
        content,
        hover: None,
    }
}
impl<F: Fn(&mut Window, &mut gpui::App) -> gpui::AnyElement + 'static> SlidingPopup<F> {
    pub fn on_hover(
        mut self,
        listener: impl Fn(&bool, &mut Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.hover = Some(Box::new(listener));
        self
    }
}
impl<F: Fn(&mut Window, &mut gpui::App) -> gpui::AnyElement + 'static> gpui::RenderOnce
    for SlidingPopup<F>
{
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let state = window.use_keyed_state(self.key.clone(), cx, |_, _| PopupPose::default());
        let viewport = window.viewport_size();
        let width = self.width.min((viewport.width.as_f32() - 24.).max(1.));
        let measured = state.read(cx).measured.clone();
        let height = if let Some((_, _, h)) =
            measured.filter(|(key, w, _)| key == &self.content_key && *w == width)
        {
            h
        } else {
            let mut element = (self.content)(window, cx);
            let h = element
                .layout_as_root(
                    gpui::size(
                        gpui::AvailableSpace::Definite(px((width - 2.).max(1.))),
                        gpui::AvailableSpace::MinContent,
                    ),
                    window,
                    cx,
                )
                .height
                .as_f32()
                + 2.;
            state.update(cx, |v, _| {
                v.measured = Some((self.content_key.clone(), width, h))
            });
            h
        }
        .min((viewport.height.as_f32() - 24.).max(1.));
        let target = [
            (self.anchor.center().x.as_f32() - width / 2.)
                .clamp(12., (viewport.width.as_f32() - width - 12.).max(12.)),
            (self.anchor.top().as_f32() - height - 8.).max(12.),
            width,
            height,
        ];
        let reduced = cx.reduce_motion();
        let (position, moving) = state.update(cx, |v, _| {
            let now = Instant::now();
            let dt = v
                .last_frame
                .replace(now)
                .map_or(0., |last| now.duration_since(last).as_secs_f32());
            let position = v.position.get_or_insert_with(|| Slide::new(target));
            let was_moving = position.is_moving();
            let changed = position.retarget(target);
            let dt = if changed && !was_moving { 0. } else { dt };
            let moving = position.advance(dt, reduced);
            (position.position(), moving)
        });
        if moving {
            window.on_next_frame(move |_, cx| state.update(cx, |_, cx| cx.notify()));
        }
        gpui::deferred(
            gpui::anchored()
                .position(gpui::point(px(position[0]), px(position[1])))
                .snap_to_window_with_margin(px(12.))
                .child(
                    DetailsTooltip::surface(format!("{}-surface", self.key))
                        .rounded(px(10.))
                        .w(px(position[2]))
                        .h(px(position[3]))
                        .overflow_hidden()
                        .when_some(self.hover, |surface, hover| surface.on_hover(hover))
                        .child(
                            div()
                                .id(format!("{}-scroll", self.key))
                                .size_full()
                                .overflow_y_scroll()
                                .child((self.content)(window, cx)),
                        ),
                ),
        )
        .with_priority(2)
    }
}

/// One persistent surface per navigation group, shared by all its triggers.
#[derive(Default)]
pub struct DetailsOverlay {
    active: Option<DetailsTooltip>,
    outgoing: Vec<(DetailsTooltip, f32)>,
    painted_opacities: Vec<f32>,
    active_opacity: f32,
    content_started: Option<Instant>,
    position: Option<Slide<3>>,
    last_frame: Option<Instant>,
    animating: bool,
    dismissing: bool,
    visibility: f32,
    measured: Option<(String, f32, f32)>,
    bounds: gpui::Bounds<gpui::Pixels>,
    trigger_hover: bool,
    panel_hover: bool,
    timer: Option<gpui::Task<()>>,
}
impl DetailsOverlay {
    fn show(
        &mut self,
        details: DetailsTooltip,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.timer.take();
        self.dismissing = false;
        let now = Instant::now();
        if self.active.as_ref().is_some_and(|v| v.key != details.key) {
            for (layer, opacity) in self.outgoing.iter_mut().zip(&self.painted_opacities) {
                layer.1 = *opacity;
            }
            self.outgoing
                .push((self.active.as_ref().unwrap().clone(), self.active_opacity));
            self.outgoing.retain(|(_, opacity)| *opacity > 0.01);
            self.outgoing.sort_by(|a, b| b.1.total_cmp(&a.1));
            self.outgoing.truncate(4);
            self.painted_opacities = self.outgoing.iter().map(|(_, opacity)| *opacity).collect();
            self.active_opacity = 0.;
            self.content_started = Some(now);
            self.measured = None;
        } else if self.active.is_none() {
            self.visibility = 1.;
            self.position = None;
            self.outgoing.clear();
            self.painted_opacities.clear();
            self.active_opacity = 1.;
            self.content_started = None;
            self.measured = None;
        }
        if !self.animating {
            self.last_frame = Some(now);
        }
        self.active = Some(details);
        self.bounds = bounds;
        self.trigger_hover = true;
        self.panel_hover = false;
        cx.notify();
    }
    fn leave(&mut self, key: &str, cx: &mut Context<Self>) {
        // A late leave event from the previous row must not close the new target.
        if self.active.as_ref().is_some_and(|v| v.key == key) {
            self.trigger_hover = false;
            self.schedule_close(cx);
        }
    }
    fn schedule_close(&mut self, cx: &mut Context<Self>) {
        self.timer.take();
        if self.trigger_hover || self.panel_hover {
            if self.dismissing {
                self.dismissing = false;
                cx.notify();
            }
            return;
        }
        self.timer = Some(cx.spawn(async move |state, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let _ = state.update(cx, |state, cx| {
                state.dismissing = true;
                if !state.animating {
                    state.last_frame = Some(Instant::now());
                }
                cx.notify();
            });
        }));
    }
}
impl gpui::Render for DetailsOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(0., |last| now.duration_since(last).as_secs_f32());
        self.last_frame = Some(now);
        let visibility_target = if self.dismissing { 0. } else { 1. };
        self.visibility = if cx.reduce_motion() {
            visibility_target
        } else {
            approach(self.visibility, visibility_target, dt / DISMISS_SECONDS)
        };
        if self.dismissing && self.visibility == 0. {
            self.active = None;
            self.outgoing.clear();
            self.painted_opacities.clear();
            self.animating = false;
        }
        let Some(details) = self.active.as_ref() else {
            return gpui::Empty.into_any_element();
        };
        let viewport = window.viewport_size();
        let right = self.bounds.right() + px(DETAILS_GAP);
        let right_space = (viewport.width - px(12.) - right).max(px(0.));
        let left_space = (self.bounds.left() - px(12. + DETAILS_GAP)).max(px(0.));
        let place_right = right_space >= px(320.) || left_space < px(320.);
        let width = px(320.).min(if place_right { right_space } else { left_space });
        let x = if place_right {
            right
        } else {
            self.bounds.left() - width - px(DETAILS_GAP)
        };
        let measured_height = match &self.measured {
            Some((key, measured_width, height))
                if key == &details.key && *measured_width == width.as_f32() =>
            {
                *height
            }
            _ => {
                let mut content = details
                    .content()
                    .w((width - px(34.)).max(px(1.)))
                    .into_any_element();
                let height = content
                    .layout_as_root(
                        gpui::size(
                            gpui::AvailableSpace::Definite(width - px(34.)),
                            gpui::AvailableSpace::MinContent,
                        ),
                        window,
                        cx,
                    )
                    .height
                    .as_f32()
                    + 34.;
                self.measured = Some((details.key.clone(), width.as_f32(), height));
                height
            }
        };
        let height = measured_height.min((viewport.height - px(24.)).as_f32());
        let target = [
            x.as_f32(),
            self.bounds
                .top()
                .as_f32()
                .min(viewport.height.as_f32() - height - 12.)
                .max(12.),
            height,
        ];
        let position = self.position.get_or_insert_with(|| Slide::new(target));
        position.retarget(target);
        let moving = position.advance(dt, cx.reduce_motion());
        let position = position.position();
        let progress = if cx.reduce_motion() {
            1.
        } else {
            self.content_started.map_or(1., |start| {
                (now.duration_since(start).as_secs_f32() / CONTENT_SECONDS).min(1.)
            })
        };
        let fade = 1. - (1. - progress).powi(3);
        self.active_opacity = fade;
        self.painted_opacities = self
            .outgoing
            .iter()
            .map(|(_, opacity)| opacity * (1. - fade))
            .collect();
        if progress >= 1. {
            self.outgoing.clear();
            self.painted_opacities.clear();
        }
        self.animating = moving || progress < 1. || self.visibility != visibility_target;
        if self.animating {
            window.request_animation_frame();
        }
        let content = div()
            .relative()
            .p(px(16.))
            .child(details.content().opacity(fade))
            .children(self.outgoing.iter().zip(&self.painted_opacities).map(
                |((details, _), opacity)| {
                    div()
                        .absolute()
                        .top(px(16.))
                        .left(px(16.))
                        .right(px(16.))
                        .opacity(*opacity)
                        .child(details.content())
                },
            ));
        gpui::deferred(
            gpui::anchored()
                .position(gpui::point(px(position[0]), px(position[1])))
                .snap_to_window_with_margin(px(12.))
                .child(
                    DetailsTooltip::surface("detail-tooltip-shared".into())
                        .opacity(self.visibility)
                        .w(width)
                        .h(px(position[2]))
                        .overflow_hidden()
                        .child(
                            div()
                                .id("shared-detail-scroll")
                                .size_full()
                                .overflow_y_scroll()
                                .child(content),
                        )
                        .on_hover(cx.listener(|v, hovered, _, cx| {
                            v.panel_hover = *hovered;
                            v.schedule_close(cx);
                        }))
                        .automation(
                            AutomationRole::Status,
                            format!("{}详情：{}", details.kind, details.title),
                        ),
                ),
        )
        .into_any_element()
    }
}

#[derive(gpui::IntoElement)]
pub struct TooltipTrigger {
    row: crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>>,
    details: DetailsTooltip,
    overlay: gpui::Entity<DetailsOverlay>,
}
pub fn trigger(
    row: crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>>,
    details: DetailsTooltip,
    overlay: gpui::Entity<DetailsOverlay>,
) -> TooltipTrigger {
    TooltipTrigger {
        row,
        details,
        overlay,
    }
}
impl gpui::RenderOnce for TooltipTrigger {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let bounds = window.use_keyed_state(
            format!("tooltip-anchor-{}", self.details.key),
            cx,
            |_, _| gpui::Bounds::default(),
        );
        let anchor = bounds.clone();
        self.row.map_inner(|row| {
            row.on_hover(move |hovered, _, cx| {
                let rect = *bounds.read(cx);
                self.overlay.update(cx, |v, cx| {
                    if *hovered {
                        v.show(self.details.clone(), rect, cx);
                    } else {
                        v.leave(&self.details.key, cx);
                    }
                });
            })
            .child(
                gpui::canvas(
                    move |bounds, _, cx| {
                        // Navigation rows have a 1 px border; the absolute canvas measures its inner box.
                        anchor.update(cx, |v, _| *v = bounds.dilate(px(1.)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
        })
    }
}

/// Shared short-hint surface, used by controls and glyph-level text tooltips.
fn hint_surface(
    id: impl Into<gpui::SharedString>,
    text: gpui::SharedString,
) -> crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>> {
    let id: gpui::SharedString = id.into();
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded(px(ui::FIELD_RADIUS))
        .border_1()
        .border_color(rgb(CUE_UI.palette.border))
        .bg(rgb(CUE_UI.palette.canvas))
        .text_color(rgb(CUE_UI.palette.text))
        .text_size(px(12.))
        .line_height(px(20.))
        .whitespace_normal()
        .shadow(vec![gpui::BoxShadow::new(
            px(0.),
            px(4.),
            rgba(0x24272B14).into(),
        )
        .blur_radius(px(12.))])
        .child(text.clone())
        .automation(AutomationRole::Status, text.to_string())
}

/// A view adapter for callers whose trigger already owns positioning and hover.
pub struct Hint {
    id: gpui::SharedString,
    text: gpui::SharedString,
}
impl Hint {
    pub fn new(id: impl Into<gpui::SharedString>, text: impl Into<gpui::SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}
impl gpui::Render for Hint {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        hint_surface(self.id.clone(), self.text.clone())
            .map_inner(|card| card.max_w((window.viewport_size().width - px(24.)).min(px(480.))))
    }
}

/// A short control hint, centred below its trigger and flipped above near the edge.
#[derive(gpui::IntoElement)]
pub struct HintTrigger {
    row: crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>>,
    key: String,
    text: String,
}
pub fn hint(
    row: crate::automation::element::AutomationElement<gpui::Stateful<gpui::Div>>,
    key: impl Into<String>,
    text: impl Into<String>,
) -> HintTrigger {
    HintTrigger {
        row,
        key: key.into(),
        text: text.into(),
    }
}
impl gpui::RenderOnce for HintTrigger {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let state = window.use_keyed_state(format!("hint-state-{}", self.key), cx, |_, _| {
            HoverState::default()
        });
        let bounds = state.read(cx).bounds;
        let open = state.read(cx).open;
        let anchor_state = state.clone();
        let hover_state = state.clone();
        let panel_state = state.clone();
        let escape_state = state.clone();
        let viewport = window.viewport_size();
        let width = px(self.text.chars().count() as f32 * 13. + 40.).min(viewport.width - px(24.));
        let above = bounds.bottom() + px(64.) > viewport.height - px(12.);
        let x = (bounds.center().x - width / 2.)
            .max(px(12.))
            .min(viewport.width - width - px(12.));
        let y = if above {
            bounds.top() - px(8.)
        } else {
            bounds.bottom() + px(8.)
        };
        self.row.map_inner(|row| {
            row.on_hover(move |hovered, _, cx| {
                hover_state.update(cx, |v, cx| v.hover(*hovered, false, cx))
            })
            .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && escape_state.read(cx).open {
                    escape_state.update(cx, |v, cx| {
                        v.open = false;
                        v.timer.take();
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            })
            .child(
                gpui::canvas(
                    move |bounds, _, cx| anchor_state.update(cx, |v, _| v.bounds = bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .when(open, |row| {
                row.child(
                    gpui::deferred(
                        gpui::anchored()
                            .anchor(if above {
                                gpui::Anchor::BottomLeft
                            } else {
                                gpui::Anchor::TopLeft
                            })
                            .position(gpui::point(x, y))
                            .snap_to_window_with_margin(px(12.))
                            .child(
                                hint_surface(
                                    format!("control-hint-{}", self.key),
                                    self.text.into(),
                                )
                                .map_inner(|card| {
                                    card.w(width).on_hover(move |hovered, _, cx| {
                                        panel_state.update(cx, |v, cx| v.hover(*hovered, true, cx))
                                    })
                                }),
                            ),
                    )
                    .with_priority(110),
                )
            })
        })
    }
}
