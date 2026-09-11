//! Quoted comment presentation. Hosts decide when to persist or send a batch.
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    comments::DraftComment,
    components::text_input::ComposerInput,
    controls as ui,
    design::CUE_UI,
};
use gpui::{div, prelude::*, px, rgb, rgba, BoxShadow, ClickEvent, Context, Div, Entity, Window};
use std::rc::Rc;
fn id(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.into()
    } else {
        format!("{prefix}-{suffix}")
    }
}
pub fn queue<V: 'static>(
    prefix: &str,
    comments: &[DraftComment],
    cx: &Context<V>,
    edit: impl Fn(&mut V, DraftComment, &ClickEvent, &mut Window, &mut Context<V>) + 'static,
    remove: impl Fn(&mut V, String, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    let edit = Rc::new(edit);
    let remove = Rc::new(remove);
    let p = CUE_UI.palette;
    div()
        .id(id(prefix, "composer-comment-queue"))
        .flex()
        .flex_col()
        .gap(px(2.))
        .max_h(px(140.))
        .overflow_y_scroll()
        .children(comments.iter().cloned().map(|comment| {
            let editing = comment.clone();
            let key = comment.id.clone();
            let edit = edit.clone();
            let remove = remove.clone();
            div()
                .id(id(prefix, &format!("queued-comment-{}", comment.id)))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_2()
                .rounded(px(6.))
                .bg(rgb(p.prompt))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(p.muted))
                                .truncate()
                                .child(format!(
                                    "{}：{}",
                                    comment.source.author.as_deref().unwrap_or("消息"),
                                    comment.source.quote
                                )),
                        )
                        .child(div().text_size(px(12.)).truncate().child(comment.comment)),
                )
                .child(
                    ui::button(
                        id(prefix, &format!("comment-edit-{}", comment.id)),
                        "编辑",
                        false,
                        true,
                    )
                    .on_click(
                        cx.listener(move |v, event, w, cx| edit(v, editing.clone(), event, w, cx)),
                    )
                    .automation(AutomationRole::Button, "编辑评论"),
                )
                .child(
                    ui::icon_button(id(prefix, &format!("comment-remove-{}", comment.id)), true)
                        .w(px(ui::CONTROL_HEIGHT))
                        .h(px(ui::CONTROL_HEIGHT))
                        .px_0()
                        .border_0()
                        .child(ui::icon("icons/x.svg", 12.))
                        .on_click(cx.listener(move |v, _, _, cx| remove(v, key.clone(), cx)))
                        .automation(AutomationRole::Button, "移除评论"),
                )
        }))
        .into_any_element()
}
pub fn popover<V: 'static>(
    prefix: &str,
    quote: String,
    editing: bool,
    input: &Entity<ComposerInput>,
    cx: &Context<V>,
    close: impl Fn(&mut V, &mut Context<V>) + 'static,
    save: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> gpui::Stateful<Div> {
    let p = CUE_UI.palette;
    let enabled = !input.read(cx).value().trim().is_empty();
    div()
        .id(id(prefix, "selection-comment-popover"))
        .occlude()
        .w_full()
        .rounded(px(ui::MENU_RADIUS))
        .border_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.canvas))
        .shadow(vec![BoxShadow::new(
            px(0.),
            px(8.),
            rgba(0x24272B12).into(),
        )
        .blur_radius(px(26.))])
        .p(px(12.))
        .flex()
        .flex_col()
        .gap_2()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(ui::label("评论这段内容"))
                .child(
                    ui::icon_button(id(prefix, "comment-popover-close"), true)
                        .size(px(ui::CONTROL_HEIGHT))
                        .px_0()
                        .border_0()
                        .child(ui::icon("icons/x.svg", 12.))
                        .on_click(cx.listener(move |v, _, _, cx| close(v, cx)))
                        .automation(AutomationRole::Button, "关闭评论"),
                ),
        )
        .child(
            div()
                .id(id(prefix, "comment-selected-quote"))
                .max_h(px(60.))
                .overflow_y_scroll()
                .px_2()
                .py_1()
                .rounded(px(4.))
                .bg(rgb(p.prompt))
                .text_size(px(12.))
                .text_color(rgb(p.muted))
                .child(quote.clone())
                .automation(AutomationRole::Status, quote),
        )
        .child(
            div()
                .id(id(prefix, "comment-input"))
                .h(px(64.))
                .child(input.clone())
                .automation(AutomationRole::TextInput, "针对选中文字评论"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(p.muted))
                        .child("Enter 添加 · 最后在输入框一次发送"),
                )
                .child(
                    ui::button(
                        id(prefix, "comment-queue-add"),
                        if editing {
                            "保存评论"
                        } else {
                            "添加评论"
                        },
                        true,
                        enabled,
                    )
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if enabled {
                            save(v, cx);
                        }
                    }))
                    .automation_enabled(
                        enabled,
                        AutomationRole::Button,
                        "添加到输入框",
                    ),
                ),
        )
}
#[cfg(feature = "stories")]
pub struct CommentsStory {
    prefix: String,
    comments: Vec<DraftComment>,
    input: Entity<ComposerInput>,
    open: bool,
    editing: Option<String>,
    next_id: usize,
}
#[cfg(feature = "stories")]
impl CommentsStory {
    pub fn new(prefix: String, state: &str, cx: &mut Context<Self>) -> Self {
        use crate::components::text_input::ComposerSubmit;
        let input = cx.new(|cx| ComposerInput::new("针对这段内容说点什么…", cx));
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&input, |v, _, _: &ComposerSubmit, cx| v.save(cx))
            .detach();
        let comments = if matches!(state, "queued" | "editing") {
            vec![DraftComment {
                id: "demo".into(),
                source: crate::comments::CommentSource {
                    author: Some("产品 Leader".into()),
                    quote: "请先统一图标与头像。".into(),
                    ..Default::default()
                },
                comment: "保持紧凑，文字需要清晰。".into(),
            }]
        } else {
            vec![]
        };
        if state == "editing" {
            input.update(cx, |v, cx| v.set_value("保持紧凑，文字需要清晰。", cx));
        }
        Self {
            prefix,
            comments,
            input,
            open: matches!(state, "compose" | "editing"),
            editing: (state == "editing").then(|| "demo".into()),
            next_id: 1,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().trim().to_owned();
        if text.is_empty() || !self.open {
            return;
        }
        if let Some(id) = self.editing.take() {
            if let Some(comment) = self.comments.iter_mut().find(|c| c.id == id) {
                comment.comment = text;
            }
        } else {
            self.comments.push(DraftComment {
                id: format!("demo-{}", self.next_id),
                source: crate::comments::CommentSource {
                    author: Some("产品 Leader".into()),
                    quote: "请先统一图标与头像。".into(),
                    ..Default::default()
                },
                comment: text,
            });
            self.next_id += 1;
        }
        self.open = false;
        self.input.update(cx, |v, cx| v.clear(cx));
        cx.notify();
    }
}
#[cfg(feature = "stories")]
impl gpui::Render for CommentsStory {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .on_key_down(cx.listener(|v, e: &gpui::KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    v.open = false;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .when(!self.comments.is_empty(), |v| {
                v.child(queue(
                    &self.prefix,
                    &self.comments,
                    cx,
                    |v, comment, _, w, cx| {
                        v.input
                            .update(cx, |input, cx| input.set_value(comment.comment, cx));
                        v.editing = Some(comment.id);
                        v.open = true;
                        w.focus(&v.input.read(cx).focus_handle(), cx);
                        cx.notify();
                    },
                    |v, id, cx| {
                        v.comments.retain(|c| c.id != id);
                        cx.notify();
                    },
                ))
            })
            .when(self.open, |v| {
                v.child(div().w(px(320.)).max_w_full().child(popover(
                    &self.prefix,
                    "请先统一图标与头像。".into(),
                    self.editing.is_some(),
                    &self.input,
                    cx,
                    |v, cx| {
                        v.open = false;
                        cx.notify();
                    },
                    |v, cx| v.save(cx),
                )))
            })
            .when(!self.open, |v| {
                v.child(
                    div().flex().child(
                        ui::button(id(&self.prefix, "add-comment"), "添加评论", false, true)
                            .on_click(cx.listener(|v, _, w, cx| {
                                v.open = true;
                                v.editing = None;
                                v.input.update(cx, |input, cx| input.clear(cx));
                                w.focus(&v.input.read(cx).focus_handle(), cx);
                                cx.notify();
                            }))
                            .automation(AutomationRole::Button, "添加评论"),
                    ),
                )
            })
    }
}
