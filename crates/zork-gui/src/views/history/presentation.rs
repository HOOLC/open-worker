//! Native composition of the shared reading-oriented history rows.
use super::*;
use zork_ui::components::history::{activity_color, kind_icon, kind_label, ActivityHeader};

#[derive(Clone)]
enum Jump {
    Conversation(String),
    Agent(String),
    Entry(String),
}

impl HistoryState {
    pub(super) fn row_entry(&self, row: usize) -> Option<&Entry> {
        let row = self.rows.get(row)?;
        let activity = row
            .activity
            .unwrap_or(self.projection.blocks[row.block].start);
        self.entries.get(self.projection.activities[activity].entry)
    }

    pub(super) fn row_for_id(&self, id: &str) -> Option<usize> {
        let entry = self.entries.iter().position(|e| e.id == id)?;
        let block = self
            .projection
            .entry_to_block
            .get(entry)
            .copied()
            .flatten()?;
        self.rows
            .iter()
            .position(|row| {
                row.block == block
                    && row
                        .activity
                        .is_some_and(|a| self.projection.activities[a].entry == entry)
            })
            .or_else(|| self.rows.iter().position(|row| row.block == block))
    }

    pub(super) fn rebuild_rows(&mut self) {
        let previous = self.rows.len();
        self.rows = self.projection.rows(self.entries.iter(), &self.expanded);
        self.scroll.splice(1..previous + 1, self.rows.len());
    }
}

impl RootView {
    pub(in crate::views) fn refresh_history_sources(&mut self) {
        if !self.history.open {
            return;
        }
        let wanted: std::collections::HashSet<&str> = self
            .history
            .entries
            .iter()
            .filter_map(|e| activity::input(e)?["request_id"].as_str())
            .collect();
        self.history.wanted_sources = wanted.iter().map(|id| (*id).to_owned()).collect();
        let mut sources = HashMap::new();
        if !wanted.is_empty() {
            let mut add = |line: &TranscriptLine| {
                let TranscriptLine::Message { role, metadata, .. } = line;
                let Some(id) = metadata.id.as_deref().filter(|id| wanted.contains(id)) else {
                    return;
                };
                let label = metadata
                    .author_name
                    .clone()
                    .or_else(|| metadata.author_agent_id.clone())
                    .unwrap_or_else(|| {
                        self.locale
                            .text(if *role == Role::User {
                                "history_user"
                            } else {
                                "history_source_unknown"
                            })
                            .into()
                    });
                sources.insert(id.to_owned(), (label, metadata.author_agent_id.clone()));
            };
            if self.transcript_lookup.len() == self.lines.len() {
                for id in &wanted {
                    if let Some(line) = self
                        .transcript_lookup
                        .index_of(id)
                        .and_then(|index| self.lines.get(index))
                    {
                        add(line);
                    }
                }
            } else {
                // Unbound preview fixtures have no core-built lookup.
                for line in self.lines.iter() {
                    add(line);
                }
            }
        }
        self.history.sources = sources;
    }

    pub(in crate::views) fn refresh_changed_message_sources(
        &mut self,
        edits: &[zork_client_core::observe::ListEdit<TranscriptLine>],
    ) {
        if !self.history.open {
            return;
        }
        let mut previous = self.lines.clone();
        for edit in edits {
            for line in previous.slice(edit.remove.clone()).iter() {
                let TranscriptLine::Message { metadata, .. } = line;
                if let Some(id) = &metadata.id {
                    self.history.sources.remove(id);
                }
            }
            for line in edit.insert.iter() {
                let TranscriptLine::Message { role, metadata, .. } = line;
                let Some(id) = metadata
                    .id
                    .as_ref()
                    .filter(|id| self.history.wanted_sources.contains(*id))
                else {
                    continue;
                };
                let label = metadata
                    .author_name
                    .clone()
                    .or_else(|| metadata.author_agent_id.clone())
                    .unwrap_or_else(|| {
                        self.locale
                            .text(if *role == Role::User {
                                "history_user"
                            } else {
                                "history_source_unknown"
                            })
                            .into()
                    });
                self.history
                    .sources
                    .insert(id.clone(), (label, metadata.author_agent_id.clone()));
            }
            edit.apply(&mut previous);
        }
    }

