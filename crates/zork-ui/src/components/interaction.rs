//! Presentation-only interaction card. Business actions, validation and outcome
//! state arrive from core; the entity owns only unsubmitted field buffers.
use super::text_input::ComposerInput;
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    controls,
    design::CUE_UI,
};
use gpui::{
    div, prelude::*, px, rgb, Context, Entity, EventEmitter, FontWeight, SharedString, Window,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Multiline,
    Choice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: String,
    pub label: SharedString,
    pub kind: FieldKind,
    pub value: SharedString,
    pub options: Vec<(String, SharedString)>,
    pub error: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: SharedString,
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub id: String,
    pub title: SharedString,
    pub status: SharedString,
    pub fields: Vec<Field>,
    pub details: Vec<(SharedString, SharedString)>,
    pub actions: Vec<Action>,
    pub editable: bool,
    pub placeholder: SharedString,
    pub error: Option<SharedString>,
}

pub struct Activated {
    pub action: String,
    pub values: BTreeMap<String, String>,
}

pub struct InteractionCard {
    view: View,
    inputs: HashMap<String, Entity<ComposerInput>>,
    choices: HashMap<String, String>,
    open_choice: Option<String>,
    on_action: Option<std::rc::Rc<dyn Fn(&Activated, &mut gpui::App)>>,
}
impl EventEmitter<Activated> for InteractionCard {}

impl InteractionCard {
    pub fn new(view: View, cx: &mut Context<Self>) -> Self {
        let mut card = Self {
            view: view.clone(),
            inputs: HashMap::new(),
            choices: HashMap::new(),
            open_choice: None,
            on_action: None,
        };
        card.synchronize_fields(&view, true, cx);
        card
    }

    pub fn with_action_handler(
        mut self,
        handler: impl Fn(&Activated, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_action = Some(std::rc::Rc::new(handler));
        self
    }

    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        let reset = self.view.id != view.id || !self.view.editable || !view.editable;
        self.synchronize_fields(&view, reset, cx);
        if !view.editable {
            self.open_choice = None;
        }
        self.view = view;
        cx.notify();
    }

    fn synchronize_fields(&mut self, view: &View, reset: bool, cx: &mut Context<Self>) {
        self.inputs
            .retain(|id, _| view.fields.iter().any(|f| &f.id == id));
        self.choices
            .retain(|id, _| view.fields.iter().any(|f| &f.id == id));
        for field in &view.fields {
            if field.kind == FieldKind::Choice {
                if reset || !self.choices.contains_key(&field.id) {
                    self.choices
                        .insert(field.id.clone(), field.value.to_string());
                }
            } else {
                let fresh = !self.inputs.contains_key(&field.id);
                let input = self.inputs.entry(field.id.clone()).or_insert_with(|| {
                    cx.new(|cx| {
                        let input = ComposerInput::new(field.label.clone(), cx);
                        if field.kind == FieldKind::Text {
                            input.single_line()
                        } else {
                            input
                        }
                    })
                });
                if fresh || reset {
                    input.update(cx, |input, cx| input.set_value(field.value.to_string(), cx));
                }
            }
        }
    }

    fn activate(&self, action: &str, cx: &mut Context<Self>) {
        let mut values = BTreeMap::new();
        for field in &self.view.fields {
            let value = if field.kind == FieldKind::Choice {
                self.choices.get(&field.id).cloned().unwrap_or_default()
            } else {
                self.inputs
                    .get(&field.id)
                    .map(|input| input.read(cx).value().to_owned())
                    .unwrap_or_default()
            };
            values.insert(field.id.clone(), value);
        }
        let event = Activated {
            action: action.into(),
            values,
        };
        if let Some(handler) = &self.on_action {
            handler(&event, cx);
        }
        cx.emit(event);
    }
}

impl Render for InteractionCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = CUE_UI.palette;
        let mut body = div()
            .id(format!("interaction-card-{}", self.view.id))
            .w_full()
            .min_w(px(0.))
            .max_w(px(620.))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(p.border_strong))
            .bg(rgb(p.elevated))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text))
                            .child(self.view.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(p.muted))
                            .child(self.view.status.clone()),
                    ),
            );
        for field in self.view.fields.clone() {
            let id = format!("interaction-{}-{}", self.view.id, field.id);
            let control = if !self.view.editable {
                let text = if field.kind == FieldKind::Choice {
                    field
                        .options
                        .iter()
                        .find(|(value, _)| value.as_str() == field.value.as_ref())
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| field.value.clone())
                } else {
                    field.value.clone()
                };
                div()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(rgb(p.text))
                    .child(text)
                    .into_any_element()
            } else if field.kind == FieldKind::Choice {
                let selected = self.choices.get(&field.id).cloned().unwrap_or_default();
                let label = field
                    .options
                    .iter()
                    .find(|(value, _)| *value == selected)
                    .map(|(_, label)| label.to_string())
                    .unwrap_or_else(|| self.view.placeholder.to_string());
                let open_id = field.id.clone();
                let choose_id = field.id.clone();
                let options = field.options.clone();
                controls::dropdown(
                    id.clone(),
                    label,
                    field
                        .options
                        .iter()
                        .enumerate()
                        .map(|(i, (value, label))| {
                            (format!("{id}-{i}"), label.to_string(), *value == selected)
                        })
                        .collect(),
                    self.open_choice.as_ref() == Some(&field.id),
                    true,
                    window,
                    cx,
                    move |view, open, cx| {
                        view.open_choice = open.then(|| open_id.clone());
                        cx.notify();
                    },
                    move |view, index, cx| {
                        if let Some((value, _)) = options.get(index) {
                            view.choices.insert(choose_id.clone(), value.clone());
                        }
                        view.open_choice = None;
                        cx.notify();
                    },
                )
            } else {
                let input = self.inputs[&field.id].clone();
                controls::input_control(id, &input, field.error.is_some(), cx)
                    .when(field.kind == FieldKind::Multiline, |field| field.h(px(96.)))
                    .automation(AutomationRole::TextInput, field.label.clone())
                    .into_any_element()
            };
            let row = div()
                .flex()
                .flex_col()
                .gap_1()
                .min_w(px(0.))
                .child(controls::label(field.label))
                .child(control)
                .when_some(field.error, |row, error| {
                    row.child(controls::feedback(error.to_string()))
                });
            body = body.child(row);
        }
        if !self.view.details.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(p.border_strong))
                    .children(self.view.details.iter().map(|(label, value)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(p.muted))
                                    .child(label.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(rgb(p.text))
                                    .child(value.clone()),
                            )
                    })),
            );
        }
        if let Some(error) = &self.view.error {
            body = body.child(controls::feedback(error.to_string()));
        }
        if !self.view.actions.is_empty() {
            let mut actions = div().flex().flex_wrap().items_center().gap_2().pt_1();
            for action in self.view.actions.clone() {
                let id = format!("interaction-{}-{}", self.view.id, action.id);
                actions = actions.child(
                    controls::button(id, action.label.clone(), action.primary, true)
                        .on_click(cx.listener(move |view, _, _, cx| view.activate(&action.id, cx)))
                        .automation(AutomationRole::Button, action.label),
                );
            }
            body = body.child(actions);
        }
        body
    }
}
