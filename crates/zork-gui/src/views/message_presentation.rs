//! Per-conversation presentation: one-shot arrivals, interruptible tail motion,
//! and full messages hosted in a centered reading dialog.
use super::*;
use std::{cell::RefCell, time::Instant};

#[derive(Default)]
pub(super) struct MessageMotion {
    pub arrivals: HashMap<String, Instant>,
    pub unread: usize,
    pub scroll: Option<Task<()>>,
}

pub(super) struct MessageReader {
    pub source: crate::comments::CommentSource,
    pub text: String,
    pub sections: Rc<Vec<crate::components::message::MessageDocument>>,
    pub plain: gpui::SharedString,
    pub offsets: Rc<Vec<usize>>,
    pub scroll: ListState,
    pub selection: Rc<RefCell<crate::components::selection::TranscriptSelection>>,
}

impl RootView {
    pub(super) fn track_message_arrivals(
        &mut self,
        arrivals: &zork_client_core::state::MessageArrivals,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        self.message_motion
            .arrivals
            .retain(|_, time| now.duration_since(*time) < Duration::from_millis(250));
        if arrivals.count == 0 {
            return;
        }
        for id in &arrivals.ids {
            self.message_motion.arrivals.insert(id.clone(), now);
        }
        if self.transcript_list.is_following_tail() || self.message_motion.scroll.is_some() {
            if !cx.reduce_motion() {
                self.animate_message_tail(cx);
            }
        } else {
            self.message_motion.unread = self
                .message_motion
                .unread
                .saturating_add(usize::try_from(arrivals.count).unwrap_or(usize::MAX));
        }
    }