    fn activity_subject(&self, a: &Activity, e: &Entry) -> (Option<String>, Option<Jump>) {
        if a.kind == Kind::Received && a.subject.is_none() {
            let receipt = activity::input(e).and_then(|input| input["request_id"].as_str());
            if receipt.is_some_and(|id| id.starts_with("assignment-") || id.starts_with("rework-"))
            {
                let session = self
                    .history
                    .session
                    .as_ref()
                    .or(self.selected_session.as_ref());
                if let Some((leader, _)) = self.tasks_by_leader.iter().find(|(_, tasks)| {
                    session.is_some_and(|session| {
                        tasks.iter().any(|t| t.session_id.as_ref() == Some(session))
                    })
                }) {
                    let agent = self.node_agents.iter().find(|a| a["id"] == *leader);
                    return (
                        Some(
                            agent
                                .and_then(|a| a["name"].as_str())
                                .unwrap_or(leader)
                                .to_owned(),
                        ),
                        agent.map(|_| Jump::Agent(leader.clone())),
                    );
                }
            }
            if let Some((label, agent)) = activity::input(e)
                .and_then(|input| input["request_id"].as_str())
                .and_then(|id| self.history.sources.get(id))
            {
                return (
                    Some(label.clone()),
                    agent
                        .as_ref()
                        .map(|id| Jump::Agent(id.clone()))
                        .or_else(|| {
                            self.history
                                .session
                                .as_ref()
                                .or(self.selected_session.as_ref())
                                .cloned()
                                .map(Jump::Conversation)
                        }),
                );
            }
            return (
                Some(self.locale.text("history_source_unknown").into()),
                None,
            );
        }
        match &a.subject {
            Some(Subject::User) => (Some(self.locale.text("history_user").into()), None),
            Some(Subject::Conversation) => {
                let session = self
                    .history
                    .session
                    .as_ref()
                    .or(self.selected_session.as_ref());
                let title = session
                    .and_then(|id| self.sessions.iter().find(|s| &s.session_id == id))
                    .and_then(|s| s.task.as_ref())
                    .map(|t| t.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| self.locale.text("history_current_conversation").into());
                (Some(title), session.cloned().map(Jump::Conversation))
            }
            Some(Subject::Agent(id)) => {
                let agent = self.node_agents.iter().find(|agent| agent["id"] == *id);
                let participant = self.participants.iter().find(|p| &p.id == id);
                let name = agent
                    .and_then(|a| a["name"].as_str())
                    .map(str::to_owned)
                    .or_else(|| participant.map(|p| p.name.clone()))
                    .unwrap_or_else(|| id.clone());
                (
                    Some(name),
                    (agent.is_some() || participant.is_some()).then(|| Jump::Agent(id.clone())),
                )
            }
            Some(Subject::Task(id)) => {
                let task = self
                    .tasks_by_leader
                    .values()
                    .flatten()
                    .find(|t| &t.task_id == id);
                (
                    Some(task.map_or_else(|| id.clone(), |t| t.title.clone())),
                    task.and_then(|t| t.session_id.clone())
                        .map(Jump::Conversation),
                )
            }
            Some(Subject::Invocation(id)) => {
                let key = format!("tool:{id}");
                let entry = self.history.entries.iter().find(|e| e.id == key);
                (
                    Some(entry.map_or_else(|| id.clone(), |e| e.action.clone())),
                    entry.map(|e| Jump::Entry(e.id.clone())),
                )
            }
            Some(Subject::Slack { channel, thread }) => (
                Some(format!(
                    "{} / {}",
                    activity::preview(channel),
                    activity::preview(thread)
                )),
                None,
            ),
            Some(
                Subject::Source(label)
                | Subject::File(label)
                | Subject::Tool(label)
                | Subject::BrowserTab(label),
            ) => (Some(activity::preview(label)), None),
            None => (None, None),
        }
    }

    fn jump_history_target(&mut self, jump: Jump, cx: &mut Context<Self>) {
        match jump {
            Jump::Conversation(session) => {
                self.history.detail = None;
                self.history.agent_detail = None;
                if self.selected_session.as_ref() == Some(&session) {
                    self.browser
                        .update(cx, |panel, cx| panel.close_native_page("history", cx));
                } else {
                    self.select_session(&session, cx);
                }
            }
            Jump::Agent(id) => {
                self.history.detail = None;
                self.history.agent_detail = Some(id);
            }
            Jump::Entry(id) => {
                let index = self.history.entries.iter().position(|e| e.id == id);
                if let Some(index) = index {
                    self.history_select(index, cx);
                    self.history.detail = Some(id);
                }
            }
        }
        zork_ui::components::region::invalidate_all(cx);
    }

