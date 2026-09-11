//! A hover owns only a read-only core subscription and its presentation.
use super::*;
#[cfg(feature = "headless-bench")]
use zork_client_core::state::HistoryData;
use zork_client_core::state::{ConversationTopics, SessionOverview};
use zork_ui::components::tooltip::DetailsTooltip;

/// One presentation for the dock; changing members replaces only its live source.
#[derive(Default)]
pub(super) struct Overlay {
    active: Option<Entity<Preview>>,
    anchor: gpui::Bounds<gpui::Pixels>,
    anchors: HashMap<String, gpui::Bounds<gpui::Pixels>>,
    revision: u64,
    observation: Option<gpui::Subscription>,
    trigger_hover: bool,
    panel_hover: bool,
    close: Option<Task<()>>,
}

impl Overlay {
    pub(super) fn show(
        &mut self,
        id: &str,
        anchor: gpui::Bounds<gpui::Pixels>,
        create: impl FnOnce(&mut gpui::App) -> Entity<Preview>,
        cx: &mut Context<Self>,
    ) {
        self.close.take();
        self.trigger_hover = true;
        self.panel_hover = false;
        self.anchor = anchor;
        if !self.is_target(id, cx) {
            let content = create(cx);
            self.observation = Some(cx.observe(&content, |v, _, cx| {
                v.revision = v.revision.wrapping_add(1);
                cx.notify();
            }));
            self.active = Some(content);
            self.revision = self.revision.wrapping_add(1);
        }
        cx.notify();
    }

    fn is_target(&self, id: &str, cx: &gpui::App) -> bool {
        self.active
            .as_ref()
            .is_some_and(|v| v.read(cx).member.id == id)
    }

    pub(super) fn anchor(
        &mut self,
        id: &str,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.anchors.insert(id.to_owned(), bounds) != Some(bounds) {
            if self.is_target(id, cx) {
                self.anchor = bounds;
            }
            if self.active.is_some() {
                cx.notify();
            }
        }
    }

    pub(super) fn retain(
        &mut self,
        ids: impl Iterator<Item = impl AsRef<str>>,
        cx: &mut Context<Self>,
    ) {
        let ids = ids
            .map(|id| id.as_ref().to_owned())
            .collect::<std::collections::HashSet<_>>();
        self.anchors.retain(|id, _| ids.contains(id));
        if let Some(active) = &self.active {
            let id = &active.read(cx).member.id;
            if !ids.contains(id) {
                self.active = None;
                self.observation = None;
                self.close = None;
                cx.notify();
            }
        }
    }

    pub(super) fn leave(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.is_target(id, cx) {
            self.trigger_hover = false;
            self.schedule_close(cx);
        }
    }

    fn schedule_close(&mut self, cx: &mut Context<Self>) {
        self.close.take();
        if self.trigger_hover || self.panel_hover {
            return;
        }
        self.close = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let _ = this.update(cx, |v, cx| {
                v.active = None;
                v.observation = None;
                v.close = None;
                cx.notify();
            });
        }));
    }
}

impl Render for Overlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(content) = self.active.clone() else {
            return gpui::Empty.into_any_element();
        };
        let mut anchor = self.anchor;
        // Active teammates form a vertical stack. Keep every avatar available
        // as a target instead of covering the next one with the floating card.
        for bounds in self.anchors.values() {
            anchor.origin.y = anchor.origin.y.min(bounds.top());
        }
        zork_ui::components::tooltip::sliding_popup(
            "composer-member-popup",
            format!("{}-{}", content.read(cx).member.id, self.revision),
            anchor,
            320.,
            move |_, cx| content.read(cx).content().into_any_element(),
        )
        .on_hover(cx.listener(|v, hovered, _, cx| {
            v.panel_hover = *hovered;
            v.schedule_close(cx);
        }))
        .into_any_element()
    }
}

pub(super) struct Preview {
    member: ParticipantStatus,
    locale: Locale,
    details: DetailsTooltip,
    state: Arc<SessionOverview>,
    _source: Option<Arc<zork_client_core::state::Conversation>>,
    _subscription: Option<Task<()>>,
    _activity: Option<Task<()>>,
}

