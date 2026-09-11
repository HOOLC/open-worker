use super::*;
use crate::comments::{CommentSource, DraftComment};
use crate::desktop::ui;

pub(super) struct CommentPopover {
    pub source: CommentSource,
    pub toolbar: bool,
    pub selection_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    pub editing: Option<String>,
    pub position: gpui::Point<gpui::Pixels>,
}
impl RootView {
    pub(super) fn dismiss_selection(&mut self, cx: &mut Context<Self>) {
        self.comment_popover = None;
        self.transcript_selection.borrow_mut().clear();
        if let Some(reader) = &self.message_reader {
            reader.selection.borrow_mut().clear();
        }
        zork_ui::components::region::invalidate(
            cx,
            &["composer", "transcript", "message-reader", "overlays"],
        );
    }
    pub(super) fn current_comments(&self) -> &[DraftComment] {
        &self.draft_state.comments
    }
    pub(super) fn finish_text_selection(
        &mut self,
        event: &gpui::MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection_bounds = if self.transcript_selection.borrow().dragging {
            self.transcript_selection.borrow().selected_bounds()
        } else {
            self.message_reader
                .as_ref()
                .filter(|r| r.selection.borrow().dragging)
                .and_then(|r| r.selection.borrow().selected_bounds())
        };
        let source = if self.transcript_selection.borrow().dragging {
            self.transcript_selection.borrow_mut().finish()
        } else if let Some(reader) = &self.message_reader {
            if !reader.selection.borrow().dragging {
                return;
            }
            reader.selection.borrow_mut().finish()
        } else {
            return;
        };
        if let Some(source) =
            source.filter(|s| Some(&s.session_id) == self.selected_session.as_ref())
        {
            self.comment_input.update(cx, |v, cx| v.clear(cx));
            self.comment_popover = Some(CommentPopover {
                source,
                toolbar: true,
                selection_bounds,
                editing: None,
                position: event.position,
            });
            window.focus(&self.overlay_focus, cx);
            zork_ui::components::region::invalidate(cx, &["composer", "transcript", "overlays"]);
        }
    }
    pub(super) fn add_comment(&mut self, cx: &mut Context<Self>) {
        let comment = self.comment_input.read(cx).value().trim().to_owned();
        if comment.is_empty() {
            return;
        }
        let Some(popover) = self.comment_popover.take() else {
            return;
        };
        if Some(&popover.source.session_id) != self.selected_session.as_ref() {
            return;
        }
        let session = popover.source.session_id.clone();
        let comment = DraftComment {
            id: popover
                .editing
                .unwrap_or_else(|| ulid::Ulid::new().to_string()),
            source: popover.source,
            comment,
        };
        if let Err(error) = self.core_device.put_comment(&session, comment) {
            self.error = Some(error.to_string());
        }
        self.draft_state = self.core_device.draft(&session);
        self.transcript_selection.borrow_mut().clear();
        if let Some(reader) = &self.message_reader {
            reader.selection.borrow_mut().clear();
        }
        self.comment_input.update(cx, |v, cx| v.clear(cx));
        self.save_draft(cx);
        zork_ui::components::region::invalidate(cx, &["composer", "transcript", "overlays"]);
    }
    pub(super) fn render_comment_queue(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        zork_ui::components::comments::queue(
            "",
            self.current_comments(),
            cx,
            |v, comment, event, w, cx| {
                v.comment_input
                    .update(cx, |input, cx| input.set_value(comment.comment.clone(), cx));
                v.comment_popover = Some(CommentPopover {
                    source: comment.source,
                    toolbar: false,
                    selection_bounds: None,
                    editing: Some(comment.id),
                    position: event.position(),
                });
                w.focus(&v.comment_input.read(cx).focus_handle(), cx);
                zork_ui::components::region::invalidate(
                    cx,
                    &["composer", "transcript", "overlays"],
                );
            },
            |v, id, cx| {
                if let Some(session) = &v.selected_session {
                    if let Err(error) = v.core_device.remove_comment(session, &id) {
                        v.error = Some(error.to_string());
                    }
                    v.draft_state = v.core_device.draft(session);
                }
                v.save_draft(cx);
                zork_ui::components::region::invalidate(
                    cx,
                    &["composer", "transcript", "overlays"],
                );
            },
        )
    }
    pub(super) fn render_comment_popover(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(popover) = &self.comment_popover else {
            return div().into_any();
        };
        if popover.toolbar {
            let width = 148_f32;
            let height = 32.;
            let bounds = popover
                .selection_bounds
                .unwrap_or_else(|| gpui::Bounds::new(popover.position, gpui::size(px(0.), px(0.))));
            let x = (bounds.center().x.as_f32() - width / 2.).clamp(
                12.,
                (window.viewport_size().width.as_f32() - width - 12.).max(12.),
            );
            let above = bounds.top().as_f32() - height - 8.;
            let y = if above >= 12. {
                above
            } else {
                bounds.bottom().as_f32() + 8.
            }
            .clamp(
                12.,
                (window.viewport_size().height.as_f32() - height - 12.).max(12.),
            );
            return div()
                .id("selection-toolbar")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(width))
                .occlude()
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(3.))
                .h(px(height))
                .rounded(px(ui::MENU_RADIUS))
                .border_1()
                .border_color(rgb(CUE_UI.palette.border))
                .bg(rgb(CUE_UI.palette.canvas))
                .shadow(vec![BoxShadow::new(
                    px(0.),
                    px(3.),
                    rgba(0x00000012).into(),
                )
                .blur_radius(px(12.))])
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                    v.dismiss_selection(cx);
                }))
                .child(
                    ui::quiet_button("selection-copy", "", true, ui::IconButtonSize::Standard)
                        .h(px(24.))
                        .border_0()
                        .font_weight(FontWeight::NORMAL)
                        .flex_1()
                        .child(ui::icon("icons/copy.svg", 13.))
                        .child("复制")
                        .on_click(cx.listener(|v, _, _, cx| {
                            if let Some(popover) = &v.comment_popover {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    popover.source.quote.clone(),
                                ));
                            }
                            v.dismiss_selection(cx);
                        }))
                        .automation(AutomationRole::Button, "复制选中文字"),
                )
                .child(div().w(px(1.)).h(px(12.)).bg(rgb(CUE_UI.palette.border)))
                .child(
                    ui::quiet_button("selection-comment", "", true, ui::IconButtonSize::Standard)
                        .h(px(24.))
                        .border_0()
                        .font_weight(FontWeight::NORMAL)
                        .flex_1()
                        .child(ui::icon("icons/message-square.svg", 13.))
                        .child("评论")
                        .on_click(cx.listener(|v, _, window, cx| {
                            if let Some(popover) = &mut v.comment_popover {
                                popover.toolbar = false;
                            }
                            window.focus(&v.comment_input.read(cx).focus_handle(), cx);
                            zork_ui::components::region::invalidate(cx, &["overlays"]);
                        }))
                        .automation(AutomationRole::Button, "评论选中文字"),
                )
                .automation(AutomationRole::Status, "选中文字工具条")
                .into_any_element();
        }
        let width = 304_f32.min(window.viewport_size().width.as_f32() - 24.);
        let x = (popover.position.x.as_f32() - width / 2.).clamp(
            12.,
            (window.viewport_size().width.as_f32() - width - 12.).max(12.),
        );
        let y = (popover.position.y.as_f32() + 12.)
            .min((window.viewport_size().height.as_f32() - 230.).max(12.));
        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .child(zork_ui::components::comments::popover(
                "",
                popover.source.quote.clone(),
                popover.editing.is_some(),
                &self.comment_input,
                cx,
                |v, cx| {
                    v.comment_popover = None;
                    v.transcript_selection.borrow_mut().clear();
                    zork_ui::components::region::invalidate(
                        cx,
                        &["composer", "transcript", "overlays"],
                    );
                },
                |v, cx| v.add_comment(cx),
            ))
            .into_any_element()
    }
}