    pub(super) fn render_history_activity(
        &self,
        index: usize,
        now: i64,
        selection_range: Option<(i64, i64)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        #[cfg(feature = "headless-bench")]
        self.benchmark_rows.set(self.benchmark_rows.get() + 1);
        let row = self.history.rows[index];
        let block = &self.history.projection.blocks[row.block];
        let a_index = row.activity.unwrap_or(block.start);
        let a = &self.history.projection.activities[a_index];
        let entry = &self.history.entries[a.entry];
        if a.kind == Kind::End && entry.state == "succeeded" {
            return div()
                .id(("history-row", index))
                .px(px(12.))
                .py(px(6.))
                .child(div().h(px(1.)).w_full().bg(rgba(0x80808030)));
        }
        let first_entry =
            &self.history.entries[self.history.projection.activities[block.start].entry];
        let group_id = first_entry.id.clone();
        let group = row.activity.is_none();
        let (subject, jump) = if group {
            (None, None)
        } else {
            self.activity_subject(a, entry)
        };
        let action = if group {
            let counts = &block.counts;
            [
                (counts.read, "history_read_count"),
                (counts.written, "history_write_count"),
                (counts.shell, "history_shell_count"),
                (counts.queries, "history_query_count"),
            ]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, key)| self.locale.text(key).replace("{count}", &count.to_string()))
            .collect::<Vec<_>>()
            .join(" · ")
        } else {
            let mut label = if a.kind == Kind::UnknownTool {
                activity::preview(&entry.action)
            } else {
                self.locale.text(kind_label(a.kind)).to_owned()
            };
            if a.kind == Kind::Wait {
                if let Some(duration) = entry.duration(now) {
                    label.push(' ');
                    label.push_str(&model::duration(duration));
                }
            }
            label
        };
        let connector = (!group
            && subject.is_some()
            && subject.as_deref() != Some(self.locale.text("history_source_unknown"))
            && matches!(
                a.kind,
                Kind::Received
                    | Kind::SendMessage
                    | Kind::SendFile
                    | Kind::Notify
                    | Kind::Assign
                    | Kind::Rework
            ))
        .then(|| {
            self.locale
                .text(if a.kind == Kind::Received {
                    "history_from"
                } else {
                    "history_to"
                })
                .to_owned()
        });
        let mut summary = if group {
            block.summary.clone()
        } else {
            a.summary.clone()
        };
        if !group && subject.as_ref() == Some(&summary) {
            summary.clear();
        }
        if a.kind == Kind::Wait && summary.is_empty() {
            if let Some(ms) = a.requested_wait_ms {
                summary = self
                    .locale
                    .text("history_wait_requested")
                    .replace("{duration}", &model::duration(ms));
            }
        }
        let status = if group {
            None
        } else {
            match entry.state.as_str() {
                "running" => Some(
                    self.locale
                        .text(if a.kind == Kind::Wait {
                            "history_waiting"
                        } else {
                            "history_running"
                        })
                        .into(),
                ),
                "failed" | "timed_out" => Some(self.locale.text("history_error").into()),
                "cancelled" | "interrupted" => Some(self.locale.text("history_cancelled").into()),
                "succeeded"
                    if matches!(a.kind, Kind::SendMessage | Kind::SendFile | Kind::Notify) =>
                {
                    Some(self.locale.text("history_sent").into())
                }
                _ => None,
            }
        };
        let first = if group { first_entry } else { entry };
        let outside = selection_range.is_some_and(|(lo, hi)| {
            let start = if group {
                block.start_at
            } else {
                entry.start.or(entry.end)
            };
            let end = if group {
                block
                    .end_at
                    .map(|end| if block.running { end.max(now) } else { end })
            } else {
                entry
                    .end
                    .or_else(|| (entry.state == "running").then_some(now))
                    .or(entry.start)
            };
            end.is_none_or(|end| end < lo) || start.is_none_or(|start| start > hi)
        });
        let id = entry.id.clone();
        let selected = self.history.selected.as_ref() == Some(&id);
        div()
            .id(("history-row", index))
            .when(selected, |v| v.bg(rgb(CUE_UI.palette.sidebar_hover)))
            .when(outside, |v| v.opacity(0.3))
            .child(zork_ui::components::history::activity_header(
                ("history-record", index),
                ActivityHeader {
                    icon: if group {
                        "history/operations.svg"
                    } else {
                        kind_icon(a.kind)
                    },
                    color: if group {
                        DIM
                    } else {
                        activity_color(a.kind, &entry.state)
                    },
                    action,
                    connector,
                    subject,
                    clickable_subject: jump.is_some(),
                    summary,
                    time: relative_time(first.start.or(first.end), now, self.locale),
                    status,
                    nested: row.activity.is_some() && block.is_group(),
                    group,
                },
                cx,
                move |v, _, cx| {
                    if group {
                        if !v.history.expanded.remove(&group_id) {
                            v.history.expanded.insert(group_id.clone());
                        }
                        v.history.rebuild_rows();
                        if let Some(row) = v.history.row_for_id(&group_id) {
                            // Keep the group heading visible rather than jumping to its last child.
                            let summary = v.history.rows[..=row]
                                .iter()
                                .rposition(|r| r.activity.is_none())
                                .unwrap_or(row);
                            v.history.scroll.scroll_to_reveal_item(summary + 1);
                        }
                    } else {
                        v.history.detail = Some(id.clone());
                        v.history.agent_detail = None;
                        v.history.selected = Some(id.clone());
                    }
                    zork_ui::components::region::invalidate_all(cx);
                },
                move |v, _, cx| {
                    if let Some(jump) = jump.clone() {
                        v.jump_history_target(jump, cx);
                    }
                },
            ))
    }

    pub(in crate::views) fn render_history_detail_modal(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let body = if let Some(agent_id) = &self.history.agent_detail {
            let agent = self.node_agents.iter().find(|a| a["id"] == *agent_id);
            let participant = self.participants.iter().find(|p| p.id == *agent_id);
            let name = agent
                .and_then(|a| a["name"].as_str())
                .or_else(|| participant.map(|p| p.name.as_str()))
                .unwrap_or(agent_id);
            let avatar = agent
                .and_then(|a| a["avatar"].as_str())
                .or_else(|| participant.and_then(|p| p.avatar.as_deref()));
            div()
                .p_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(ui::agent_avatar(avatar, 32.))
                        .child(div().text_size(px(16.)).child(name.to_owned())),
                )
                .child(
                    div()
                        .mt_3()
                        .text_size(px(12.))
                        .text_color(rgb(DIM))
                        .child(agent_id.clone()),
                )
                .when_some(agent.and_then(|a| a["role"].as_str()), |v, role| {
                    v.child(div().mt_2().child(role.to_owned()))
                })
                .into_any_element()
        } else if let Some(entry) = self
            .history
            .detail
            .as_ref()
            .and_then(|id| self.history.entries.iter().find(|e| &e.id == id))
        {
            div()
                .id("history-detail-scroll")
                .max_h(px((window.viewport_size().height.as_f32() - 180.).max(120.)))
                .overflow_y_scroll()
                .p_3()
                .text_size(px(12.))
                .line_height(px(18.))
                .child(div().mb_3().child(entry.summary.clone()))
                .when_some(
                    zork_client_core::resources::history_target(entry),
                    |body, target| {
                        let label = self.locale.text(
                            if matches!(
                                target.query,
                                zork_client_core::resources::Inspection::Mcp(_)
                            ) {
                                "tool_connections"
                            } else {
                                "device_services"
                            },
                        );
                        body.child(
                            ui::button("history-resource-details", label, false, true)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.history.detail = None;
                                    cx.emit(crate::views::InspectResource(target.clone()));
                                    zork_ui::components::region::invalidate_all(cx);
                                }))
                                .automation(AutomationRole::Button, label),
                        )
                    },
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(DIM))
                        .child(self.locale.text("history_raw")),
                )
                .children(entry.raw.iter().enumerate().map(|(n, value)| {
                    self.render_history_json(value, format!("{}:{n}", entry.id), 0, None, cx)
                }))
                .into_any_element()
        } else {
            div()
                .child(self.locale.text("history_empty"))
                .into_any_element()
        };
        zork_ui::modal::detail_modal(
            "history-detail-dialog",
            self.locale.text(if self.history.agent_detail.is_some() {
                "history_agent_details"
            } else {
                "history_summary_details"
            }),
            body,
            None,
            &self.history_modal.focus,
            window,
            cx,
            true,
            |v, _, cx| {
                v.history.detail = None;
                v.history.agent_detail = None;
                zork_ui::components::region::invalidate_all(cx);
            },
        )
    }
}

