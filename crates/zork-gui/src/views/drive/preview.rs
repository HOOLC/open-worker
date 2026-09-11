//! Read-only attachment viewer. File grouping belongs to the opening message;
//! byte snapshots, transforms and asynchronous image work have separate lifetimes.
use super::super::files::content::{self, Kind};
use super::super::files::image::{self as images, DecodedImage};
use super::*;
use crate::components::message::{render_selectable_document, MessageDocument};
use zork_ui::controls as ui;

fn preview_icon(id: impl Into<gpui::ElementId>, enabled: bool) -> gpui::Stateful<Div> {
    ui::icon_button(id, enabled).focus_visible(|v| {
        v.border_color(rgba(0))
            .bg(rgb(CUE_UI.palette.sidebar_hover))
    })
}
fn preview_quiet(
    id: impl Into<gpui::ElementId>,
    text: impl Into<gpui::SharedString>,
    enabled: bool,
    size: ui::IconButtonSize,
) -> gpui::Stateful<Div> {
    ui::quiet_button(id, text, enabled, size).focus_visible(|v| {
        v.border_color(rgba(0))
            .bg(rgb(CUE_UI.palette.sidebar_hover))
    })
}

#[derive(Default)]
pub(super) struct PreviewState {
    group: Arc<Vec<Artifact>>,
    index: usize,
    image: Option<DecodedImage>,
    image_failed: bool,
    overview: bool,
    text: Option<Arc<str>>,
    document: Option<MessageDocument>,
    source_document: Option<MessageDocument>,
    text_truncated: bool,
    selection: Rc<std::cell::RefCell<crate::components::selection::TranscriptSelection>>,
    source: bool,
    more: bool,
    info: bool,
    more_position: Option<gpui::Point<gpui::Pixels>>,
    zoom: Option<f32>,
    scroll: gpui::ScrollHandle,
    drag: Option<(gpui::Point<gpui::Pixels>, gpui::Point<gpui::Pixels>)>,
    focus: Option<FocusHandle>,
    return_focus: Option<FocusHandle>,
    view_size: gpui::Size<f32>,
    target_raster_width: u32,
    raster_request: u64,
}
impl PreviewState {
    fn scale(&self) -> f32 {
        self.zoom.unwrap_or_else(|| {
            self.image
                .as_ref()
                .map(|image| {
                    ((self.view_size.width - 64.) / image.size.width)
                        .min((self.view_size.height - 64.) / image.size.height)
                        .max(0.01)
                })
                .unwrap_or(1.)
        })
    }
    fn reset_file(&mut self, cx: &mut gpui::App) {
        if let Some(image) = self.image.take() {
            cx.drop_image(image.rendered, None);
        }
        self.image_failed = false;
        self.overview = false;
        self.text = None;
        self.document = None;
        self.source_document = None;
        self.source = false;
        self.more = false;
        self.info = false;
        self.more_position = None;
        self.text_truncated = false;
        self.selection.borrow_mut().clear();
        self.zoom = None;
        self.scroll = gpui::ScrollHandle::new();
        self.drag = None;
        self.target_raster_width = 0;
        self.raster_request = self.raster_request.wrapping_add(1);
    }
}

impl RootView {
    #[cfg(feature = "headless-bench")]
    pub(super) fn seed_preview_fixture(
        &mut self,
        artifact: Artifact,
        bytes: Vec<u8>,
        kind: &str,
        cx: &mut Context<Self>,
    ) {
        self.drive.viewer.reset_file(cx);
        self.reset_preview_group(artifact.clone());
        let image = content::decode(
            &artifact.name,
            &artifact.media_type,
            &bytes,
            &cx.svg_renderer(),
            1536,
        );
        self.drive.selected = Some(artifact);
        if !matches!(kind, "loading" | "failed") {
            self.accept_preview_bytes(bytes, image, cx);
        } else {
            self.drive.bytes = None;
        }
        self.drive.preview_failed = kind == "failed";
        self.drive.preview_focused = false;
    }

    pub(super) fn reset_preview_group(&mut self, artifact: Artifact) {
        self.drive.viewer.group = Arc::new(vec![artifact]);
        self.drive.viewer.index = 0;
    }

