//! Native Drive for immutable task submissions. Live workspace files are never a preview source.
use super::*;
use crate::api::Artifact;
use zork_client_core::pages::{ContentCatalog, ContentKind, ConversationContents};

mod content;
pub(in crate::views) use content::tab_id as content_tab_id;
mod preview;

#[derive(Default)]
pub(super) struct DriveState {
    pub(super) items: Arc<Vec<Artifact>>,
    pub(super) pages: Arc<zork_client_core::pages::PageCatalog>,
    pub(super) contents: Arc<ContentCatalog>,
    tabs: HashMap<(String, ContentKind), content::ContentTabState>,
    files_session: Option<String>,
    files_button_bounds: Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    selected: Option<Artifact>,
    viewer: preview::PreviewState,
    preview_focused: bool,
    bytes: Option<Arc<Vec<u8>>>,
    image: Option<Arc<gpui::Image>>,
    preview_failed: bool,
    request: u64,
    saving: bool,
    choosing_save: bool,
    notice: Option<&'static str>,
}

impl RootView {
    pub(super) fn select_artifact(&mut self, artifact: Artifact, cx: &mut Context<Self>) {
        self.reset_preview_group(artifact.clone());
        self.load_artifact_preview(artifact, cx);
    }

    fn save_artifact_copy(&mut self, cx: &mut Context<Self>) {
        let Some(artifact) = self.drive.selected.clone() else {
            return;
        };
        let Some(bytes) = self.drive.bytes.clone() else {
            return;
        };
        if self.drive.saving || self.drive.choosing_save {
            return;
        }
        let saved_id = artifact.artifact_id.clone();
        let directory = std::path::Path::new(&artifact.workspace);
        let receiver = cx.prompt_for_new_path(directory, Some(&artifact.name));
        self.drive.choosing_save = true;
        self.drive.notice = None;
        cx.spawn(async move |this, cx| {
            let result = match receiver.await {
                Ok(Ok(Some(path))) => {
                    let _ = this.update(cx, |v, cx| {
                        v.drive.choosing_save = false;
                        v.drive.saving = true;
                        zork_ui::components::region::invalidate_all(cx);
                    });
                    Some(
                        cx.background_executor()
                            .spawn(async move { save_snapshot(&path, &bytes) })
                            .await,
                    )
                }
                Ok(Ok(None)) => None,
                _ => Some(Err(std::io::Error::other("save picker failed"))),
            };
            this.update(cx, |v, cx| {
                v.drive.saving = false;
                v.drive.choosing_save = false;
                if v.drive
                    .selected
                    .as_ref()
                    .is_some_and(|a| a.artifact_id == saved_id)
                {
                    v.drive.notice = result.map(|result| {
                        if result.is_ok() {
                            "drive_saved"
                        } else {
                            "drive_save_failed"
                        }
                    });
                }
                zork_ui::components::region::invalidate_all(cx);
            })
            .ok();
        })
        .detach();
        zork_ui::components::region::invalidate_all(cx);
    }
}

pub(super) fn file_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

