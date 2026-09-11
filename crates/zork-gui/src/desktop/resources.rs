//! Purpose-specific resource views composed into the existing settings and Agent UI.
use super::ui;
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    i18n::Locale,
};
use gpui::{div, prelude::*, px, rgb, Context, Div, Render, Task, Window};
use std::sync::Arc;
use zork_client_core::resources::{
    Inspection, InspectionContent, ResourceKind, Resources, ResourcesData,
};
use zork_ui::{
    components::{
        frame_delivery::FrameDelivery,
        message::{render_document, MessageDocument},
    },
    design::TextRole,
};

#[derive(Clone)]
enum Mode {
    Connections,
    Services(String),
    Skills { node: String, agent: String },
    Inspector,
}

pub struct ResourcesView {
    core: Arc<Resources>,
    data: Arc<ResourcesData>,
    locale: Locale,
    mode: Mode,
    rows: Arc<Vec<(usize, usize)>>,
    selected: Option<(String, Inspection)>,
    opening_error: Option<String>,
    trail: Vec<(String, Inspection)>,
    updates: zork_client_core::state::Subscription<ResourcesData>,
    frame: FrameDelivery,
    _task: Task<()>,
    modal: ui::ModalState,
    document: Option<(String, bool, MessageDocument)>,
    parameters: Option<String>,
    facts_open: bool,
    focus_pending: bool,
}
impl ResourcesView {
    pub fn new(core: Arc<Resources>, locale: Locale, cx: &mut Context<Self>) -> Self {
        Self::create(core, locale, Mode::Connections, cx)
    }
    pub fn services(
        core: Arc<Resources>,
        node: String,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::create(core, locale, Mode::Services(node), cx)
    }
    pub fn skills(
        core: Arc<Resources>,
        node: String,
        agent: String,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::create(core, locale, Mode::Skills { node, agent }, cx)
    }
    pub fn inspector(
        core: Arc<Resources>,
        node: String,
        query: Inspection,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::create(core, locale, Mode::Inspector, cx);
        view.select(node, query, cx);
        view
    }
    pub fn unavailable(
        core: Arc<Resources>,
        error: String,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::create(core, locale, Mode::Inspector, cx);
        view.opening_error = Some(error);
        view
    }
    #[cfg(feature = "headless-bench")]
    pub fn fixture(data: ResourcesData, locale: Locale, cx: &mut Context<Self>) -> Self {
        Self::new(Resources::fixture(data), locale, cx)
    }
    fn create(core: Arc<Resources>, locale: Locale, mode: Mode, cx: &mut Context<Self>) -> Self {
        let mut updates = core.subscribe();
        let data = updates.snapshot();
        let mut readiness = updates.readiness();
        let task = cx.spawn(async move |view, cx| {
            while readiness.changed().await.is_ok() {
                if readiness.take_urgent() {
                    if view.update(cx, |v, cx| v.deliver(cx)).is_err() {
                        return;
                    }
                } else if !FrameDelivery::request(&view, cx, |v| &mut v.frame, Self::deliver) {
                    return;
                }
            }
        });
        let mut view = Self {
            core,
            data,
            locale,
            mode,
            rows: Arc::new(vec![]),
            selected: None,
            opening_error: None,
            trail: vec![],
            updates,
            frame: Default::default(),
            _task: task,
            modal: ui::ModalState::new(cx),
            document: None,
            parameters: None,
            facts_open: false,
            focus_pending: false,
        };
        view.project();
        if !matches!(view.mode, Mode::Inspector) {
            view.refresh(cx);
        }
        view
    }
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        if self.locale != locale {
            self.locale = locale;
            cx.notify();
        }
    }
    pub fn refresh(&self, cx: &mut Context<Self>) {
        let core = self.core.clone();
        let query = self.selected.clone().or_else(|| match &self.mode {
            Mode::Skills { node, agent } => {
                Some((node.clone(), Inspection::AgentSkills(agent.clone())))
            }
            _ => None,
        });
        cx.background_executor()
            .spawn(async move {
                if let Some((node, query)) = query {
                    core.inspect(&node, query).await;
                } else {
                    core.refresh().await;
                }
            })
            .detach();
    }
    fn deliver(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = self.updates.prepare() {
            let id = batch.id;
            self.data = batch.snapshot.value.clone();
            self.project();
            self.updates.acknowledge(id);
            cx.notify();
        }
    }
    fn project(&mut self) {
        if self
            .selected
            .as_ref()
            .is_some_and(|(node, _)| !self.data.devices.iter().any(|device| &device.id == node))
        {
            self.selected = None;
            self.trail.clear();
            self.parameters = None;
            self.document = None;
        }
        self.rows = Arc::new(match &self.mode {
            Mode::Connections => self.data.rows(ResourceKind::Mcp, None),
            Mode::Services(node) => self.data.rows(ResourceKind::Service, Some(node)),
            _ => vec![],
        });
        let source = self
            .selected
            .as_ref()
            .and_then(|(node, query)| self.data.inspection(node, query))
            .and_then(|state| state.content.as_deref())
            .and_then(|content| match content {
                InspectionContent::Details(details) => {
                    if let Some(tool) = &self.parameters {
                        details
                            .tools
                            .iter()
                            .find(|t| &t.name == tool)
                            .and_then(|t| serde_json::to_string_pretty(&t.input_schema).ok())
                            .map(|text| (text, false))
                    } else {
                        details.document.as_ref().map(|d| {
                            (
                                d.text.clone(),
                                d.path.ends_with(".md") || d.path.ends_with(".markdown"),
                            )
                        })
                    }
                }
                _ => None,
            });
        if self
            .document
            .as_ref()
            .map(|(text, markdown, _)| (text, *markdown))
            != source.as_ref().map(|(text, markdown)| (text, *markdown))
        {
            self.document = source.map(|(text, markdown)| {
                let doc = if markdown {
                    MessageDocument::parse(&text)
                } else {
                    MessageDocument::plain(&text)
                };
                (text, markdown, doc)
            });
        }
    }
    fn select(&mut self, node: String, query: Inspection, cx: &mut Context<Self>) {
        if let Some(previous) = self.selected.replace((node, query)) {
            self.trail.push(previous);
        }
        self.parameters = None;
        self.facts_open = false;
        self.focus_pending = true;
        self.project();
        self.refresh(cx);
        cx.notify();
    }
    fn back(&mut self, cx: &mut Context<Self>) {
        self.focus_pending = true;
        if self.parameters.take().is_some() {
            self.project();
            cx.notify();
            return;
        }
        self.selected = self.trail.pop();
        self.project();
        // The shared bounded cache may have evicted an earlier file while the
        // user explored another resource. Returning is an explicit read intent.
        self.refresh(cx);
        cx.notify();
    }
    fn close(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.opening_error = None;
        self.trail.clear();
        self.parameters = None;
        self.document = None;
        cx.notify();
    }
    fn title(&self) -> String {
        if let Some(tool) = &self.parameters {
            return format!("{tool} · {}", self.locale.text("resource_parameters"));
        }
        self.selected
            .as_ref()
            .and_then(|(node, query)| self.data.inspection(node, query))
            .and_then(|s| s.content.as_deref())
            .and_then(|content| match content {
                InspectionContent::Details(d) => Some(
                    d.document
                        .as_ref()
                        .map(|f| f.path.clone())
                        .filter(|_| !self.trail.is_empty())
                        .unwrap_or_else(|| d.title.clone()),
                ),
                _ => None,
            })
            .unwrap_or_else(|| self.locale.text("resource_details").into())
    }
    fn notice(&self, text: String) -> Div {
        div().py_2().child(ui::feedback(text))
    }
    fn list(&self, cx: &mut Context<Self>) -> Div {
        if let Mode::Skills { node, agent } = &self.mode {
            let query = Inspection::AgentSkills(agent.clone());
            let state = self.data.inspection(node, &query);
            let mut body = ui::section().child(ui::text_role(
                self.locale.text("agent_skills"),
                TextRole::SectionTitle,
            ));
            if let Some(state) = state {
                if state.loading {
                    body = body.child(ui::text_role(
                        self.locale.text("resource_loading"),
                        TextRole::Description,
                    ));
                }
                if let Some(error) = &state.error {
                    body = body.child(self.notice(error.clone()));
                }
                if let Some(InspectionContent::Skills(catalog)) = state.content.as_deref() {
                    if catalog.skills.is_empty() {
                        body = body.child(ui::text_role(
                            self.locale.text("agent_skills_empty"),
                            TextRole::Description,
                        ));
                    }
                    for skill in &catalog.skills {
                        let (node, agent, skill_id) =
                            (node.clone(), agent.clone(), skill.id.clone());
                        body = body.child(zork_ui::components::attachments::content_row(
                            format!("agent-skill-{}", skill.id),
                            "icons/file.svg",
                            skill.name.clone(),
                            skill.description.clone(),
                            cx,
                            move |view, cx| {
                                view.select(
                                    node.clone(),
                                    Inspection::Skill {
                                        agent: agent.clone(),
                                        skill: skill_id.clone(),
                                        file: None,
                                    },
                                    cx,
                                )
                            },
                        ));
                    }
                    for diagnostic in &catalog.diagnostics {
                        body = body.child(self.notice(diagnostic.clone()));
                    }
                }
            }
            return body;
        }
        let kind = if matches!(self.mode, Mode::Services(_)) {
            ResourceKind::Service
        } else {
            ResourceKind::Mcp
        };
        let title = self.locale.text(if kind == ResourceKind::Mcp {
            "tool_connections"
        } else {
            "device_services"
        });
        let mut body = div().w_full().flex().flex_col().gap_3().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(ui::text_role(title, TextRole::SectionTitle))
                .child(
                    ui::icon_button("resource-refresh", true)
                        .child(ui::icon("icons/reload.svg", 14.))
                        .on_click(cx.listener(|view, _, _, cx| view.refresh(cx)))
                        .automation(AutomationRole::Button, self.locale.text("refresh")),
                ),
        );
        let scope = match &self.mode {
            Mode::Services(node) => Some(node.as_str()),
            _ => None,
        };
        for device in self
            .data
            .devices
            .iter()
            .filter(|d| scope.is_none_or(|id| d.id == id))
        {
            if let Some(error) = &device.error {
                body = body.child(self.notice(format!("{} · {error}", device.name)));
            }
            if let Some(catalog) = &device.catalog {
                for issue in catalog.issues.iter().filter(|issue| issue.kind == kind) {
                    body = body.child(self.notice(format!("{} · {}", device.name, issue.error)));
                }
            }
        }
        if self.rows.is_empty() {
            let failed = self
                .data
                .devices
                .iter()
                .filter(|device| scope.is_none_or(|id| device.id == id))
                .any(|device| {
                    device.error.is_some()
                        || device.catalog.as_ref().is_some_and(|catalog| {
                            catalog.issues.iter().any(|issue| issue.kind == kind)
                        })
                });
            if failed {
                return body;
            }
            return body.child(ui::text_role(
                self.locale
                    .text(if self.data.devices.iter().any(|d| d.loading) {
                        "resource_loading"
                    } else if kind == ResourceKind::Mcp {
                        "tool_connections_empty"
                    } else {
                        "device_services_empty"
                    }),
                TextRole::Description,
            ));
        }
        let rows = self.rows.clone();
        let data = self.data.clone();
        let locale = self.locale;
        body = body.child(
            gpui::uniform_list(
                "registered-resources",
                rows.len(),
                cx.processor(move |_: &mut Self, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| {
                            let (di, ri) = rows[index];
                            let device = &data.devices[di];
                            let item = &device.catalog.as_ref().unwrap().items[ri];
                            let node = device.id.clone();
                            let query = if item.kind == ResourceKind::Mcp {
                                Inspection::Mcp(item.id.clone())
                            } else {
                                Inspection::Service {
                                    id: item.id.clone(),
                                    log: None,
                                }
                            };
                            let meta = if item.description.is_empty() {
                                format!("{} · {}", device.name, status(locale, &item.status))
                            } else {
                                format!(
                                    "{} · {} · {}",
                                    item.description,
                                    device.name,
                                    status(locale, &item.status)
                                )
                            };
                            div().h(px(56.)).pb_2().child(
                                zork_ui::components::attachments::content_row(
                                    format!("resource-row-{index}"),
                                    if item.kind == ResourceKind::Mcp {
                                        "icons/mesh.svg"
                                    } else {
                                        "icons/node.svg"
                                    },
                                    item.name.clone(),
                                    meta,
                                    cx,
                                    move |view, cx| view.select(node.clone(), query.clone(), cx),
                                ),
                            )
                        })
                        .collect()
                }),
            )
            .h(px((self.rows.len().min(8) * 56) as f32))
            .w_full(),
        );
        body
    }
    fn detail(&self, cx: &mut Context<Self>) -> Div {
        if let Some(error) = &self.opening_error {
            return self.notice(error.clone());
        }
        let Some((node, query)) = &self.selected else {
            return div();
        };
        let mut body = div().w_full().flex().flex_col().gap_3();
        if matches!(self.mode, Mode::Skills { .. }) {
            body = body.child(ui::text_role(self.title(), TextRole::SectionTitle));
        }
        if !self.trail.is_empty()
            || matches!(self.mode, Mode::Skills { .. })
            || self.parameters.is_some()
        {
            body = body.child(
                ui::quiet_button("resource-back", "", true, ui::IconButtonSize::Compact)
                    .self_start()
                    .child(ui::icon("icons/arrow-left.svg", 14.))
                    .child(self.locale.text("back"))
                    .on_click(cx.listener(|view, _, _, cx| view.back(cx)))
                    .automation(AutomationRole::Button, self.locale.text("back")),
            );
        }
        let Some(state) = self.data.inspection(node, query) else {
            if let Some(error) = self
                .data
                .devices
                .iter()
                .find(|device| &device.id == node)
                .and_then(|device| device.error.as_ref())
            {
                return body.child(self.notice(error.clone())).child(
                    ui::button("resource-retry", self.locale.text("retry"), false, true)
                        .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
                );
            }
            return body.child(ui::text_role(
                self.locale.text("resource_loading"),
                TextRole::Description,
            ));
        };
        if state.loading {
            body = body.child(ui::text_role(
                self.locale.text("resource_loading"),
                TextRole::Description,
            ));
        }
        if let Some(error) = &state.error {
            body = body.child(self.notice(error.clone())).child(
                ui::button(
                    "resource-retry",
                    self.locale.text("retry"),
                    false,
                    !state.loading,
                )
                .on_click(cx.listener(|view, _, _, cx| view.refresh(cx))),
            );
        }
        let Some(InspectionContent::Details(details)) = state.content.as_deref() else {
            return body;
        };
        if let Some(device) = self.data.devices.iter().find(|d| &d.id == node) {
            body = body.child(ui::text_role(device.name.clone(), TextRole::Metadata));
        }
        if !details.description.is_empty() {
            body = body.child(ui::text_role(
                details.description.clone(),
                TextRole::Description,
            ));
        }
        for (key, value) in &details.facts {
            if matches!(key.as_str(), "last_error" | "inspection_error") {
                body = body.child(self.notice(value.clone()));
            }
        }
        if let Some((_, _, document)) = &self.document {
            body = body.child(
                div()
                    .id("resource-document-scroll")
                    .w_full()
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(render_document("resource-document", document)),
            );
            if details.document.as_ref().is_some_and(|d| d.truncated) {
                body = body.child(ui::text_role(
                    self.locale.text("resource_document_truncated"),
                    TextRole::Metadata,
                ));
            }
        }
        if self.parameters.is_some() {
            return body;
        }
        if !details.tools.is_empty() {
            body = body.child(ui::text_role(
                self.locale.text("resource_tool_list"),
                TextRole::SectionTitle,
            ));
            let content = state.content.clone().unwrap();
            body = body.child(
                gpui::uniform_list(
                    "resource-tools",
                    details.tools.len(),
                    cx.processor(move |_: &mut Self, range: std::ops::Range<usize>, _, cx| {
                        let InspectionContent::Details(details) = content.as_ref() else {
                            return vec![];
                        };
                        range
                            .map(|index| {
                                let tool = &details.tools[index];
                                let name = tool.name.clone();
                                div().h(px(52.)).pb_1().child(
                                    zork_ui::components::attachments::content_row(
                                        format!("resource-tool-{}", tool.name),
                                        "icons/mesh.svg",
                                        tool.name.clone(),
                                        tool.description.clone(),
                                        cx,
                                        move |view, cx| {
                                            view.parameters = Some(name.clone());
                                            view.focus_pending = true;
                                            view.project();
                                            cx.notify();
                                        },
                                    ),
                                )
                            })
                            .collect()
                    }),
                )
                .w_full()
                .h(px(details.tools.len().min(6) as f32 * 52.)),
            );
        }
        if !details.files.is_empty() {
            body = body.child(ui::text_role(
                self.locale
                    .text(if matches!(query, Inspection::Service { .. }) {
                        "resource_logs"
                    } else {
                        "resource_attached_files"
                    }),
                TextRole::SectionTitle,
            ));
            for file in &details.files {
                let node = node.clone();
                let next = match query {
                    Inspection::Skill { agent, skill, .. } => Some(Inspection::Skill {
                        agent: agent.clone(),
                        skill: skill.clone(),
                        file: Some(file.path.clone()),
                    }),
                    Inspection::Service { id, .. } => Some(Inspection::Service {
                        id: id.clone(),
                        log: Some(file.path.clone()),
                    }),
                    _ => None,
                };
                if let Some(next) = next {
                    body = body.child(zork_ui::components::attachments::content_row(
                        format!("resource-file-{}", file.path),
                        "icons/file.svg",
                        file.path.clone(),
                        String::new(),
                        cx,
                        move |view, cx| view.select(node.clone(), next.clone(), cx),
                    ));
                }
            }
        }
        body = body.child(
            ui::quiet_button(
                "resource-more",
                self.locale.text("resource_information"),
                true,
                ui::IconButtonSize::Compact,
            )
            .self_start()
            .child(ui::icon("icons/phosphor-caret-down.svg", 12.))
            .on_click(cx.listener(|view, _, _, cx| {
                view.facts_open = !view.facts_open;
                cx.notify();
            }))
            .automation(
                AutomationRole::Button,
                self.locale.text("resource_information"),
            ),
        );
        if self.facts_open {
            for (key, value) in &details.facts {
                if matches!(
                    key.as_str(),
                    "files_truncated" | "last_error" | "inspection_error"
                ) {
                    continue;
                }
                let label = self.locale.text(match key.as_str() {
                    "status" => "resource_status",
                    "mode" => "resource_mode",
                    "port" => "resource_port",
                    "shared" => "resource_sharing",
                    "owner_session" => "resource_owner",
                    "last_started_at" => "resource_started",
                    "source" => "resource_source",
                    "path" => "resource_path",
                    "protocol" => "resource_protocol",
                    _ => "resource_information",
                });
                let value = if key == "status" {
                    status(self.locale, value)
                } else {
                    value.clone()
                };
                body = body.child(
                    div()
                        .flex()
                        .gap_4()
                        .child(ui::text_role(label, TextRole::Label).w(px(90.)))
                        .child(ui::text_role(value, TextRole::Body).flex_1().min_w_0()),
                );
            }
        }
        body
    }
}
impl Render for ResourcesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.frame.enter(window) {
            self.deliver(cx);
        }
        let inline = matches!(self.mode, Mode::Skills { .. });
        let detail_open = (self.selected.is_some() || self.opening_error.is_some()) && !inline;
        self.modal
            .sync(detail_open.then_some("resource-detail-modal"), window, cx);
        if self.focus_pending {
            window.focus(&self.modal.focus, cx);
            self.focus_pending = false;
        }
        let mut body = if matches!(self.mode, Mode::Inspector) {
            div()
        } else if inline && self.selected.is_some() {
            self.detail(cx)
        } else {
            self.list(cx)
        };
        if detail_open {
            body = body.child(ui::detail_modal(
                "resource-detail-modal",
                self.title(),
                self.detail(cx),
                None,
                &self.modal.focus,
                window,
                cx,
                true,
                |view, _, cx| view.close(cx),
            ));
        }
        if inline {
            body = body.track_focus(&self.modal.focus);
        }
        body.font_family("Inter Variable")
            .text_size(px(13.))
            .text_color(rgb(crate::design::CUE_UI.palette.text))
    }
}
fn status(locale: Locale, value: &str) -> String {
    locale
        .text(match value {
            "ready" => "resource_ready",
            "disabled" => "resource_disabled",
            "running" => "resource_running",
            "stopped" => "resource_stopped",
            "external" => "resource_external",
            "starting" => "resource_starting",
            "failed" => "resource_failed",
            "auth_required" => "resource_auth_required",
            "unprobed" => "resource_unprobed",
            _ => return value.into(),
        })
        .into()
}