    pub(in crate::views) fn open_artifact_group(
        &mut self,
        group: Vec<Artifact>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(artifact) = group.get(index).cloned() else {
            return;
        };
        self.drive.viewer.return_focus = window.focused(cx);
        self.drive.viewer.group = Arc::new(group);
        self.drive.viewer.index = index;
        self.load_artifact_preview(artifact, cx);
    }

    pub(super) fn load_artifact_preview(&mut self, artifact: Artifact, cx: &mut Context<Self>) {
        self.drive.files_session = None;
        self.drive.preview_focused = self.drive.selected.is_some() && self.drive.preview_focused;
        self.drive.selected = Some(artifact.clone());
        self.drive.viewer.reset_file(cx);
        self.drive.bytes = None;
        self.drive.image = None;
        self.drive.preview_failed = false;
        self.drive.notice = None;
        self.drive.request = self.drive.request.wrapping_add(1);
        let request = self.drive.request;
        let core = self.core_device.clone();
        let renderer = cx.svg_renderer();
        #[cfg(feature = "headless-bench")]
        let offline = self.benchmark_offline;
        #[cfg(not(feature = "headless-bench"))]
        let offline = false;
        let local = self.local_cache.clone();
        cx.spawn(async move |this, cx| {
            let result = if offline {
                local
                    .and_then(|(store, node)| {
                        store
                            .blob(&node, &format!("upload:{}", artifact.artifact_id))
                            .ok()
                            .flatten()
                    })
                    .ok_or_else(|| anyhow::anyhow!("snapshot unavailable"))
            } else {
                core.artifact_content(&artifact.artifact_id)
                    .await
                    .map_err(anyhow::Error::from)
            };
            let result = match result {
                Ok(bytes) => {
                    let image_bytes = bytes.clone();
                    let name = artifact.name.clone();
                    let mime = artifact.media_type.clone();
                    let image = cx
                        .background_executor()
                        .spawn(async move {
                            content::decode(&name, &mime, &image_bytes, &renderer, 1536)
                        })
                        .await;
                    Ok((bytes, image))
                }
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |v, cx| {
                if v.drive.request != request {
                    return;
                }
                match result {
                    Ok((bytes, image)) => v.accept_preview_bytes(bytes, image, cx),
                    Err(_) => v.drive.preview_failed = true,
                }
                zork_ui::components::region::invalidate_all(cx);
            });
        })
        .detach();
        zork_ui::components::region::invalidate_all(cx);
    }

    fn accept_preview_bytes(
        &mut self,
        bytes: Vec<u8>,
        content: anyhow::Result<content::Content>,
        _cx: &mut Context<Self>,
    ) {
        match content {
            Ok(content) => {
                self.drive.viewer.overview = content.overview;
                self.drive.viewer.image = content.image;
                self.drive.viewer.text_truncated = content.truncated;
                if let Some(text) = content.text {
                    self.drive.viewer.source_document = Some(MessageDocument::plain(&text));
                    self.drive.viewer.document = Some(match content.kind {
                        Kind::Markdown => MessageDocument::parse(&text),
                        Kind::Code(language) => {
                            let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                            let fence = "`".repeat(longest.max(2) + 1);
                            MessageDocument::parse(&format!("{fence}{language}\n{text}\n{fence}"))
                        }
                        _ => MessageDocument::plain(&text),
                    });
                    self.drive.viewer.text = Some(text);
                }
            }
            Err(_) => self.drive.viewer.image_failed = true,
        }
        self.drive.bytes = Some(Arc::new(bytes));
    }

    fn move_preview(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(index) = self
            .drive
            .viewer
            .index
            .checked_add_signed(step)
            .filter(|i| *i < self.drive.viewer.group.len())
        else {
            return;
        };
        let artifact = self.drive.viewer.group[index].clone();
        self.drive.viewer.index = index;
        self.load_artifact_preview(artifact, cx);
    }

    fn zoom_preview(&mut self, zoom: Option<f32>, cx: &mut Context<Self>) {
        self.drive.viewer.zoom = zoom.map(|value| value.clamp(0.1, 20.));
        self.drive
            .viewer
            .scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        zork_ui::components::region::invalidate_all(cx);
    }

