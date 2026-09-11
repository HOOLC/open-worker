//! Loading animation lifecycle: delay, motion, clipping, completion and accessibility.
use gpui::{div, prelude::*, px, rgb, AppContext, Context, HeadlessAppContext, Render, Window};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};
use zork_gui::assets::EmbeddedAssets;

struct Fixture {
    show: bool,
    dots: bool,
    animate: bool,
    clipped: bool,
    draws: Rc<Cell<usize>>,
}
impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.draws.set(self.draws.get() + 1);
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x24272b))
            .child(
                div().size(px(100.)).overflow_hidden().child(
                    div()
                        .when(self.clipped, |v| v.mt(px(200.)))
                        .when(self.show, |v| {
                            v.child(if self.dots {
                                zork_ui::components::loading::activity("test", self.animate)
                            } else {
                                zork_ui::components::loading::indicator("test", 24.)
                            })
                        }),
                ),
            )
    }
}
fn frame(
    cx: &mut HeadlessAppContext,
    window: gpui::AnyWindowHandle,
    force: bool,
) -> anyhow::Result<usize> {
    cx.update_window(window, |_, window, cx| {
        let requests = window.simulate_next_frame(cx);
        if force || requests > 0 {
            window.draw(cx).clear(cx);
        }
        requests
    })
}
fn main() -> anyhow::Result<()> {
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/loading/headless");
    std::fs::create_dir_all(&output)?;
    for (reduced, dots) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_platform::current_platform(true).text_system(),
            Arc::new(EmbeddedAssets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            zork_gui::assets::init_fonts(cx);
            cx.set_reduce_motion(reduced);
        });
        let draws = Rc::new(Cell::new(0));
        let mut root = None;
        let window = cx.open_window(gpui::size(px(200.), px(120.)), |_, cx| {
            let v = cx.new(|_| Fixture {
                show: true,
                dots,
                animate: true,
                clipped: false,
                draws: draws.clone(),
            });
            root = Some(v.clone());
            v
        })?;
        let root = root.unwrap();
        cx.update_window(window.into(), |_, w, cx| w.draw(cx).clear(cx))?;
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        let initial = cx.capture_screenshot(window.into())?;
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        anyhow::ensure!(
            initial == cx.capture_screenshot(window.into())?,
            "short request flashed"
        );
        cx.advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        let shown = cx.capture_screenshot(window.into())?;
        anyhow::ensure!(initial != shown, "indicator did not appear after delay");
        shown.save(output.join(format!(
            "{}-{}.png",
            if dots { "dots" } else { "ring" },
            if reduced { "reduced" } else { "motion" }
        )))?;
        for _ in 0..8 {
            cx.advance_clock(Duration::from_millis(34));
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
        }
        let moved = cx.capture_screenshot(window.into())?;
        anyhow::ensure!((shown == moved) == reduced, "motion preference was ignored");
        if dots && !reduced {
            root.update(&mut cx, |v, cx| {
                v.animate = false;
                cx.notify();
            });
            frame(&mut cx, window.into(), true)?;
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
            cx.advance_clock(Duration::from_millis(250));
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
            let before = draws.get();
            cx.advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
            anyhow::ensure!(
                draws.get() == before,
                "scroll-paused activity kept repainting"
            );
            root.update(&mut cx, |v, cx| {
                v.animate = true;
                cx.notify();
            });
            frame(&mut cx, window.into(), true)?;
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
        }
        // Once clipped, no persistent timer should keep repainting the window.
        root.update(&mut cx, |v, cx| {
            v.clipped = true;
            cx.notify();
        });
        frame(&mut cx, window.into(), true)?;
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        let before = draws.get();
        for _ in 0..12 {
            cx.advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
        }
        anyhow::ensure!(draws.get() == before, "clipped indicator kept repainting");
        root.update(&mut cx, |v, cx| {
            v.clipped = false;
            cx.notify();
        });
        frame(&mut cx, window.into(), true)?;
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        root.update(&mut cx, |v, cx| {
            v.show = false;
            cx.notify();
        });
        frame(&mut cx, window.into(), true)?;
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        frame(&mut cx, window.into(), false)?;
        let before = draws.get();
        for _ in 0..12 {
            cx.advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            frame(&mut cx, window.into(), false)?;
        }
        anyhow::ensure!(draws.get() == before, "completed indicator kept repainting");
        println!("PASS loading: reduced={reduced}, dots={dots}, delay / animation / clipping / completion");
    }
    Ok(())
}
