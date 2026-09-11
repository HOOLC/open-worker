use super::motion::{DraftFiles, Frame};
use super::*;
use std::cell::RefCell;
use zork_client_core::files::FileRef;
use zork_ui::components::attachment_fan::{self as geometry, Opening};

#[derive(Clone, Copy, Default)]
pub(in crate::views) struct FanState {
    pub hovered: bool,
    pub pinned: bool,
    pub active: Option<usize>,
    remove_active: Option<usize>,
    pub progress: f32,
    row: Option<usize>,
    from: f32,
    began: Option<std::time::Instant>,
    leave_deadline: Option<std::time::Instant>,
}
impl FanState {
    fn advance(&mut self, now: std::time::Instant, reduced: bool) -> bool {
        if self.leave_deadline.is_some_and(|deadline| now >= deadline) {
            self.leave_deadline = None;
            self.from = self.progress;
            self.began = Some(now);
        }
        let target = self.open() as u8 as f32;
        if reduced {
            self.progress = target;
            self.began = None;
            return false;
        }
        if let Some(began) = self.began {
            let duration =
                ((if target > 0. { 0.28 } else { 0.22 }) * (target - self.from).abs()).max(0.08);
            let t = (now.duration_since(began).as_secs_f32() / duration).min(1.);
            self.progress = self.from + (target - self.from) * (1. - (1. - t).powi(3));
            if t == 1. {
                self.began = None;
            }
        }
        self.began.is_some() || self.leave_deadline.is_some()
    }
    fn transition(&mut self, now: std::time::Instant, reduced: bool) {
        self.from = self.progress;
        self.began = Some(now);
        self.advance(now, reduced);
    }
    fn set_hover(&mut self, hover: bool, now: std::time::Instant, reduced: bool) {
        self.hovered = hover;
        self.leave_deadline = if !hover && !reduced {
            Some(now + std::time::Duration::from_millis(120))
        } else {
            None
        };
    }
    pub fn open(self) -> bool {
        self.hovered || self.pinned || self.leave_deadline.is_some()
    }
}

struct PreviewEntry {
    page: Option<Arc<::image::RgbaImage>>,
    images: HashMap<usize, Arc<gpui::RenderImage>>,
    rotating: bool,
    ready: bool,
    used: u64,
}
pub(in crate::views) struct PreviewCache {
    placeholder: super::preview::Images,
    entries: HashMap<String, PreviewEntry>,
    clock: u64,
    loading: usize,
    retired: Vec<Arc<gpui::RenderImage>>,
}
impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            placeholder: super::preview::placeholder(),
            entries: HashMap::new(),
            clock: 0,
            loading: 0,
            retired: Vec::new(),
        }
    }
}

fn key(file: &FileRef) -> String {
    format!("{}:{}", file.id, file.content_root)
}
impl PreviewCache {
    fn begin(&mut self, file: &FileRef) -> bool {
        if self.loading >= 2 || self.entries.contains_key(&key(file)) {
            return false;
        }
        if self.entries.len() >= 24 {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, v)| v.ready)
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                if let Some(entry) = self.entries.remove(&oldest) {
                    self.retired.extend(entry.images.into_values());
                }
            }
        }
        self.clock += 1;
        self.loading += 1;
        self.entries.insert(
            key(file),
            PreviewEntry {
                page: None,
                images: HashMap::new(),
                rotating: false,
                ready: false,
                used: self.clock,
            },
        );
        true
    }
    fn image(
        &mut self,
        file: &FileRef,
        angle: usize,
    ) -> (Arc<gpui::RenderImage>, Arc<gpui::RenderImage>) {
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&key(file)) {
            entry.used = self.clock;
            if let Some((index, image)) = entry
                .images
                .iter()
                .min_by_key(|(index, _)| index.abs_diff(angle))
            {
                return (image.clone(), self.placeholder.0[*index].1.clone());
            }
        }
        self.placeholder.0[angle].clone()
    }
    #[cfg(feature = "headless-bench")]
    pub(in crate::views) fn image_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| !e.images.is_empty())
            .count()
    }
}

#[derive(Default)]
pub(in crate::views) struct UiState {
    pub draft: FanState,
    pub draft_files: DraftFiles,
    pub messages: Rc<RefCell<HashMap<String, FanState>>>,
    pub previews: Rc<RefCell<PreviewCache>>,
    pub message_previews: Rc<RefCell<super::message::PreviewCache>>,
}

