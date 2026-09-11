use gpui::{
    div, prelude::*, px, rgb, AppContext, Context, Entity, HeadlessAppContext, Render, Window,
    WindowHandle,
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use zork_gui::{
    assets::EmbeddedAssets,
    automation::{
        protocol::{ElementInfo, UserAction},
        AutomationRoot, HeadlessAutomation,
    },
    desktop::{HeadlessAgentsView, HeadlessProfilesView},
};

struct SettingsFrame<V: Render + 'static> {
    inner: Entity<V>,
}
impl<V: Render + 'static> Render for SettingsFrame<V> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .font_family("Inter Variable")
            .text_size(px(13.))
            .bg(rgb(0xF7F6F2))
            .child(
                div()
                    .id("settings-scroll")
                    .ml(px(240.))
                    .h_full()
                    .overflow_y_scroll()
                    .bg(rgb(0xFFFFFF))
                    .child(zork_gui::desktop::headless_settings_content(
                        self.inner.clone(),
                    )),
            )
    }
}
struct Fixture<V: Render + 'static> {
    view: Entity<V>,
    cx: HeadlessAppContext,
    window: WindowHandle<AutomationRoot<SettingsFrame<V>>>,
    driver: HeadlessAutomation,
}
impl<V: Render + 'static> Fixture<V> {
    fn new(
        width: f32,
        height: f32,
        make: impl FnOnce(&mut Context<V>) -> V,
    ) -> anyhow::Result<Self> {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_platform::current_platform(true).text_system(),
            Arc::new(EmbeddedAssets),
            gpui_platform::current_headless_renderer,
        );
        let driver = cx.update(|cx| {
            zork_gui::assets::init_fonts(cx);
            zork_gui::components::init(cx);
            // Layout/focus contracts inspect endpoints; test_motion.py samples animated frames.
            cx.set_reduce_motion(true);
            HeadlessAutomation::install(cx)
        });
        let mut view = None;
        let window = cx.open_window(gpui::size(px(width), px(height)), |_, cx| {
            let inner = cx.new(make);
            view = Some(inner.clone());
            let frame = cx.new(|_| SettingsFrame { inner });
            cx.new(|_| AutomationRoot::new(frame))
        })?;
        cx.update_window(window.into(), |_, w, cx| w.draw(cx).clear(cx))?;
        cx.run_until_parked();
        Ok(Self {
            cx,
            window,
            view: view.unwrap(),
            driver,
        })
    }
    fn element(&self, id: &str) -> Option<ElementInfo> {
        self.driver
            .snapshot(false)
            .elements
            .into_iter()
            .find(|e| e.id == id)
    }
    fn action(&mut self, value: serde_json::Value) -> anyhow::Result<()> {
        let action: UserAction = serde_json::from_value(value)?;
        self.cx.update_window(self.window.into(), |_, w, cx| {
            self.driver.dispatch(action, w, cx)
        })??;
        self.cx.run_until_parked();
        Ok(())
    }
    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        self.action(json!({"type":"click","target":{"element_id":id}}))
    }
    fn key(&mut self, key: &str) -> anyhow::Result<()> {
        self.action(json!({"type":"key","keystroke":key}))
    }
    fn modal(&self, id: &str, width: f32, height: f32) -> anyhow::Result<()> {
        let card = self
            .element(id)
            .ok_or_else(|| anyhow::anyhow!("missing modal {id}"))?;
        anyhow::ensure!(
            card.bounds.x >= 19.
                && card.bounds.y >= 19.
                && card.bounds.x + card.bounds.width <= width - 19.
                && card.bounds.y + card.bounds.height <= height - 19.,
            "modal outside viewport: {:?}",
            card.bounds
        );
        anyhow::ensure!(
            card.bounds == card.visible_bounds,
            "modal was clipped by settings scroll container"
        );
        anyhow::ensure!(
            (card.bounds.width - 540.).abs() < 1.,
            "modal width drifted from the shared desktop design"
        );
        let footer = self
            .element(&format!("{id}-footer"))
            .expect("modal must keep a fixed action area");
        anyhow::ensure!(
            footer.bounds == footer.visible_bounds
                && footer.bounds.y + footer.bounds.height <= card.bounds.y + card.bounds.height,
            "modal actions are clipped"
        );
        Ok(())
    }
    fn screenshot(&mut self, name: &str) -> anyhow::Result<()> {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../artifacts/headless-interactions/modals");
        std::fs::create_dir_all(&out)?;
        // SVG decoding is asynchronous. Require painted avatars and stable pixels,
        // without advancing the virtual clock (which would animate the text caret).
        let started = std::time::Instant::now();
        let mut previous = None;
        let mut stable = 0;
        loop {
            self.cx.run_until_parked();
            self.cx
                .update_window(self.window.into(), |_, w, cx| w.draw(cx).clear(cx))?;
            let pixels = self.cx.capture_screenshot(self.window.into())?;
            let snapshot = self.driver.snapshot(false);
            let scale = snapshot.scale_factor;
            let avatars = [
                "cat", "bunny", "bear", "fox", "panda", "chick", "dog", "owl", "koala", "penguin",
                "deer", "octopus",
            ];
            let ready = snapshot
                .elements
                .iter()
                .filter(|e| {
                    avatars.iter().any(|a| e.id == format!("agent-avatar-{a}")) && e.visible
                })
                .all(|e| {
                    let colors: std::collections::HashSet<_> = (-8..8)
                        .flat_map(|dy| (-8..8).map(move |dx| (dx, dy)))
                        .map(|(dx, dy)| {
                            let x = ((e.center.x + dx as f32) * scale)
                                .clamp(0., (pixels.width() - 1) as f32)
                                as u32;
                            let y = ((e.center.y + dy as f32) * scale)
                                .clamp(0., (pixels.height() - 1) as f32)
                                as u32;
                            pixels.get_pixel(x, y).0
                        })
                        .collect();
                    colors.len() > 4
                });
            if ready && previous.as_ref() == Some(pixels.as_raw()) {
                stable += 1;
            } else {
                stable = 0;
            }
            previous = Some(pixels.as_raw().clone());
            if stable >= 2 && started.elapsed().as_millis() >= 100 {
                pixels.save(out.join(name))?;
                std::fs::write(
                    out.join(name.replace(".png", ".json")),
                    serde_json::to_vec_pretty(&snapshot)?,
                )?;
                break;
            }
            anyhow::ensure!(
                started.elapsed().as_secs() < 5,
                "screenshot did not settle or has unloaded SVGs: {name}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }
}
fn main() -> anyhow::Result<()> {
    std::env::set_var("SEED", "0");
    let state = tempfile::tempdir()?;
    std::env::set_var(
        "ZORK_GUI_PREFERENCES_PATH",
        state.path().join("preferences.json"),
    );
    for (width, height) in [(900., 600.), (1280., 800.)] {
        let mut f = Fixture::new(width, height, |cx| {
            HeadlessProfilesView::headless_fixture(false, cx)
        })?;
        anyhow::ensure!(
            f.element("profile-create-dialog").is_none(),
            "create form opened without user action"
        );
        let opener = f.element("profile-add").unwrap().center;
        f.click("profile-add")?;
        f.modal("profile-create-dialog", width, height)?;
        f.screenshot(&format!("connection-{width}.png"))?;
        anyhow::ensure!(
            (f.element("profile-id").unwrap().bounds.height - 32.).abs() < 0.1,
            "field height drift"
        );
        f.click("profile-id")?;
        f.action(json!({"type":"type_text","text":"固定连接"}))?;
        anyhow::ensure!(
            f.view
                .read_with(&f.cx, |v, cx| v.headless_connection_name(cx))
                == "固定连接",
            "modal field did not receive input"
        );
        f.key("escape")?;
        anyhow::ensure!(
            f.element("profile-create-dialog").is_none(),
            "Escape did not dismiss modal"
        );
        f.click("profile-add")?;
        f.action(json!({"type":"click","target":{"x":opener.x,"y":opener.y}}))?;
        anyhow::ensure!(
            f.element("profile-create-dialog").is_none(),
            "backdrop click leaked to underlying add button"
        );
        let mut f = Fixture::new(width, height, |cx| {
            HeadlessProfilesView::headless_fixture(true, cx)
        })?;
        f.click("profile-model-add")?;
        f.modal("model-editor-dialog", width, height)?;
        f.click("profile-model")?;
        f.action(json!({"type":"type_text","text":"copied-model"}))?;
        f.click("model-copy-select")?;
        f.click("model-copy-0")?;
        anyhow::ensure!(
            f.element("model-copy-select-menu").is_none(),
            "copy menu remained open after choosing a model"
        );
        let state = f.view.read_with(&f.cx, |view, cx| view.headless_state(cx));
        anyhow::ensure!(
            state["model_id"] == "copied-model",
            "copy replaced the new model identity"
        );
        anyhow::ensure!(
            state["context_window"] == "32K" && state["max_output_tokens"] == "4.096K",
            "copy did not populate token limits: {state}"
        );
        let card = f.element("model-editor-dialog").unwrap().bounds;
        for id in [
            "model-copy-select",
            "profile-model-cancel",
            "profile-model-save",
        ] {
            let action = f.element(id).unwrap();
            anyhow::ensure!(
                action.bounds == action.visible_bounds
                    && action.bounds.x >= card.x + 24.
                    && action.bounds.x + action.bounds.width <= card.x + card.width - 24.,
                "{id} overflowed the model dialog: {:?}",
                action.bounds
            );
        }
        f.screenshot(&format!("model-{width}.png"))?;
        f.click("model-editor-dialog-close")?;
        anyhow::ensure!(
            f.element("model-editor-dialog").is_none(),
            "close button did not dismiss model editor"
        );
        f.click("model-edit-fixture-model")?;
        f.modal("model-editor-dialog", width, height)?;
        f.key("escape")?;
        let mut f = Fixture::new(width, height, HeadlessAgentsView::headless_fixture)?;
        f.click("agent-add")?;
        f.modal("agent-create-dialog", width, height)?;
        anyhow::ensure!(
            f.element("agent-profile-select").unwrap().label == "自动分配",
            "new agents must default to the pool"
        );
        f.screenshot(&format!("agent-create-{width}.png"))?;
        anyhow::ensure!(
            f.element("agent-create")
                .is_some_and(|e| e.visible_bounds == e.bounds),
            "create action hidden below scroll area"
        );
        f.key("escape")?;
        anyhow::ensure!(
            f.element("agent-create-dialog").is_none(),
            "Escape did not dismiss Agent creator"
        );
        f.click("agent-settings-leader")?;
        f.modal("agent-editor-dialog", width, height)?;
        let model_before = f.element("agent-edit-model-select").unwrap().bounds;
        let effort = f.element("agent-edit-thinking-select").unwrap();
        let profile = f.element("agent-edit-profile-select").unwrap();
        anyhow::ensure!(
            model_before.y < effort.bounds.y && effort.bounds.y < profile.bounds.y,
            "model and effort must precede optional connection"
        );
        let model_label = f.element("agent-edit-model-select").unwrap().label.clone();
        let effort_label = effort.label.clone();
        f.click("agent-edit-profile-select")?;
        let option = f
            .element("agent-edit-profile-0")
            .expect("dropdown options must be visible");
        anyhow::ensure!(
            option.label == "自动分配",
            "automatic account option missing"
        );
        anyhow::ensure!(option.visible_bounds == option.bounds, "dropdown clipped");
        anyhow::ensure!(
            f.element("agent-edit-model-select").unwrap().bounds == model_before,
            "dropdown reflowed form"
        );
        let select = f.element("agent-edit-profile-select").unwrap().bounds;
        let menu = f.element("agent-edit-profile-select-menu").unwrap().bounds;
        anyhow::ensure!(
            (menu.y - select.y - select.height - 6.).abs() < 0.1
                && (menu.x - select.x + 4.).abs() < 0.1
                && (menu.width - select.width - 8.).abs() < 0.1,
            "dropdown must retain its 6 px gap and 4 px outward inset"
        );
        anyhow::ensure!(
            (option.bounds.y - menu.y - 9.).abs() < 0.1,
            "dropdown options must retain the 8 px padding inside the border"
        );
        f.screenshot(&format!("agent-dropdown-{width}.png"))?;
        f.click("agent-edit-profile-0")?;
        anyhow::ensure!(
            f.element("agent-edit-model-select").unwrap().label == model_label
                && f.element("agent-edit-thinking-select").unwrap().label == effort_label,
            "connection change reset model or effort"
        );
        f.click("agent-edit-thinking-select")?;
        f.click("agent-edit-thinking-0")?;
        f.click("agent-edit-model-select")?;
        f.click("agent-edit-model-0")?;
        f.screenshot(&format!("agent-edit-{width}.png"))?;
        f.click("agent-editor-dialog-close")?;
        anyhow::ensure!(
            f.element("agent-editor-dialog").is_none(),
            "Agent editor did not close"
        );
    }
    println!(
        "Headless modal opening, clipping, input, Escape, close button and backdrop checks passed at both sizes."
    );
    Ok(())
}
