//! Move the capped column's centering space with the panel, rather than
//! spending all that space in its first frames and only then shrinking text.
use crate::browser::PanelMotion;
use gpui::{prelude::*, Context, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent};
use std::time::Instant;

impl super::RootView {
    pub(super) fn panel_resize_events(&self, cx: &Context<Self>) -> impl IntoElement {
        let root = cx.entity().downgrade();
        gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                // The divider occludes the root hitbox. A hover-filtered root
                // listener therefore loses slow moves while the pointer stays on
                // the divider. Track an active resize in the window capture phase.
                let moving = root.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moving.update(cx, |view, cx| {
                            if view.browser.read(cx).is_resizing() {
                                view.browser.update(cx, |panel, cx| {
                                    panel.resize_panel(
                                        (window.viewport_size().width - event.position.x).as_f32(),
                                        cx,
                                    )
                                });
                            }
                        });
                    }
                });
                let released = root.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        let _ = released.update(cx, |view, cx| {
                            view.browser.update(cx, |panel, _| panel.finish_resize());
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }
}

#[derive(Default)]
pub(super) struct Centering {
    available: Option<f32>,
    motion: Option<PanelMotion>,
}

impl Centering {
    pub(super) fn sample(
        &mut self,
        available: f32,
        panel: f32,
        target_panel: f32,
        column: f32,
        now: Instant,
        snap: bool,
    ) -> f32 {
        let target = (available - target_panel - column).max(0.);
        let resized = self.available.replace(available) != Some(available);
        let motion = self
            .motion
            .get_or_insert_with(|| PanelMotion::stationary(target, now));
        motion.retarget(target, now, snap || resized);
        (available - panel - motion.sample(now).0.max(0.)).max(0.)
    }
}
