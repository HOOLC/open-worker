//! Coalesce invalidations at a window's frame boundary. This knows only GPUI
//! scheduling; the owner decides which read-only presentation sources to drain.
use gpui::{AnyWindowHandle, AppContext, AsyncApp, Context, WeakEntity, Window};

#[derive(Default)]
pub struct FrameDelivery {
    window: Option<AnyWindowHandle>,
    pending: bool,
    scheduled: bool,
}

impl FrameDelivery {
    /// Called at render entry. A newly attached view catches up even if it had
    /// no window when the invalidation arrived, or its old window was closed.
    pub fn enter(&mut self, window: &Window) -> bool {
        let handle = window.window_handle();
        if self.window != Some(handle) {
            self.window = Some(handle);
            self.scheduled = false;
        }
        if self.pending && !self.scheduled {
            self.pending = false;
            true
        } else {
            false
        }
    }

    pub fn request<T: 'static>(
        owner: &WeakEntity<T>,
        cx: &mut AsyncApp,
        access: fn(&mut T) -> &mut Self,
        deliver: fn(&mut T, &mut Context<T>),
    ) -> bool {
        let requested = owner.update(cx, |view, _| {
            let delivery = access(view);
            if delivery.pending {
                false
            } else {
                delivery.pending = true;
                true
            }
        });
        let Ok(requested) = requested else {
            return false;
        };
        if !requested {
            return true;
        }
        let target = owner.clone();
        let scheduled = cx.with_window(owner.entity_id(), |window, cx| {
            let handle = window.window_handle();
            let _ = target.update(cx, |view, _| {
                let delivery = access(view);
                delivery.window = Some(handle);
                delivery.scheduled = true;
            });
            let target = target.clone();
            window.on_next_frame(move |_, cx| {
                let _ = target.update(cx, |view, cx| {
                    let delivery = access(view);
                    if delivery.window != Some(handle) || !delivery.scheduled {
                        return;
                    }
                    delivery.pending = false;
                    delivery.scheduled = false;
                    deliver(view, cx);
                });
            });
            // on_next_frame already wakes the platform's frame source. The
            // animation helper requires a currently rendering view, which a
            // subscription callback outside the paint phase does not have.
        });
        if scheduled.is_none() {
            // No window is registered before the first render, or while a
            // retained view is hidden. enter() will drain it when attached.
            return owner.update(cx, |_, cx| cx.notify()).is_ok();
        }
        true
    }
}