impl RootView {
    pub(in crate::views) fn advance_file_fans(&mut self, window: &Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let mut moving = self.file_ui.draft.advance(now, cx.reduce_motion());
        moving |= self.file_ui.draft_files.advance(
            &self.draft_state.files,
            &mut self.file_ui.draft,
            now,
            cx.reduce_motion(),
            (self.composer_surface_width - 72.).max(168.),
        );
        let mut remeasure = Vec::new();
        for state in self.file_ui.messages.borrow_mut().values_mut() {
            let previous = state.progress;
            moving |= state.advance(now, cx.reduce_motion());
            if state.progress != previous {
                if let Some(row) = state.row {
                    remeasure.push(row);
                }
            }
        }
        if !remeasure.is_empty() {
            let anchor = self.transcript_list.logical_scroll_top();
            for row in remeasure {
                if row < self.lines.len() {
                    self.transcript_list.splice(row..row + 1, 1);
                }
            }
            self.transcript_list.scroll_to(anchor);
        }
        if moving {
            let root = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = root.update(cx, |_, cx| {
                    zork_ui::components::region::invalidate(cx, &["composer", "transcript"])
                });
            });
        }
    }
    pub(in crate::views) fn draft_file_frame(&self) -> Frame {
        self.file_ui.draft_files.frame(
            self.file_ui.draft.progress,
            (self.composer_surface_width - 72.).max(168.),
        )
    }
    pub(in crate::views) fn file_fan_center(&self) -> f32 {
        let frame = self.draft_file_frame();
        let half = (frame.width * 0.5).max(72. * frame.shape.width);
        (self.composer_surface_width - half - 24.).max(half + 24.)
    }
    pub(in crate::views) fn file_fan_dimensions(&self) -> (f32, f32) {
        let frame = self.draft_file_frame();
        (frame.width, frame.height)
    }
    pub(in crate::views) fn draft_opening(&self) -> Option<Opening> {
        let frame = self.draft_file_frame();
        (!frame.files.is_empty()).then(|| {
            Opening::new(frame.shape, frame.expanded)
                .translated(gpui::point(self.file_fan_center(), 0.))
        })
    }
    pub(in crate::views) fn ensure_file_previews(
        &mut self,
        files: &[(FileRef, usize)],
        cx: &mut Context<Self>,
    ) {
        let retired = std::mem::take(&mut self.file_ui.previews.borrow_mut().retired);
        if !retired.is_empty() {
            cx.defer(move |cx| {
                for image in retired {
                    cx.drop_image(image, None);
                }
            });
        }
        for (file, angle) in files {
            let angle = *angle;
            let rotation = {
                let mut cache = self.file_ui.previews.borrow_mut();
                cache.entries.get_mut(&key(file)).and_then(|entry| {
                    if entry.rotating || entry.images.contains_key(&angle) {
                        return None;
                    }
                    let page = entry.page.clone()?;
                    entry.rotating = true;
                    Some(page)
                })
            };
            if let Some(page) = rotation {
                let file = file.clone();
                let cache = self.file_ui.previews.clone();
                cx.spawn(async move |this, cx| {
                    let image = cx
                        .background_executor()
                        .spawn(async move { super::preview::render(&page, angle) })
                        .await;
                    if let Some(entry) = cache.borrow_mut().entries.get_mut(&key(&file)) {
                        entry.images.insert(angle, image);
                        entry.rotating = false;
                    }
                    let _ = this.update(cx, |_, cx| {
                        zork_ui::components::region::invalidate(cx, &["composer"]);
                    });
                })
                .detach();
                continue;
            }
            if !self.file_ui.previews.borrow_mut().begin(file) {
                continue;
            }
            let file = file.clone();
            let device = self.core_device.clone();
            let cache = self.file_ui.previews.clone();
            #[cfg(feature = "headless-bench")]
            let offline = self.benchmark_offline;
            #[cfg(not(feature = "headless-bench"))]
            let offline = false;
            let local = self.local_cache.clone();
            let renderer = cx.svg_renderer();
            cx.spawn(async move |this, cx| {
                let bytes = if offline {
                    local.and_then(|(store, node)| {
                        store
                            .blob(&node, &format!("upload:{}", file.id))
                            .ok()
                            .flatten()
                    })
                } else {
                    device.artifact_content(&file.id).await.ok()
                };
                let prepared = if let Some(bytes) = bytes {
                    let name = file.name.clone();
                    cx.background_executor()
                        .spawn(async move {
                            let page = Arc::new(super::preview::load(&name, &bytes, &renderer)?);
                            let started = std::time::Instant::now();
                            let image = super::preview::render(&page, angle);
                            #[cfg(feature = "headless-bench")]
                            eprintln!(
                                "preview first angle {:.2} ms",
                                started.elapsed().as_secs_f64() * 1000.
                            );
                            #[cfg(not(feature = "headless-bench"))]
                            let _ = started;
                            Some((page, image))
                        })
                        .await
                } else {
                    None
                };
                let mut cache = cache.borrow_mut();
                cache.loading = cache.loading.saturating_sub(1);
                if let Some(entry) = cache.entries.get_mut(&key(&file)) {
                    if let Some((page, image)) = prepared {
                        entry.page = Some(page);
                        entry.images.insert(angle, image);
                    }
                    entry.ready = true;
                }
                drop(cache);
                let _ = this.update(cx, |_, cx| {
                    zork_ui::components::region::invalidate(cx, &["composer", "transcript"])
                });
            })
            .detach();
        }
    }
    pub(in crate::views) fn release_file_previews(&mut self, cx: &mut gpui::App) {
        self.file_ui.message_previews.borrow_mut().release(cx);
        let mut cache = self.file_ui.previews.borrow_mut();
        for (_, entry) in cache.entries.drain() {
            for image in entry.images.into_values() {
                cx.drop_image(image, None);
            }
        }
        for (paper, shadow) in &cache.placeholder.0 {
            cx.drop_image(paper.clone(), None);
            cx.drop_image(shadow.clone(), None);
        }
        for image in cache.retired.drain(..) {
            cx.drop_image(image, None);
        }
    }
    pub(in crate::views) fn render_draft_fan(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let frame = self.draft_file_frame();
        self.ensure_file_previews(
            &frame
                .files
                .iter()
                .map(|v| {
                    (
                        v.file.clone(),
                        ((v.pose.angle + 12.) * 4.).round().clamp(0., 96.) as usize,
                    )
                })
                .collect::<Vec<_>>(),
            cx,
        );
        let width = frame.width;
        let below =
            geometry::clip_below(&Opening::new(frame.shape, frame.expanded), frame.expanded);
        render(
            frame,
            self.file_ui.draft,
            self.file_ui.previews.clone(),
            cx.entity().downgrade(),
            None,
            true,
            self.selected_session.clone().unwrap_or_default(),
            self.locale,
        )
        .absolute()
        .right(px(self.composer_surface_width
            - self.file_fan_center()
            - width * 0.5))
        .bottom(px(
            zork_ui::components::liquid_composer::TOP_EXTENSION - below
        ))
    }
    pub(in crate::views) fn close_draft_fan(&mut self, cx: &mut Context<Self>) {
        self.file_ui.draft.hovered = false;
        self.file_ui.draft.leave_deadline = None;
        self.file_ui.draft.pinned = false;
        self.file_ui
            .draft
            .transition(cx.background_executor().now(), cx.reduce_motion());
        zork_ui::components::region::invalidate(cx, &["composer", "transcript"]);
    }
    fn highlight_file(
        &mut self,
        message: Option<(String, usize)>,
        index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some((key, _)) = message {
            let mut states = self.file_ui.messages.borrow_mut();
            let state = states.entry(key).or_default();
            if hovered {
                state.active = Some(index);
            } else if state.active == Some(index) {
                state.active = None;
            }
            drop(states);
            zork_ui::components::region::invalidate(cx, &["transcript"]);
        } else {
            if hovered {
                self.file_ui.draft.active = Some(index);
            } else if self.file_ui.draft.active == Some(index) {
                self.file_ui.draft.active = None;
            }
            zork_ui::components::region::invalidate(cx, &["composer"]);
        }
    }
    fn change_fan(
        &mut self,
        message: Option<(String, usize)>,
        hover: Option<bool>,
        toggle: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some((key, index)) = message {
            let mut states = self.file_ui.messages.borrow_mut();
            let state = states.entry(key).or_default();
            state.row = Some(index);
            let old = state.open();
            if let Some(hover) = hover {
                state.set_hover(hover, cx.background_executor().now(), cx.reduce_motion());
            }
            if toggle {
                state.pinned = !state.pinned;
            }
            if old != state.open() {
                state.transition(cx.background_executor().now(), cx.reduce_motion());
            }
            if old != state.open() && index < self.lines.len() {
                let anchor = self.transcript_list.logical_scroll_top();
                self.transcript_list.splice(index..index + 1, 1);
                self.transcript_list.scroll_to(anchor);
            }
            drop(states);
            zork_ui::components::region::invalidate(cx, &["transcript"]);
        } else {
            let old = self.file_ui.draft.open();
            if let Some(hover) = hover {
                self.file_ui.draft.set_hover(
                    hover,
                    cx.background_executor().now(),
                    cx.reduce_motion(),
                );
            }
            if toggle {
                self.file_ui.draft.pinned = !self.file_ui.draft.pinned;
            }
            if old != self.file_ui.draft.open() {
                self.file_ui
                    .draft
                    .transition(cx.background_executor().now(), cx.reduce_motion());
            }
            zork_ui::components::region::invalidate(cx, &["composer", "transcript"]);
        }
    }
}