use zork_client_core::file_io::save_snapshot;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_snapshots_render_as_images_including_legacy_media_types() {
        let bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="16"><rect width="32" height="16" fill="#ff0000"/></svg>"##;
        for media_type in ["image/svg+xml", "text/plain", "application/octet-stream"] {
            let format =
                super::super::files::content::image_format("drawing.SVG", media_type).unwrap();
            let image = gpui::Image::from_bytes(format, bytes.to_vec())
                .to_image_data(gpui::SvgRenderer::new(Arc::new(
                    crate::assets::EmbeddedAssets,
                )))
                .unwrap();
            assert_eq!(
                image.size(0),
                gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(32))
            );
            assert_eq!(&image.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        }
        assert!(super::super::files::content::image_format("drawing.txt", "text/plain").is_none());
    }

    #[test]
    fn saving_a_snapshot_replaces_only_the_selected_file_and_cleans_failed_temporary_files() {
        let dir = std::env::temp_dir().join(format!("zork-save-test-{}", ulid::Ulid::new()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("report.txt");
        std::fs::write(&file, "old").unwrap();
        save_snapshot(&file, b"saved snapshot").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"saved snapshot");
        let directory = dir.join("folder");
        std::fs::create_dir(&directory).unwrap();
        assert!(save_snapshot(&directory, b"must not replace a directory").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

impl RootView {
    fn conversation_contents(&self) -> Arc<ConversationContents> {
        self.drive
            .contents
            .get(&self.selected_session)
            .cloned()
            .unwrap_or_default()
    }
    pub(super) fn open_page(
        &mut self,
        page: zork_client_core::pages::PageLink,
        cx: &mut Context<Self>,
    ) {
        self.close_conversation_files();
        let host = self.browser_host();
        self.browser.update(cx, |browser, cx| {
            browser.set_host(host, cx);
            browser.set_connection(self.client.clone(), self.selected_session.clone());
            browser.open_shared_link(page.url, cx);
        });
        zork_ui::components::region::invalidate_all(cx);
    }

    pub(super) fn sync_conversation_files(&mut self) {
        if self.drive.files_session != self.selected_session {
            self.drive.files_session = None;
        }
    }

    pub(super) fn has_conversation_files(&self) -> bool {
        self.drive.files_session.is_some()
            && self.drive.files_session == self.selected_session
            && matches!(self.shell.route(), ShellRoute::Task(_))
    }

    pub(super) fn close_conversation_files(&mut self) -> bool {
        self.drive.files_session.take().is_some()
    }

    pub(super) fn render_conversation_files_button(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self.conversation_contents().is_empty() {
            return div().into_any_element();
        }
        let label = self.locale.text("conversation_contents").to_owned();
        let bounds = self.drive.files_button_bounds.clone();
        crate::desktop::ui::quiet_button(
            "conversation-files-button",
            "",
            true,
            crate::desktop::ui::IconButtonSize::Compact,
        )
        .relative()
        .bg(rgb(CUE_UI.palette.canvas))
        .text_size(px(11.))
        .text_color(rgb(DIM))
        .child(crate::desktop::ui::icon("icons/file.svg", 14.))
        .child(label.clone())
        .child(
            gpui::canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        )
        .on_click(cx.listener(|v, _, window, cx| {
            v.drive.files_session = if v.has_conversation_files() {
                None
            } else {
                v.selected_session.clone()
            };
            window.focus(&v.overlay_focus, cx);
            zork_ui::components::region::invalidate_all(cx);
        }))
        .automation(AutomationRole::Button, label)
        .into_any_element()
    }

    pub(super) fn render_conversation_files(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let contents = self.conversation_contents();
        let preview_rows = ((window.viewport_size().height.as_f32() - 212.) / 104.)
            .floor()
            .clamp(1., 3.) as usize;
        let groups = [ContentKind::Page, ContentKind::File]
            .into_iter()
            .map(|kind| {
                let entries = contents.entries(kind);
                let title = self.locale.text(content::title_key(kind));
                div()
                    .id(format!("{}-preview", content::tab_id(kind)))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(28.))
                            .pl_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{title} · {}", entries.len())),
                            )
                            .child(
                                crate::desktop::ui::quiet_button(
                                    content::all_id(kind),
                                    self.locale.text("content_view_all"),
                                    true,
                                    crate::desktop::ui::IconButtonSize::Compact,
                                )
                                .text_size(px(11.))
                                .on_click(
                                    cx.listener(move |v, _, _, cx| v.open_content_tab(kind, cx)),
                                )
                                .automation(
                                    AutomationRole::Button,
                                    format!("{} {title}", self.locale.text("content_view_all")),
                                ),
                            ),
                    )
                    .when(entries.is_empty(), |v| {
                        v.child(div().h(px(36.)).px_3().flex().items_center().child(
                            crate::desktop::ui::text_role(
                                self.locale.text(match kind {
                                    ContentKind::Page => "content_pages_empty",
                                    ContentKind::File => "content_files_empty",
                                }),
                                zork_ui::design::TextRole::Description,
                            ),
                        ))
                    })
                    .children(entries.iter().take(preview_rows).copied().map(|entry| {
                        div()
                            .h(px(52.))
                            .pb_1()
                            .child(self.render_content_row(entry, false, cx))
                    }))
                    .automation(AutomationRole::Status, title)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let title = self.locale.text("conversation_contents");
        let panel = div()
            .id("conversation-files-panel")
            .w(px(384.))
            .max_w_full()
            .min_w_0()
            .occlude()
            .rounded(px(crate::desktop::ui::MENU_RADIUS))
            .p_2()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CUE_UI.palette.elevated))
            .shadow(vec![BoxShadow::new(
                px(0.),
                px(8.),
                rgba(0x24272b14).into(),
            )
            .blur_radius(px(24.))])
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|v, event: &gpui::MouseDownEvent, _, cx| {
                if v.drive
                    .files_button_bounds
                    .get()
                    .is_none_or(|bounds| !bounds.contains(&event.position))
                {
                    v.close_conversation_files();
                    zork_ui::components::region::invalidate_all(cx);
                }
            }))
            .child(
                div()
                    .h(px(40.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title)
                    .child(div().flex_1())
                    .child(
                        crate::desktop::ui::icon_button_sized(
                            "conversation-files-close",
                            true,
                            crate::desktop::ui::IconButtonSize::Compact,
                        )
                        .child(crate::desktop::ui::icon("icons/x.svg", 14.))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.close_conversation_files();
                            zork_ui::components::region::invalidate_all(cx);
                        }))
                        .automation(
                            AutomationRole::Button,
                            self.locale.text("conversation_files_close"),
                        ),
                    ),
            )
            .child(div().pt_1().flex().flex_col().gap_2().children(groups))
            .automation(AutomationRole::Status, title);
        // Position in the chat column and share the entry button's right inset.
        div()
            .absolute()
            .top(px(56.))
            .left(px(16.))
            .right(px(self.panel_tools_right_inset(cx)))
            .flex()
            .justify_end()
            .child(panel)
    }
    pub(super) fn focus_artifact_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_preview_focus(window, cx);
    }
    pub(super) fn has_conversation_artifact_preview(&self) -> bool {
        matches!(self.shell.route(), ShellRoute::Task(_)) && self.drive.selected.is_some()
    }
    pub(super) fn close_conversation_artifact(&mut self) {
        {
            self.drive.selected = None;
            self.drive.bytes = None;
            self.drive.image = None;
            self.drive.request = self.drive.request.wrapping_add(1);
        }
    }
}