fn relative_time(timestamp: Option<i64>, now: i64, locale: Locale) -> String {
    let Some(time) = timestamp else {
        return locale.text("history_unknown_time").into();
    };
    let age = now.saturating_sub(time);
    if age < 0 {
        return locale.text("history_clock_ahead").into();
    }
    let seconds = age / 1000;
    let (key, value) = match seconds {
        0..=4 => return locale.text("history_relative_just_now").into(),
        5..=59 => ("history_relative_seconds", seconds),
        60..=3599 => ("history_relative_minutes", seconds / 60),
        3600..=86399 => ("history_relative_hours", seconds / 3600),
        _ => ("history_relative_days", seconds / 86400),
    };
    locale.text(key).replace("{count}", &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_time_handles_unknown_future_and_unit_boundaries() {
        assert_eq!(relative_time(None, 1000, Locale::ZhCn), "时间未知");
        assert_eq!(relative_time(Some(1001), 1000, Locale::En), "Clock ahead");
        assert_eq!(relative_time(Some(0), 59000, Locale::ZhCn), "59 秒前");
        assert_eq!(relative_time(Some(0), 60000, Locale::ZhCn), "1 分钟前");
        assert_eq!(relative_time(Some(0), 3600000, Locale::En), "1h ago");
    }
}