fn render(
    frame: Frame,
    state: FanState,
    cache: Rc<RefCell<PreviewCache>>,
    root: gpui::WeakEntity<RootView>,
    message: Option<(String, usize)>,
    draft: bool,
    session: String,
    locale: Locale,
) -> gpui::Stateful<Div> {
    #[cfg(feature = "headless-bench")]
    let opacity = if std::env::var_os("ZORK_FILES_CONTOUR_ONLY").is_some() {
        0.
    } else {
        1.
    };
    #[cfg(not(feature = "headless-bench"))]
    let opacity = 1.;
    let open = state.open();
    let width = frame.width;
    let height = frame.height;
    let opening = Opening::new(frame.shape, frame.expanded);
    let release = 0.;
    let below = geometry::clip_below(&opening, frame.expanded);
    let hover_root = root.clone();
    let hover_message = message.clone();
    let pin_root = root.clone();
    let pin_message = message.clone();
    let mut order = (0..frame.files.len()).collect::<Vec<_>>();
    order.sort_by_key(|i| (frame.expanded > 0.98 && state.active == Some(*i), *i));
    let toggle_id = if draft {
        "draft-file-fan-toggle".to_owned()
    } else {
        format!("file-fan-toggle-{}", message.as_ref().unwrap().0)
    };
    div()
        .id(if draft {
            "draft-file-fan".into()
        } else {
            format!("file-fan-{}", message.as_ref().unwrap().0)
        })
        .relative()
        .w(px(width))
        .h(px(height + below))
        .overflow_hidden()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_hover(move |hover, _, cx| {
            let _ = hover_root.update(cx, |v, cx| {
                v.change_fan(hover_message.clone(), Some(*hover), false, cx)
            });
        })
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            let _ = pin_root.update(cx, |v, cx| {
                v.change_fan(pin_message.clone(), None, true, cx);
                v.focus_composer(window, cx);
            });
        })
        .child(
            div()
                .id(toggle_id)
                .absolute()
                .top_0()
                .w_full()
                .h(px(12.))
                .automation(AutomationRole::Button, locale.text("conversation_files")),
        )
        .children(
            order
                .into_iter()
                .filter(|i| frame.files[*i].visible)
                .map(|i| {
                    let visual = &frame.files[i];
                    let pose = visual.pose;
                    let angle = ((pose.angle + 12.) * 4.).round().clamp(0., 96.) as usize;
                    let (image, shadow) = cache.borrow_mut().image(&visual.file, angle);
                    let file = visual.file.clone();
                    let root = root.clone();
                    let pin_message = message.clone();
                    let label = file.name.clone();
                    let session = session.clone();
                    let active_root = root.clone();
                    let active_message = message.clone();
                    let extent = pose.half_extent();
                    let image_width = pose.width / (216. / 384.);
                    let image_height = pose.height / (304. / 384.);
                    div()
                        .absolute()
                        .inset_0()
                        .child(
                            clipped_image(
                                shadow,
                                pose,
                                opening,
                                release,
                                height,
                                image_width,
                                image_height,
                            )
                            .absolute()
                            .size_full()
                            .opacity(opacity),
                        )
                        .child(
                            clipped_image(
                                image,
                                pose,
                                opening,
                                release,
                                height,
                                image_width,
                                image_height,
                            )
                            .absolute()
                            .size_full()
                            .opacity(opacity),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(width * 0.5 + pose.center.x - extent.x))
                                .top(px(height + pose.center.y - extent.y))
                                .w(px(extent.x * 2.))
                                .h(px(extent.y * 2.))
                                .id(if draft {
                                    format!("draft-preview-{}", file.id)
                                } else {
                                    format!(
                                        "message-file-{}-{}",
                                        message.as_ref().unwrap().1,
                                        file.id
                                    )
                                })
                                .cursor_pointer()
                                .when(
                                    draft && open && state.progress > 0.98 && !visual.departing,
                                    |paper| {
                                        paper.child(remove_button(
                                            file.clone(),
                                            root.clone(),
                                            session.clone(),
                                            locale,
                                            i,
                                            state.active == Some(i)
                                                || state.remove_active == Some(i),
                                        ))
                                    },
                                )
                                .on_hover(move |hovered, _, cx| {
                                    let _ = active_root.update(cx, |v, cx| {
                                        v.highlight_file(active_message.clone(), i, *hovered, cx)
                                    });
                                })
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = root.update(cx, |v, cx| {
                                        if v.file_ui.draft_files.changing() && draft {
                                            return;
                                        }
                                        if !open || (draft && !state.pinned) {
                                            v.change_fan(pin_message.clone(), None, true, cx);
                                        } else {
                                            v.open_message_file(&file, &session, cx);
                                        }
                                    });
                                })
                                .automation(AutomationRole::Button, label),
                        )
                }),
        )
        .when(draft, |fan| {
            fan.child(
                gpui::canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        use zork_ui::components::liquid_composer::{
                            BORDER_COLOR, SLOT_BORDER_WIDTH,
                        };
                        let rim = opening.translated(gpui::point(
                            bounds.center().x.as_f32(),
                            bounds.top().as_f32() + height,
                        ));
                        let lower = &rim.hole[2..6];
                        let point = |p: gpui::Point<f32>| gpui::point(px(p.x), px(p.y));
                        let mut path = gpui::PathBuilder::stroke(px(SLOT_BORDER_WIDTH));
                        path.move_to(point(lower[0][0]));
                        for curve in lower {
                            path.cubic_bezier_to(point(curve[3]), point(curve[1]), point(curve[2]));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(BORDER_COLOR));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
        })
}