impl Preview {
    #[cfg(feature = "headless-bench")]
    pub(super) fn fixture(
        member: ParticipantStatus,
        locale: Locale,
        state: Arc<HistoryData>,
    ) -> Self {
        let state = Arc::new(SessionOverview::fixture(&state));
        Self {
            details: details(&member, locale, &state),
            member,
            locale,
            state,
            _source: None,
            _subscription: None,
            _activity: None,
        }
    }
    pub(super) fn new(
        member: ParticipantStatus,
        locale: Locale,
        source: Option<Arc<zork_client_core::state::Conversation>>,
        conversation: Option<Arc<zork_client_core::state::Conversation>>,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut changes = source
            .as_ref()
            .map(|source| source.subscribe_topics(ConversationTopics::OVERVIEW));
        let state = changes
            .as_mut()
            .map(|changes| changes.snapshot().state.overview.clone())
            .unwrap_or_else(|| {
                Arc::new(SessionOverview {
                    loaded: true,
                    ..Default::default()
                })
            });
        let card = details(&member, locale, &state);
        let subscription = changes.map(|mut changes| {
            cx.spawn(async move |this, cx| {
                while let Some(update) = changes.changed().await {
                    if this
                        .update(cx, |v, cx| {
                            v.details = details(&v.member, v.locale, &update.state.overview);
                            v.state = update.state.overview.clone();
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        let activity = conversation.map(|conversation| {
            let mut changes = conversation
                .subscribe_topics(ConversationTopics::ACTIVITY | ConversationTopics::PARTICIPANTS);
            cx.spawn(async move |this, cx| {
                while let Some(update) = changes.changed().await {
                    if this
                        .update(cx, |v, cx| {
                            let previous = v.member.clone();
                            if let Some(member) = update
                                .state
                                .participants
                                .iter()
                                .find(|m| m.id == v.member.id)
                            {
                                v.member = member.clone();
                            } else if is_selected {
                                v.member.activity = update.state.activity.clone();
                            }
                            if previous != v.member {
                                v.details = details(&v.member, v.locale, &v.state);
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        if let Some(source) = &source {
            // The normal session stream supplies snapshot + future updates.
            // A hover never creates a history controller or requests its pages.
            source.start();
        }
        Self {
            member,
            locale,
            details: card,
            state,
            _source: source,
            _subscription: subscription,
            _activity: activity,
        }
    }
    fn content(&self) -> impl IntoElement {
        div()
            .id(format!("detail-tooltip-{}", self.details.key))
            .w_full()
            .p(px(16.))
            .text_color(rgb(TEXT))
            .child(self.details.content())
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(10.))
                    .text_color(rgb(DIM))
                    .child(self.locale.text("presence_history_hint")),
            )
            .automation(
                AutomationRole::Status,
                format!("{}详情：{}", self.details.kind, self.details.title),
            )
    }
}

fn details(member: &ParticipantStatus, locale: Locale, state: &SessionOverview) -> DetailsTooltip {
    let mut rows = Vec::with_capacity(5);
    for (index, entry) in state.highlights().enumerate() {
        let projection = zork_ui::history::activity::Projection::new(std::slice::from_ref(&entry));
        let action = projection
            .activities
            .first()
            .filter(|a| a.kind != zork_ui::history::activity::Kind::UnknownTool)
            .map(|a| {
                locale
                    .text(zork_ui::components::history::kind_label(a.kind))
                    .to_owned()
            })
            .unwrap_or_else(|| zork_ui::history::activity::preview(&entry.action));
        let status = locale.text(match entry.state.as_str() {
            "running" => "history_running",
            "succeeded" => "history_success",
            "cancelled" | "interrupted" => "history_cancelled",
            "failed" | "timed_out" => "history_error",
            _ => "history_notice",
        });
        // Only explicit failure detail is useful here. Successful stdout and
        // model reasoning belong in the full history, not a hover card.
        let error = matches!(entry.state.as_str(), "failed" | "timed_out")
            .then(|| entry.outcome_summary.as_deref().unwrap_or(&entry.summary))
            .filter(|text| !text.trim().is_empty())
            .map(zork_ui::history::activity::preview);
        let time = entry
            .end
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| {
                format!(
                    " · {}",
                    at.with_timezone(&chrono::Local).format("%m-%d %H:%M")
                )
            })
            .unwrap_or_default();
        rows.push((
            locale
                .text(if index == 0 {
                    "presence_recent"
                } else {
                    "presence_previous"
                })
                .into(),
            match error {
                Some(error) => format!("{action} · {status}{time} — {error}"),
                None => format!("{action} · {status}{time}"),
            },
        ));
    }
    if rows.is_empty() || state.error.is_some() {
        rows.push((
            locale.text("presence_recent").into(),
            locale
                .text(
                    if state.error.is_some() || (state.loaded && !state.aggregates.complete) {
                        "presence_history_unavailable"
                    } else if state.loaded {
                        "history_no_activity"
                    } else {
                        "presence_history_loading"
                    },
                )
                .into(),
        ));
    }
    if let Some(runtime) = &state.runtime {
        if let Some(model) = &runtime.model {
            rows.push((locale.text("model").into(), model.clone()));
        }
        if let (Some(used), Some(limit)) = (runtime.context_tokens, runtime.context_limit) {
            if limit > 0 {
                rows.push((
                    locale.text("presence_context").into(),
                    format!("{used} / {limit} tokens"),
                ));
            }
        }
    }
    rows.push((
        locale.text("chat_receiving").into(),
        locale
            .text(if member.subscribed {
                "chat_subscribed"
            } else {
                "chat_unsubscribed"
            })
            .into(),
    ));
    DetailsTooltip {
        key: format!("presence-{}", member.id),
        title: member.name.clone(),
        kind: locale.text("presence_member").into(),
        avatar: Some(member.avatar.clone().unwrap_or_else(|| "cat".into())),
        description: member
            .activity
            .as_ref()
            .filter(|s| should_render_live_activity(s, None))
            .map(|s| agent_status_label(s, locale))
            .unwrap_or_else(|| locale.text("presence_idle").into()),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_keeps_failures_but_omits_success_output_and_internal_model_text() {
        let state = SessionOverview {
            loaded: true,
            aggregates: serde_json::from_value(serde_json::json!({
                "complete":true,"usage":{"input":0,"output":0,"cached":0,"reported_steps":0,"cache_reported_steps":0,"cache_input":0},
                "recent":[
                    {"id":"tool:b","lane":2,"tool":"read_file","action":"Read file","state":"failed","finished_at_ms":2,"error":"File missing"},
                    {"id":"tool:a","lane":2,"tool":"exec","action":"Execute command","state":"succeeded","finished_at_ms":1,"error":null}
                ]
            })).unwrap(),
            ..Default::default()
        };
        let member = ParticipantStatus {
            subscribed: false,
            id: "a".into(),
            name: "Atlas".into(),
            avatar: None,
            session_id: "session-a".into(),
            activity: None,
        };
        for locale in Locale::ALL {
            let card = details(&member, locale, &state);
            assert_eq!(card.title, "Atlas");
            assert!(card.rows[0].1.contains("File missing"));
            let text = format!("{:?}", card.rows);
            assert!(!text.contains("PRIVATE OUTPUT"));
            assert!(!text.contains("INTERNAL REASONING"));
        }
    }
}