    pub(super) fn animate_message_tail(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.message_motion.unread) > 0 {
            zork_ui::components::region::invalidate(cx, &["composer"]);
        }
        if cx.reduce_motion() {
            self.transcript_list.set_follow_mode(FollowMode::Tail);
            self.transcript_list.scroll_to_end();
            zork_ui::components::region::invalidate(cx, &["transcript"]);
            return;
        }
        if self.message_motion.scroll.is_some() {
            return;
        }
        let start = -self
            .transcript_list
            .scroll_px_offset_for_scrollbar()
            .y
            .as_f32();
        self.transcript_list.set_follow_mode(FollowMode::Normal);
        self.message_motion.scroll = Some(cx.spawn(async move |this, cx| {
            let began = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let done = this
                    .update(cx, |v, cx| {
                        let t = (began.elapsed().as_secs_f32() / 0.20).min(1.);
                        // GPUI's scrollbar extent excludes content padding; the
                        // transcript reserves its floating composer at the tail.
                        let target = v.transcript_list.max_offset_for_scrollbar().y.as_f32()
                            + if v.can_send_selected() {
                                v.composer_overlay_height
                            } else {
                                0.
                            };
                        let current = -v
                            .transcript_list
                            .scroll_px_offset_for_scrollbar()
                            .y
                            .as_f32();
                        let next = start + (target - start).max(0.) * (1. - (1. - t).powi(3));
                        v.transcript_list.scroll_by(px((next - current).max(0.)));
                        if t >= 1. {
                            v.transcript_list.set_follow_mode(FollowMode::Tail);
                            v.transcript_list.scroll_to_end();
                            v.message_motion.scroll = None;
                        }
                        zork_ui::components::region::invalidate(cx, &["transcript"]);
                        t >= 1.
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        }));
    }

    pub(super) fn interrupt_message_scroll(&mut self, cx: &mut Context<Self>) {
        let interrupted = self.message_motion.scroll.take().is_some();
        let root = cx.entity().downgrade();
        // GPUI invokes the wheel callback while borrowing ListState mutably.
        // Cancel immediately, then restore ordinary follow tracking after it exits.
        cx.defer(move |cx| {
            let _ = root.update(cx, |v, cx| {
                if interrupted {
                    let offset = v.transcript_list.logical_scroll_top();
                    v.transcript_list.set_follow_mode(FollowMode::Tail);
                    v.transcript_list.scroll_to(offset);
                }
                if v.transcript_list.is_following_tail() && v.message_motion.unread > 0 {
                    v.message_motion.unread = 0;
                    zork_ui::components::region::invalidate(cx, &["composer"]);
                }
            });
        });
    }

    pub(super) fn open_message_reader(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(TranscriptLine::Message {
            role,
            content,
            metadata,
        }) = self.lines.get(index)
        else {
            return;
        };
        let text = crate::comments::display_text(content);
        let document = crate::components::message::message_document(role, content);
        let sections = document.reader_sections();
        let plain = document.shared_plain_text();
        let mut cursor = 0;
        let offsets = sections
            .iter()
            .map(|section| {
                let text = section.shared_plain_text();
                let offset = plain
                    .get(cursor..)
                    .and_then(|tail| tail.find(text.as_ref()))
                    .map(|at| cursor + at)
                    .unwrap_or(cursor);
                cursor = (offset + text.len()).min(plain.len());
                offset
            })
            .collect();
        self.message_reader = Some(MessageReader {
            source: crate::comments::CommentSource {
                session_id: self.selected_session.clone().unwrap_or_default(),
                message_id: metadata.id.clone(),
                author: metadata.author_name.clone(),
                author_agent_id: metadata.author_agent_id.clone(),
                quote: String::new(),
            },
            text,
            scroll: ListState::new(sections.len(), ListAlignment::Top, px(300.)),
            sections: Rc::new(sections),
            plain,
            offsets: Rc::new(offsets),
            selection: Default::default(),
        });
        zork_ui::components::region::invalidate_all(cx);
    }

    pub(super) fn render_message_reader_modal(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let body = div()
            .h(px(
                (window.viewport_size().height.as_f32() - 180.).clamp(120., 660.)
            ))
            .child(self.render_message_reader(cx));
        zork_ui::modal::detail_modal(
            "message-reader-dialog",
            self.locale.text("message_full_title"),
            body,
            None,
            &self.message_reader_modal.focus,
            window,
            cx,
            true,
            |v, _, cx| {
                v.message_reader = None;
                v.regions.retain(|key| key != "message-reader");
                zork_ui::components::region::invalidate_all(cx);
            },
        )
    }

    pub(super) fn render_message_reader(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(reader) = &self.message_reader else {
            return div().into_any_element();
        };
        reader.selection.borrow_mut().begin_frame();
        let sections = reader.sections.clone();
        let selection = reader.selection.clone();
        let plain = reader.plain.clone();
        let offsets = reader.offsets.clone();
        let source = reader.source.clone();
        let focus = self.message_reader_modal.focus.clone();
        let root = cx.entity().downgrade();
        let notify: Rc<dyn Fn(&mut gpui::App)> = Rc::new(move |cx| {
            let _ = root.update(cx, |_, cx| {
                zork_ui::components::region::invalidate(cx, &["message-reader", "overlays"])
            });
        });
        let link_handler = self.message_link_handler(cx);
        let list = gpui::list(reader.scroll.clone(), move |index, _, _| {
            let document = &sections[index];
            let context = crate::components::selection::SelectionContext::new(
                "message-reader-selection".into(),
                source.clone(),
                plain.clone(),
                selection.clone(),
                focus.clone(),
                notify.clone(),
            )
            .with_link_handler(link_handler.clone())
            .with_offset(offsets[index]);
            div()
                .px_5()
                .py_2()
                .text_size(px(13.))
                .line_height(px(20.))
                .child(crate::components::message::render_selectable_document(
                    &format!("message-reader-{index}"),
                    document,
                    &context,
                ))
                .into_any_element()
        });
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .child(
                div()
                    .px_5()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(DIM))
                            .child(reader.source.author.clone().unwrap_or_default()),
                    )
                    .child(
                        div()
                            .id("message-copy-full")
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(rgb(DIM))
                            .child(self.locale.text("message_copy_full"))
                            .on_click(cx.listener(|v, _, _, cx| {
                                if let Some(reader) = &v.message_reader {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        reader.text.clone(),
                                    ));
                                }
                            }))
                            .automation(
                                AutomationRole::Button,
                                self.locale.text("message_copy_full"),
                            ),
                    ),
            )
            .child(list.flex_1().min_h_0().py_3())
            .into_any_element()
    }
}
