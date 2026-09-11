use super::*;

fn token_count(value: u64) -> String {
    let (number, suffix) = if value >= 1_000_000 {
        (format!("{:.2}", value as f64 / 1_000_000.), "M")
    } else if value >= 10_000 {
        (format!("{:.1}", value as f64 / 1_000.), "k")
    } else {
        return value.to_string();
    };
    format!(
        "{}{suffix}",
        number.trim_end_matches('0').trim_end_matches('.')
    )
}

impl RootView {
    pub(super) fn render_history_statistics(&mut self) -> impl IntoElement {
        if self
            .history
            .quota
            .as_ref()
            .is_none_or(|(locale, _)| *locale != self.locale)
        {
            self.history.quota = self
                .history
                .runtime
                .as_ref()
                .and_then(|r| r.profile.as_ref())
                .map(|profile| {
                    let mut quota =
                        crate::desktop::profile_quota::QuotaPresentation::new(profile, self.locale);
                    quota.checked = profile
                        .checked_at
                        .as_deref()
                        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                        .map(|date| {
                            self.locale.text("history_quota_checked").replace(
                                "{time}",
                                &date
                                    .with_timezone(&chrono::Local)
                                    .format("%m-%d %H:%M")
                                    .to_string(),
                            )
                        });
                    (self.locale, quota)
                });
        }
        let usage = &self.history.usage;
        let complete = self.history.usage_complete;
        let cache_rate = complete.then(|| usage.cache_hit_rate()).flatten();
        let count = |value| {
            if !complete || usage.reported_steps == 0 {
                "—".to_owned()
            } else {
                token_count(value)
            }
        };
        let metric = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(3.))
                .text_size(px(10.))
                .line_height(px(14.))
                .child(div().text_color(rgb(DIM)).child(self.locale.text(label)))
                .child(div().text_color(rgb(TEXT)).child(value))
        };
        div()
            .id("history-usage-overview")
            .max_h(relative(0.34))
            .overflow_y_scroll()
            .line_height(px(14.))
            .mx(px(8.))
            .mt(px(4.))
            .mb(px(4.))
            .p(px(8.))
            .flex_shrink_0()
            .rounded(px(8.))
            .bg(rgb(CUE_UI.palette.sidebar))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(self.render_history_runtime())
            .child(
                div()
                    .id("history-token-totals")
                    .w_full()
                    .overflow_x_scroll()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(4.))
                    .child(metric("history_input_tokens", count(usage.input)))
                    .child(metric(
                        "history_uncached_input",
                        if complete
                            && usage.reported_steps > 0
                            && usage.cache_reported_steps == usage.reported_steps
                        {
                            token_count(usage.input.saturating_sub(usage.cached))
                        } else {
                            "—".into()
                        },
                    ))
                    .child(metric(
                        "history_cache_rate",
                        cache_rate
                            .map(|rate| format!("{:.1}%", rate * 100.))
                            .unwrap_or_else(|| "—".into()),
                    ))
                    .child(metric("history_output_tokens", count(usage.output)))
                    .child(metric(
                        "history_total_tokens",
                        count(usage.input.saturating_add(usage.output)),
                    )),
            )
            .when(
                complete
                    && usage.cache_reported_steps > 0
                    && usage.cache_reported_steps < usage.reported_steps,
                |v| {
                    v.child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(DIM))
                            .child(self.locale.text("history_cache_partial")),
                    )
                },
            )
            .when(self.history.overview_loaded && !complete, |view| {
                view.child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(DIM))
                        .child(self.locale.text("history_usage_incomplete")),
                )
            })
            .automation(
                AutomationRole::Status,
                format!(
                    "{}: {} · {}: {}",
                    self.locale.text("history_total_tokens"),
                    if !complete || usage.reported_steps == 0 {
                        "—".into()
                    } else {
                        usage.input.saturating_add(usage.output).to_string()
                    },
                    self.locale.text("history_cache_rate"),
                    cache_rate
                        .map(|rate| format!("{:.1}%", rate * 100.))
                        .unwrap_or_else(|| "—".into())
                ),
            )
    }
    fn render_history_runtime(&self) -> impl IntoElement {
        let runtime = self.history.runtime.as_ref();
        let model = runtime.and_then(|r| r.model.as_deref()).unwrap_or("—");
        let context = runtime.and_then(|r| r.context_tokens);
        let limit = runtime.and_then(|r| r.context_limit);
        let quota = self.history.quota.as_ref().map(|(_, quota)| quota);
        let field = |value: String| div().flex_shrink_0().child(value);
        div()
            .id("history-runtime")
            .w_full()
            .overflow_x_scroll()
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(10.))
            .line_height(px(14.))
            .child(field(
                runtime
                    .and_then(|r| r.profile.as_ref())
                    .map(|p| p.display_name().to_owned())
                    .unwrap_or_else(|| "—".into()),
            ))
            .child(field(model.to_owned()).font_weight(FontWeight::MEDIUM))
            .child(
                field(
                    runtime
                        .and_then(|r| r.thinking.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "—".into()),
                )
                .text_color(rgb(DIM)),
            )
            .child(field(format!(
                "{} / {}",
                context.map(token_count).unwrap_or_else(|| "—".into()),
                limit.map(token_count).unwrap_or_else(|| "—".into())
            )))
            .when_some(quota.filter(|q| q.visible()), |v, quota| {
                v.children(
                    quota
                        .windows
                        .iter()
                        .map(|window| field(format!("{} {}", window.label, window.value))),
                )
                .when_some(quota.balance.clone(), |v, balance| v.child(field(balance)))
                .when(quota.failed, |v| {
                    v.child(field(quota.summary.clone()).text_color(rgb(DIM)))
                })
            })
            .automation(
                AutomationRole::Status,
                format!(
                    "{} · {} / {}",
                    model,
                    context.map(|v| v.to_string()).unwrap_or_else(|| "—".into()),
                    limit.map(|v| v.to_string()).unwrap_or_else(|| "—".into())
                ),
            )
    }
}