    fn ensure_preview_raster(&mut self, scale_factor: f32, cx: &mut Context<Self>) {
        if self.drive.viewer.source {
            return;
        }
        let Some(image) = &self.drive.viewer.image else {
            return;
        };
        let Some(svg) = image.svg.clone() else {
            return;
        };
        let width = (image.size.width * self.drive.viewer.scale() * scale_factor)
            .ceil()
            .clamp(1., 4096.) as u32;
        if self.drive.viewer.target_raster_width == width {
            return;
        }
        self.drive.viewer.target_raster_width = width;
        self.drive.viewer.raster_request = self.drive.viewer.raster_request.wrapping_add(1);
        let request = self.drive.viewer.raster_request;
        let renderer = cx.svg_renderer();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    renderer.render_parsed(
                        &svg,
                        gpui::SvgSize::Size(gpui::size(
                            gpui::DevicePixels(width as i32),
                            gpui::DevicePixels(width as i32),
                        )),
                    )
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.drive.viewer.raster_request != request {
                    return;
                }
                if let (Ok(rendered), Some(image)) = (result, &mut v.drive.viewer.image) {
                    let old = std::mem::replace(&mut image.rendered, rendered);
                    cx.drop_image(old, None);
                    zork_ui::components::region::invalidate_all(cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn sync_preview_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_conversation_artifact_preview() && !self.drive.preview_focused {
            let focus = self
                .drive
                .viewer
                .focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            if self.drive.viewer.return_focus.is_none() {
                self.drive.viewer.return_focus = window.focused(cx);
            }
            window.focus(&focus, cx);
            self.drive.preview_focused = true;
        } else if !self.has_conversation_artifact_preview() && self.drive.preview_focused {
            if let Some(previous) = self.drive.viewer.return_focus.take() {
                window.focus(&previous, cx);
            }
            self.drive.preview_focused = false;
            self.drive.viewer.reset_file(cx);
        }
    }

    pub(in crate::views) fn render_conversation_artifact_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(artifact) = self.drive.selected.clone() else {
            return div();
        };
        let image_view = content::kind(&artifact.name, &artifact.media_type).is_image();
        let available = window.viewport_size();
        let width =
            (available.width.as_f32() - 40.).clamp(280., if image_view { f32::MAX } else { 960. });
        let height =
            (available.height.as_f32() - 40.).clamp(280., if image_view { f32::MAX } else { 760. });
        let origin = gpui::point(
            (available.width - px(width)) / 2.,
            (available.height - px(height)) / 2.,
        );
        let narrow = width < 560.;
        let gutter = if image_view {
            12.
        } else if narrow {
            20.
        } else {
            24.
        };
        let stage_height = (height - gutter * 2. - if image_view { 96. } else { 118. }).max(80.);
        self.drive.viewer.view_size = gpui::size(width - gutter * 2., stage_height);
        self.ensure_preview_raster(window.scale_factor(), cx);
        let focus = self
            .drive
            .viewer
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let key_focus = focus.clone();
        let image = self.drive.viewer.image.as_ref();
        let kind = images::kind(&artifact.name);
        let subtitle = format!(
            "{}{}{}",
            kind,
            image
                .map(|i| format!(" · {:.0} × {:.0}", i.size.width, i.size.height))
                .unwrap_or_default(),
            format!(" · {}", file_size(artifact.byte_len))
        );
        let subtitle = if self.drive.saving {
            self.locale.text("saving_file").to_owned()
        } else if let Some(notice) = self.drive.notice {
            self.locale.text(notice).to_owned()
        } else {
            subtitle
        };
        let can_save =
            self.drive.bytes.is_some() && !self.drive.saving && !self.drive.choosing_save;
        let header = div()
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        ui::text_role(artifact.name.clone(), zork_ui::design::TextRole::PageTitle)
                            .truncate(),
                    )
                    .child(ui::text_role(subtitle, zork_ui::design::TextRole::Metadata).truncate()),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        preview_icon("drive-save", can_save)
                            .child(ui::icon("icons/download.svg", 16.))
                            .on_click(cx.listener(|v, _, _, cx| v.save_artifact_copy(cx)))
                            .automation_enabled(
                                can_save,
                                AutomationRole::Button,
                                self.locale.text("drive_save_copy"),
                            ),
                    )
                    .child(
                        preview_icon("preview-more", true)
                            .child(ui::icon("icons/settings-three.svg", 16.))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.drive.viewer.more = !v.drive.viewer.more;
                                zork_ui::components::region::invalidate_all(cx);
                            }))
                            .automation(AutomationRole::Button, self.locale.text("preview_more")),
                    )
                    .child(
                        preview_icon("drive-close-preview", true)
                            .child(ui::icon("icons/x.svg", 12.))
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.close_conversation_artifact();
                                zork_ui::components::region::invalidate_all(cx);
                            }))
                            .automation(AutomationRole::Button, self.locale.text("close")),
                    ),
            );
        let header = if image_view {
            div().w_full().h(px(32.)).flex().justify_end().child(
                preview_icon("drive-close-preview", true)
                    .child(ui::icon("icons/x.svg", 12.))
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.close_conversation_artifact();
                        zork_ui::components::region::invalidate_all(cx);
                    }))
                    .automation(AutomationRole::Button, self.locale.text("close")),
            )
        } else {
            header
        };
        let body = self.preview_body(&artifact, stage_height, cx).automation(
            AutomationRole::ScrollArea,
            self.locale.text("drive_preview"),
        );
        let footer = self.preview_footer(narrow, cx);
        let panel = div()
            .id("attachment-preview-dialog")
            .track_focus(&focus)
            .tab_group()
            .tab_stop(false)
            .relative()
            .w(px(width))
            .h(px(height))
            .p(px(gutter))
            .flex()
            .flex_col()
            .gap_4()
            .when(!image_view, |v| {
                v.rounded(px(ui::MODAL_RADIUS))
                    .bg(rgb(BG))
                    .shadow(vec![BoxShadow::new(
                        px(0.),
                        px(8.),
                        rgba(0x24272B1C).into(),
                    )
                    .blur_radius(px(20.))])
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |v, event: &gpui::MouseDownEvent, _, cx| {
                    if image_view {
                        let point = event.position - origin;
                        v.drive.viewer.more_position = Some(gpui::point(
                            point.x.clamp(px(8.), px(width - 248.)),
                            point.y.clamp(px(8.), px(height - 220.)),
                        ));
                        v.drive.viewer.more = true;
                        zork_ui::components::region::invalidate_all(cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(move |v, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if v.drive.viewer.more {
                        v.drive.viewer.more = false;
                    } else {
                        v.close_conversation_artifact();
                    }
                    zork_ui::components::region::invalidate_all(cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    zork_ui::modal::cycle_focus(
                        &key_focus,
                        event.keystroke.modifiers.shift,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                } else if event.keystroke.modifiers.platform && event.keystroke.key == "c" {
                    if let Some(source) = v.drive.viewer.selection.borrow_mut().finish() {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(source.quote));
                    }
                    cx.stop_propagation();
                } else if !event.keystroke.modifiers.modified()
                    && !v.drive.viewer.more
                    && v.drive.viewer.selection.borrow_mut().finish().is_none()
                {
                    match event.keystroke.key.as_str() {
                        "left" => {
                            v.move_preview(-1, cx);
                            cx.stop_propagation();
                        }
                        "right" => {
                            v.move_preview(1, cx);
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }
            }))
            .child(header)
            .child(body)
            .child(footer)
            .when(self.drive.viewer.more, |v| v.child(self.preview_menu(cx)));
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(if image_view {
                rgb(CUE_UI.palette.sidebar)
            } else {
                rgba(0x00000059)
            })
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.close_conversation_artifact();
                    zork_ui::components::region::invalidate_all(cx);
                }),
            )
            .child(panel)
    }

    fn preview_body(
        &self,
        artifact: &Artifact,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let viewer = &self.drive.viewer;
        let mut body = div()
            .id("drive-preview")
            .w_full()
            .h(px(height))
            .flex_shrink_0()
            .overflow_scroll()
            .track_scroll(&viewer.scroll);
        if self.drive.preview_failed {
            return body.child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(ui::text_role(
                        self.locale.text("drive_preview_failed"),
                        zork_ui::design::TextRole::Body,
                    ))
                    .child(
                        ui::button(
                            "drive-retry-preview",
                            self.locale.text("inbox_retry"),
                            false,
                            true,
                        )
                        .on_click(cx.listener(|v, _, _, cx| {
                            if let Some(a) = v.drive.selected.clone() {
                                v.load_artifact_preview(a, cx);
                            }
                        }))
                        .automation(AutomationRole::Button, self.locale.text("inbox_retry")),
                    ),
            );
        }
        if self.drive.bytes.is_none() {
            return body.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(loading::status(
                        "drive-preview-loading",
                        self.locale.text("loading_preview"),
                    )),
            );
        }
        if let Some(image) = viewer.image.as_ref().filter(|_| !viewer.source) {
            let scale = viewer.scale();
            let size = gpui::size(image.size.width * scale, image.size.height * scale);
            let content_width = viewer.view_size.width.max(size.width + 64.);
            let content_height = height.max(size.height + 64.);
            body = body
                .cursor(gpui::CursorStyle::OpenHand)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|v, event: &gpui::MouseDownEvent, _, cx| {
                        v.drive.viewer.more = false;
                        v.drive.viewer.drag =
                            Some((event.position, v.drive.viewer.scroll.offset()));
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_move(cx.listener(|v, event: &gpui::MouseMoveEvent, _, cx| {
                    if let Some((start, offset)) = v.drive.viewer.drag {
                        v.drive
                            .viewer
                            .scroll
                            .set_offset(offset + event.position - start);
                        zork_ui::components::region::invalidate_all(cx);
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, _| v.drive.viewer.drag = None),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, _| v.drive.viewer.drag = None),
                );
            return body
                .relative()
                .child(zork_ui::components::attachments::image_viewport(
                    image.rendered.clone(),
                    gpui::size(px(size.width), px(size.height)),
                    gpui::size(px(content_width), px(content_height)),
                ))
                .when(viewer.overview, |v| {
                    v.child(div().absolute().left_3().top_3().child(ui::text_role(
                        self.locale.text("preview_overview"),
                        zork_ui::design::TextRole::Metadata,
                    )))
                });
        }
        if viewer.image_failed && !viewer.source {
            return body.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(ui::text_role(
                        self.locale.text("preview_content_failed"),
                        zork_ui::design::TextRole::Body,
                    )),
            );
        }
        let document = if viewer.source {
            viewer.source_document.as_ref()
        } else {
            viewer.document.as_ref()
        };
        if let Some(document) = document {
            if viewer.text.as_ref().is_some_and(|text| text.is_empty()) {
                return body.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::text_role(
                            self.locale.text("preview_empty"),
                            zork_ui::design::TextRole::Body,
                        )),
                );
            }
            viewer.selection.borrow_mut().begin_frame();
            let root = cx.entity().downgrade();
            let selection = crate::components::selection::SelectionContext::new(
                "attachment-preview-selection".into(),
                crate::comments::CommentSource::default(),
                document.shared_plain_text(),
                viewer.selection.clone(),
                viewer.focus.as_ref().unwrap().clone(),
                Rc::new(move |cx| {
                    let _ =
                        root.update(cx, |_, cx| zork_ui::components::region::invalidate_all(cx));
                }),
            );
            body = body
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, window, cx| {
                        v.drive.viewer.selection.borrow_mut().clear();
                        v.drive.viewer.more = false;
                        if let Some(focus) = &v.drive.viewer.focus {
                            window.focus(focus, cx);
                        }
                        zork_ui::components::region::invalidate_all(cx);
                    }),
                )
                .on_mouse_move(cx.listener(|v, event: &gpui::MouseMoveEvent, _, cx| {
                    if v.drive.viewer.selection.borrow_mut().update(event.position) {
                        zork_ui::components::region::invalidate_all(cx);
                    }
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, _| {
                        v.drive.viewer.selection.borrow_mut().finish();
                    }),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, _| {
                        v.drive.viewer.selection.borrow_mut().finish();
                    }),
                );
            return body.child(
                div()
                    .p_6()
                    .text_size(px(13.))
                    .line_height(px(21.))
                    .when(viewer.source, |v| v.font_family("JetBrains Mono"))
                    .child(render_selectable_document(
                        "attachment-content",
                        document,
                        &selection,
                    ))
                    .when(viewer.text_truncated, |v| {
                        v.child(ui::text_role(
                            self.locale.text("preview_truncated"),
                            zork_ui::design::TextRole::Metadata,
                        ))
                    }),
            );
        }
        body.child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .flex_col()
                .gap_3()
                .p_6()
                .child(ui::icon("icons/file.svg", 32.))
                .child(ui::text_role(
                    artifact.name.clone(),
                    zork_ui::design::TextRole::Label,
                ))
                .child(ui::text_role(
                    self.locale.text("drive_download_to_view"),
                    zork_ui::design::TextRole::Body,
                ))
                .child(
                    preview_quiet(
                        "preview-fallback-save",
                        self.locale.text("drive_save_copy"),
                        !self.drive.saving && !self.drive.choosing_save,
                        ui::IconButtonSize::Standard,
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.save_artifact_copy(cx)))
                    .automation(AutomationRole::Button, self.locale.text("drive_save_copy")),
                ),
        )
    }

    fn preview_footer(&self, narrow: bool, cx: &mut Context<Self>) -> Div {
        let p = &self.drive.viewer;
        let previous = p.index > 0;
        let next = p.index + 1 < p.group.len();
        let navigation = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                preview_icon("preview-previous", previous)
                    .child(ui::icon("icons/arrow-left.svg", 16.))
                    .on_click(cx.listener(|v, _, _, cx| v.move_preview(-1, cx)))
                    .automation_enabled(
                        previous,
                        AutomationRole::Button,
                        self.locale.text("preview_previous"),
                    ),
            )
            .child(
                ui::text_role(
                    format!("{} / {}", p.index + 1, p.group.len().max(1)),
                    zork_ui::design::TextRole::Metadata,
                )
                .w(px(40.))
                .text_center(),
            )
            .child(
                preview_icon("preview-next", next)
                    .child(ui::icon("icons/arrow-right.svg", 16.))
                    .on_click(cx.listener(|v, _, _, cx| v.move_preview(1, cx)))
                    .automation_enabled(
                        next,
                        AutomationRole::Button,
                        self.locale.text("preview_next"),
                    ),
            );
        let mut footer = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .when(narrow, |v| v.flex_wrap())
            .child(navigation);
        if p.image.is_some() && !p.source {
            let scale = p.scale();
            let tools = div()
                .flex()
                .items_center()
                .px_1()
                .child(
                    preview_quiet(
                        "preview-zoom-out",
                        "−",
                        scale > 0.1,
                        ui::IconButtonSize::Standard,
                    )
                    .on_click(cx.listener(|v, _, _, cx| {
                        v.zoom_preview(Some(v.drive.viewer.scale() / 1.25), cx)
                    }))
                    .automation(AutomationRole::Button, self.locale.text("preview_zoom_out")),
                )
                .child(
                    preview_quiet(
                        "preview-actual",
                        format!("{:.0}%", scale * 100.),
                        true,
                        ui::IconButtonSize::Standard,
                    )
                    .w(px(56.))
                    .on_click(cx.listener(|v, _, _, cx| v.zoom_preview(Some(1.), cx)))
                    .automation(AutomationRole::Button, self.locale.text("preview_actual")),
                )
                .child(
                    preview_icon("preview-zoom-in", scale < 20.)
                        .child(ui::icon("icons/plus.svg", 14.))
                        .on_click(cx.listener(|v, _, _, cx| {
                            v.zoom_preview(Some(v.drive.viewer.scale() * 1.25), cx)
                        }))
                        .automation(AutomationRole::Button, self.locale.text("preview_zoom_in")),
                )
                .child(
                    preview_quiet(
                        "preview-fit",
                        self.locale.text("preview_fit"),
                        true,
                        ui::IconButtonSize::Standard,
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.zoom_preview(None, cx)))
                    .automation(AutomationRole::Button, self.locale.text("preview_fit")),
                );
            footer = footer.child(tools);
        } else {
            footer = footer.child(ui::text_role(
                self.locale.text("preview_read_only"),
                zork_ui::design::TextRole::Metadata,
            ));
        }
        footer
    }

    fn preview_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut menu = div()
            .absolute()
            .when_some(self.drive.viewer.more_position, |v, position| {
                v.left(position.x).top(position.y)
            })
            .when(self.drive.viewer.more_position.is_none(), |v| {
                v.right(px(24.)).top(px(64.))
            })
            .w(px(240.))
            .p_2()
            .rounded(px(ui::MENU_RADIUS))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BG))
            .shadow(vec![BoxShadow::new(
                px(0.),
                px(8.),
                rgba(0x24272b14).into(),
            )
            .blur_radius(px(24.))])
            .flex()
            .flex_col()
            .gap_1();
        let can_save =
            self.drive.bytes.is_some() && !self.drive.saving && !self.drive.choosing_save;
        menu = menu.child(
            preview_quiet(
                "preview-save-copy",
                self.locale.text("drive_save_copy"),
                can_save,
                ui::IconButtonSize::Standard,
            )
            .w_full()
            .justify_start()
            .on_click(cx.listener(|v, _, _, cx| v.save_artifact_copy(cx)))
            .automation_enabled(
                can_save,
                AutomationRole::Button,
                self.locale.text("drive_save_copy"),
            ),
        );
        if self.drive.viewer.text.is_some() {
            let text = self.locale.text(if self.drive.viewer.source {
                "preview_rendered"
            } else {
                "preview_source"
            });
            menu = menu.child(
                preview_quiet("preview-source", text, true, ui::IconButtonSize::Standard)
                    .w_full()
                    .justify_start()
                    .on_click(cx.listener(|v, _, window, cx| {
                        v.drive.viewer.source = !v.drive.viewer.source;
                        v.drive.viewer.selection.borrow_mut().clear();
                        v.drive.viewer.more = false;
                        if let Some(focus) = &v.drive.viewer.focus {
                            window.focus(focus, cx);
                        }
                        v.drive
                            .viewer
                            .scroll
                            .set_offset(gpui::point(px(0.), px(0.)));
                        zork_ui::components::region::invalidate_all(cx);
                    }))
                    .automation(AutomationRole::Button, text),
            );
        }
        menu = menu.child(
            preview_quiet(
                "preview-info",
                self.locale.text("preview_info"),
                true,
                ui::IconButtonSize::Standard,
            )
            .w_full()
            .justify_start()
            .on_click(cx.listener(|v, _, _, cx| {
                v.drive.viewer.info = !v.drive.viewer.info;
                zork_ui::components::region::invalidate_all(cx);
            }))
            .automation(AutomationRole::Button, self.locale.text("preview_info")),
        );
        if self.drive.viewer.info {
            if let Some(artifact) = &self.drive.selected {
                menu = menu.child(
                    div()
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(ui::text_role(
                            artifact.name.clone(),
                            zork_ui::design::TextRole::Label,
                        ))
                        .when(
                            !artifact.source_path.starts_with("file-")
                                && !artifact.source_path.is_empty(),
                            |v| {
                                v.child(ui::text_role(
                                    artifact.source_path.clone(),
                                    zork_ui::design::TextRole::Metadata,
                                ))
                            },
                        )
                        .when(artifact.version > 1, |v| {
                            v.child(ui::text_role(
                                format!("v{}", artifact.version),
                                zork_ui::design::TextRole::Metadata,
                            ))
                        })
                        .when(!artifact.created_at.is_empty(), |v| {
                            v.child(ui::text_role(
                                artifact.created_at.clone(),
                                zork_ui::design::TextRole::Metadata,
                            ))
                        }),
                );
            }
        }
        if self.can_send_selected()
            && self.drive.bytes.is_some()
            && self
                .drive
                .selected
                .as_ref()
                .is_some_and(|a| a.session_id.as_ref() == self.selected_session.as_ref())
        {
            menu = menu.child(
                preview_quiet(
                    "drive-reuse",
                    self.locale.text("reuse_attachment"),
                    true,
                    ui::IconButtonSize::Standard,
                )
                .w_full()
                .justify_start()
                .on_click(cx.listener(|v, _, window, cx| {
                    if let (Some(file), Some(bytes), Some(session)) =
                        (&v.drive.selected, &v.drive.bytes, &v.selected_session)
                    {
                        let reference = zork_client_core::files::FileRef {
                            id: file.artifact_id.clone(),
                            name: file.name.clone(),
                            byte_len: bytes.len(),
                            content_root: zork_client_core::api::content_root(bytes),
                        };
                        if let Err(error) = v.core_device.reuse_file(session, reference, bytes) {
                            v.error = Some(error.to_string());
                        } else {
                            v.drive.notice = Some("preview_added_to_draft");
                        }
                    }
                    v.drive.viewer.more = false;
                    if let Some(focus) = &v.drive.viewer.focus {
                        window.focus(focus, cx);
                    }
                    zork_ui::components::region::invalidate_all(cx);
                }))
                .automation(AutomationRole::Button, self.locale.text("reuse_attachment")),
            );
        }
        menu
    }
}