fn remove_button(
    file: FileRef,
    root: gpui::WeakEntity<RootView>,
    session: String,
    locale: Locale,
    index: usize,
    visible: bool,
) -> impl IntoElement {
    let hover_root = root.clone();
    let hover_id = format!("file-remove-fill-{}", file.id);
    div()
        .id(format!("remove-{}", file.id))
        .absolute()
        .right(px(-geometry::REMOVE_SIZE * 0.5))
        .top(px(-geometry::REMOVE_SIZE * 0.5))
        .size(px(geometry::REMOVE_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .opacity(if visible { 1. } else { 0. })
        .child(
            div()
                .relative()
                .size(px(16.))
                .rounded_full()
                .bg(rgba(0xfffffff2))
                .child(zork_ui::components::motion::HoverFill {
                    id: hover_id.into(),
                    color: zork_ui::design::INTERACTION.neutral_hover,
                    radius: 8.,
                    pressed: None,
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/x.svg")
                        .size(px(geometry::REMOVE_GLYPH))
                        .text_color(rgb(0x757b82)),
                ),
        )
        .on_hover(move |hovered, _, cx| {
            let _ = hover_root.update(cx, |v, cx| {
                if *hovered {
                    v.file_ui.draft.remove_active = Some(index);
                } else if v.file_ui.draft.remove_active == Some(index) {
                    v.file_ui.draft.remove_active = None;
                }
                zork_ui::components::region::invalidate(cx, &["composer"]);
            });
        })
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = root.update(cx, |v, cx| {
                if v.selected_session.as_deref() == Some(&session) {
                    if let Err(error) = v.core_device.remove_file(&session, &file.id) {
                        v.error = Some(error.to_string());
                    }
                }
                zork_ui::components::region::invalidate(cx, &["composer"]);
            });
        })
        .automation(AutomationRole::Button, locale.text("remove_attachment"))
}

// Clip only image pixels. Painting an opaque front patch here would erase
// the lower half of the aperture border even where there is no paper.
fn clipped_image(
    image: Arc<gpui::RenderImage>,
    pose: geometry::Pose,
    opening: Opening,
    release: f32,
    fan_height: f32,
    image_width: f32,
    image_height: f32,
) -> impl IntoElement + gpui::Styled {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            // Derive both the image and clipping rim from one stable layout
            // origin. Nested rounded layout bounds jump at the final upright pose.
            let origin = gpui::point(
                bounds.center().x.as_f32(),
                bounds.top().as_f32() + fan_height,
            );
            let bounds = gpui::Bounds::new(
                gpui::point(
                    px(origin.x + pose.center.x - image_width * 0.5),
                    px(origin.y + pose.center.y - image_height * 0.5),
                ),
                gpui::size(px(image_width), px(image_height)),
            );
            let origin = gpui::point(origin.x, origin.y + release);
            let opening = opening.translated(origin);
            let lower = &opening.hole[2..6];
            let edge = |x: f32| {
                if x >= lower[0][0].x {
                    return lower[0][0].y;
                }
                if x <= lower[3][3].x {
                    return lower[3][3].y;
                }
                for curve in lower {
                    if x <= curve[0].x && x >= curve[3].x {
                        let (mut lo, mut hi) = (0., 1.);
                        for _ in 0..12 {
                            let mid = (lo + hi) * 0.5;
                            if geometry::sample(*curve, mid).x > x {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        return geometry::sample(*curve, (lo + hi) * 0.5).y;
                    }
                }
                lower[0][0].y
            };
            let scale = window.scale_factor();
            let min_y = lower.iter().flatten().map(|p| p.y).fold(f32::MAX, f32::min);
            let max_y = lower.iter().flatten().map(|p| p.y).fold(f32::MIN, f32::max);
            if bounds.top().as_f32() >= max_y {
                return;
            }
            let paint = |window: &mut Window| {
                // The rotated thumbnail has transparent padding. Exclude its
                // outer texel so linear atlas sampling cannot pick up a
                // neighbouring tile along the quad's edge.
                window.with_content_mask(
                    Some(gpui::ContentMask {
                        bounds: bounds.inset(px(1.)),
                    }),
                    |window| {
                        let _ = window.paint_image(
                            bounds,
                            bounds,
                            gpui::Corners::all(px(0.)),
                            image.clone(),
                            0,
                            false,
                        );
                    },
                );
            };
            if bounds.bottom().as_f32() <= min_y {
                paint(window);
                return;
            }
            let top = (min_y * scale).floor() / scale;
            if top > bounds.top().as_f32() {
                window.with_content_mask(
                    Some(gpui::ContentMask {
                        bounds: gpui::Bounds::new(
                            bounds.origin,
                            gpui::size(bounds.size.width, px(top) - bounds.top()),
                        ),
                    }),
                    |window| paint(window),
                );
            }
            let band_top = top.max(bounds.top().as_f32());
            let first = (bounds.left().as_f32() * scale).floor() as i32;
            let last = (bounds.right().as_f32() * scale).ceil() as i32;
            let cutoff = |column: i32| {
                (edge((column as f32 + 0.5) / scale).min(bounds.bottom().as_f32()) * scale * 8.)
                    .round()
                    / (scale * 8.)
            };
            let mut start = first;
            let mut y = cutoff(first);
            for column in first + 1..=last {
                let next = if column == last {
                    f32::NAN
                } else {
                    cutoff(column)
                };
                if next != y {
                    if y > band_top {
                        window.with_content_mask(
                            Some(gpui::ContentMask {
                                bounds: gpui::Bounds::new(
                                    gpui::point(px(start as f32 / scale), px(band_top)),
                                    gpui::size(
                                        px((column - start) as f32 / scale),
                                        px(y - band_top),
                                    ),
                                ),
                            }),
                            |window| paint(window),
                        );
                    }
                    start = column;
                    y = next;
                }
            }
        },
    )
}
