use crate::{
    automation::{AutomationElementExt, AutomationRole},
    components::motion::SlidingSurface,
    design::{BRAND_ACCENT, CUE_UI, INTERACTION},
};
use gpui::{
    div, prelude::*, px, rgb, rgba, App, AppContext, Bounds, Div, ElementId, Entity, Pixels, Window,
};

pub const TAB_GAP: f32 = 2.;

/// One sidebar owns both its pointer feedback and its current-page indicator.
#[derive(Clone)]
pub struct TabGroup {
    hover: Entity<SlidingSurface>,
    active: Entity<SlidingSurface>,
}
impl TabGroup {
    pub fn new(cx: &mut App) -> Self {
        Self {
            hover: cx.new(|_| Default::default()),
            active: cx.new(|_| Default::default()),
        }
    }

    pub fn keyed(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Self {
        window
            .use_keyed_state(id, cx, |_, cx| Self::new(cx))
            .read(cx)
            .clone()
    }

    pub fn column(&self) -> Div {
        div().flex().flex_col().gap(px(TAB_GAP))
    }

    /// Sections share the same heading metrics and leading/tab alignment.
    pub fn section(&self, id: impl Into<ElementId>, title: impl Into<gpui::SharedString>) -> Div {
        let title = title.into();
        self.column().flex_shrink_0().child(
            div()
                .id(id)
                .h(px(20.))
                .flex_shrink_0()
                .px(px(8.))
                .flex()
                .items_center()
                .text_size(px(11.))
                .line_height(px(16.))
                .text_color(rgb(CUE_UI.palette.muted))
                .child(title.clone())
                .automation(AutomationRole::Status, title),
        )
    }

    /// Wrap the whole transparent sidebar, including section gaps. Hover is
    /// painted beneath its content, and the active marker above pressed fills.
    pub fn surface(&self, content: impl IntoElement) -> TabSurface {
        TabSurface {
            inner: content.into_any_element(),
            tabs: self.clone(),
        }
    }

    pub fn tab(&self, id: String, selected: bool) -> gpui::Stateful<Div> {
        let group: gpui::SharedString = format!("navigation-{id}").into();
        let anchor_id = format!("{id}-anchor");
        div()
            .id(id)
            .group(group.clone())
            .relative()
            .focusable()
            .tab_stop(true)
            .h(px(crate::controls::CONTROL_HEIGHT))
            .px(px(7.))
            .border_1()
            .border_color(rgba(0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(crate::controls::FIELD_RADIUS))
            .child(TabAnchor {
                id: anchor_id.into(),
                group,
                selected,
                tabs: self.clone(),
            })
            .text_color(rgb(CUE_UI.palette.text))
            .text_size(px(12.))
            .cursor_pointer()
            .focus_visible(|v| v.border_color(rgb(INTERACTION.focus_border)))
            .on_key_down(keyboard_navigation)
    }
}

#[derive(Clone, Copy, Default)]
struct Anchor {
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
}
#[derive(gpui::IntoElement)]
struct TabAnchor {
    id: ElementId,
    group: gpui::SharedString,
    selected: bool,
    tabs: TabGroup,
}
impl gpui::RenderOnce for TabAnchor {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let geometry =
            window.use_keyed_state((self.id.clone(), "geometry"), cx, |_, _| Anchor::default());
        let released_tabs = self.tabs.clone();
        let released_id = self.id.clone();
        let _release = window.use_keyed_state((self.id.clone(), "release"), cx, |_, cx| {
            cx.on_release(move |_, cx| {
                for state in [&released_tabs.hover, &released_tabs.active] {
                    state.update(cx, |state, cx| {
                        if state.clear(&released_id) {
                            cx.notify();
                        }
                    });
                }
            })
        });
        let hover = self.tabs.hover.clone();
        let hover_id = self.id.clone();
        let read_geometry = geometry.clone();
        div()
            .id(self.id.clone())
            .absolute()
            .inset_0()
            .rounded(px(crate::controls::FIELD_RADIUS))
            .group_active(self.group, |v| v.bg(rgb(INTERACTION.neutral_pressed)))
            .on_hover(move |hovered, _, cx| {
                let anchor = *read_geometry.read(cx);
                hover.update(cx, |state, cx| {
                    let changed = if *hovered {
                        state.retarget(&hover_id, anchor.bounds, anchor.clip)
                    } else {
                        state.clear(&hover_id)
                    };
                    if changed {
                        cx.notify();
                    }
                });
            })
            .child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        let clip = window.content_mask().bounds;
                        geometry.update(cx, |anchor, _| *anchor = Anchor { bounds, clip });
                        self.tabs.hover.update(cx, |state, cx| {
                            if state.is_target(&self.id) && state.retarget(&self.id, bounds, clip) {
                                cx.notify();
                            }
                        });
                        self.tabs.active.update(cx, |state, cx| {
                            let marker = Bounds::new(
                                gpui::point(bounds.left() + px(1.), bounds.center().y - px(7.)),
                                gpui::size(px(2.), px(14.)),
                            );
                            let changed = if self.selected {
                                state.retarget(&self.id, marker, clip)
                            } else {
                                state.clear(&self.id)
                            };
                            if changed {
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
    }
}

/// A single paint boundary prevents either moving surface from being clipped
/// by individual rows, section labels, or the gaps between separate columns.
pub struct TabSurface {
    inner: gpui::AnyElement,
    tabs: TabGroup,
}
impl IntoElement for TabSurface {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl gpui::Element for TabSurface {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        let _observers = window.use_keyed_state(
            format!("tab-surface-{:?}", self.tabs.hover.entity_id()),
            cx,
            |_, cx| {
                (
                    cx.observe(&self.tabs.hover, |_, _, cx| cx.notify()),
                    cx.observe(&self.tabs.active, |_, _, cx| cx.notify()),
                )
            },
        );
        (self.inner.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let hover_moving = self.tabs.hover.update(cx, |state, cx| {
            state.paint(
                INTERACTION.neutral_hover,
                crate::controls::FIELD_RADIUS,
                bounds,
                window,
                cx,
            )
        });
        self.inner.paint(window, cx);
        let active_moving = self.tabs.active.update(cx, |state, cx| {
            state.paint(BRAND_ACCENT, 1., bounds, window, cx)
        });
        if hover_moving || active_moving {
            window.request_animation_frame();
        }
    }
}

/// GPUI tab traversal is explicit; consume only Tab and preserve activation keys.
pub fn keyboard_navigation(
    event: &gpui::KeyDownEvent,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    if event.keystroke.key == "tab"
        && !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.platform
        && !event.keystroke.modifiers.alt
    {
        if event.keystroke.modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        cx.stop_propagation();
    }
}