#[cfg(feature = "headless-bench")]
impl RootView {
    pub(super) fn benchmark_seed_artifacts(&mut self, count: usize) {
        let png = include_bytes!("../../../zork-ui/assets/app/icon.png");
        let items = (0..count)
            .filter(|index| super::benchmark::artifact_is_image(*index).is_some())
            .map(|index| {
                let image = super::benchmark::artifact_is_image(index).unwrap();
                Artifact {
                    artifact_id: format!("stress-artifact-{index}"),
                    task_id: Some("render-fixture".into()),
                    session_id: Some("render-fixture".into()),
                    task_title: "压力测试附件".into(),
                    workspace: "/fixture".into(),
                    name: format!("artifact-{index}.{}", if image { "png" } else { "txt" }),
                    source_path: String::new(),
                    media_type: if image {
                        "image/png".into()
                    } else {
                        "text/plain".into()
                    },
                    caption: None,
                    byte_len: if image { png.len() as i64 } else { 256 },
                    version: 1,
                    created_at: "2026-09-07T08:00:00Z".into(),
                }
            })
            .collect();
        self.drive.items = Arc::new(items);
        self.drive.contents = Arc::new(zork_client_core::pages::content_indices(
            &self.drive.items,
            &self.drive.pages,
        ));
    }

    pub fn benchmark_preview_artifact(&mut self, kind: &str, cx: &mut Context<Self>) {
        let image = matches!(kind, "png" | "jpeg" | "svg");
        let Some(mut artifact) = self
            .drive
            .items
            .iter()
            .find(|item| (item.media_type == "image/png") == image)
            .cloned()
        else {
            return;
        };
        let png = include_bytes!("../../../zork-ui/assets/app/icon.png");
        let (media, bytes) = match kind {
            "png" => ("image/png", png.to_vec()),
            "svg" => (
                "image/svg+xml",
                include_bytes!("../../../zork-ui/assets/avatars/portraits/fox.svg").to_vec(),
            ),
            "jpeg" => {
                let mut bytes = std::io::Cursor::new(Vec::new());
                image::load_from_memory(png)
                    .expect("fixture PNG")
                    .to_rgb8()
                    .write_to(&mut bytes, image::ImageFormat::Jpeg)
                    .expect("fixture JPEG");
                ("image/jpeg", bytes.into_inner())
            }
            "markdown" => (
                "text/markdown",
                include_bytes!("../../tests/fixtures/markdown-scroll.md").to_vec(),
            ),
            "binary" => ("application/octet-stream", vec![0; 1024]),
            _ => (
                "text/plain",
                "第一行：真实文本附件预览。\n第二行：中文与 UTF-8 🐈。\n"
                    .repeat(32)
                    .into_bytes(),
            ),
        };
        artifact.name = format!("fixture.{kind}");
        artifact.media_type = media.into();
        artifact.byte_len = bytes.len() as i64;
        self.seed_preview_fixture(artifact, bytes, kind, cx);
        zork_ui::components::region::invalidate_all(cx);
    }

    pub fn benchmark_close_artifact(&mut self, cx: &mut Context<Self>) {
        self.close_conversation_artifact();
        zork_ui::components::region::invalidate_all(cx);
    }
    pub fn benchmark_artifact_count(&self) -> usize {
        self.drive.items.len()
    }
}
