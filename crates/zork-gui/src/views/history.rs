//! Native port of Cue's participant session history page (e9a817c0c).
use super::*;
use crate::desktop::ui;
use crate::session_history::{self as model, Entry, Record};
use gpui::{relative, ListAlignment, ListOffset, ListState};
use zork_ui::history::activity::{self, Activity, Kind, Projection, Row, Subject};

mod live;
mod presentation;
mod statistics;
use model::usage::UsageSummary;

pub(super) struct HistoryState {
    #[cfg(feature = "headless-bench")]
    fixed_now: Option<i64>,
    pub open: bool,
    session: Option<String>,
    records: zork_client_core::observe::List<Record>,
    entries: zork_client_core::observe::List<Entry>,
    projection: Projection,
    usage: UsageSummary,
    usage_complete: bool,
    overview_loaded: bool,
    runtime: Option<zork_client_core::state::HistoryRuntime>,
    quota: Option<(Locale, crate::desktop::profile_quota::QuotaPresentation)>,
    rows: Vec<Row>,
    pub(super) detail: Option<String>,
    pub(super) agent_detail: Option<String>,
    source: Option<Arc<zork_client_core::state::Conversation>>,
    sources: HashMap<String, (String, Option<String>)>,
    wanted_sources: std::collections::HashSet<String>,
    pub(super) subscription: Option<Task<()>>,
    updates: Option<zork_client_core::state::HistorySubscription>,
    overview_subscription: Option<Task<()>>,
    overview_updates: Option<zork_client_core::state::ConversationSubscription>,
    older: Option<String>,
    latest: Option<String>,
    busy: bool,
    loading_older: bool,
    loaded: bool,
    error: Option<String>,
    selected: Option<String>,
    hovered: Option<String>,
    hover_anchor: gpui::Bounds<gpui::Pixels>,
    hover_close: Option<Task<()>>,
    expanded: std::collections::HashSet<String>,
    json_open: std::collections::HashSet<String>,
    pub rendered_width: f32,
    scroll: ListState,
    scroll_observed: bool,
    zoom: f64,
    pan: f64,
    clock_offset: i64,
    range: Option<(f64, f64)>,
    dragging: bool,
    bounds: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}
impl Default for HistoryState {
    fn default() -> Self {
        Self {
            #[cfg(feature = "headless-bench")]
            fixed_now: None,
            open: false,
            session: None,
            records: Default::default(),
            entries: Default::default(),
            projection: Projection::default(),
            usage: UsageSummary::default(),
            usage_complete: false,
            overview_loaded: false,
            runtime: None,
            quota: None,
            rows: vec![],
            detail: None,
            agent_detail: None,
            source: None,
            sources: HashMap::new(),
            wanted_sources: Default::default(),
            subscription: None,
            updates: None,
            overview_subscription: None,
            overview_updates: None,
            older: None,
            latest: None,
            busy: false,
            loading_older: false,
            loaded: false,
            error: None,
            selected: None,
            hovered: None,
            hover_anchor: Default::default(),
            hover_close: None,
            expanded: Default::default(),
            json_open: Default::default(),
            rendered_width: 440.,
            scroll: ListState::new(1, ListAlignment::Top, px(200.)),
            scroll_observed: false,
            zoom: 1.,
            pan: 0.,
            clock_offset: 0,
            range: None,
            dragging: false,
            bounds: Rc::new(std::cell::Cell::new(Default::default())),
        }
    }
}
impl HistoryState {
    fn now(&self) -> i64 {
        #[cfg(feature = "headless-bench")]
        if let Some(now) = self.fixed_now {
            return now;
        }
        model::now() + self.clock_offset
    }

    #[cfg(feature = "headless-bench")]
    pub(super) fn benchmark_offset(&self) -> ListOffset {
        self.scroll.logical_scroll_top()
    }

    #[cfg(feature = "headless-bench")]
    pub(super) fn story(records: Vec<Record>, now: i64) -> Self {
        let entries = model::entries(&records);
        let projection = Projection::new(&entries);
        let rows = projection.rows(&entries, &Default::default());
        let scroll = ListState::new(rows.len() + 1, ListAlignment::Top, px(200.));
        Self {
            open: true,
            session: Some("render-fixture".into()),
            records: records.into(),
            usage: UsageSummary::new(&entries),
            usage_complete: true,
            overview_loaded: true,
            entries: entries.into(),
            projection,
            rows,
            loaded: true,
            scroll,
            fixed_now: Some(now),
            ..Self::default()
        }
    }
    #[cfg(feature = "headless-bench")]
    pub(super) fn benchmark(records: Vec<Record>, now: i64) -> Self {
        let entries = model::entries(&records);
        let projection = Projection::new(&entries);
        let rows = projection.rows(&entries, &Default::default());
        let scroll = ListState::new(rows.len() + 1, ListAlignment::Top, px(200.));
        scroll.scroll_to(ListOffset {
            item_ix: 100,
            offset_in_item: px(0.),
        });
        Self {
            open: true,
            session: Some("render-fixture".into()),
            records: records.into(),
            usage: UsageSummary::new(&entries),
            usage_complete: true,
            overview_loaded: true,
            entries: entries.into(),
            projection,
            rows,
            loaded: true,
            scroll,
            fixed_now: Some(now),
            ..Self::default()
        }
    }
}
impl RootView {
    pub(super) fn save_chat_history(&mut self) {
        // Inactive chats retain their presentation, not a live subscription or clock.
        self.history.subscription = None;
        self.history.updates = None;
        self.history.overview_subscription = None;
        self.history.overview_updates = None;
        self.history.source = None;
        self.history.hovered = None;
        self.history.hover_close = None;
        self.chat_histories
            .insert(self.browser_host(), std::mem::take(&mut self.history));
    }
    pub(super) fn restore_chat_history(&mut self, cx: &mut Context<Self>) {
        let host = self.browser_host();
        self.history = self.chat_histories.remove(&host).unwrap_or_default();
        self.browser
            .update(cx, |panel, cx| panel.set_host(host, cx));
        if self.history.open {
            self.load_history(false, cx);
            self.history.scroll.scroll_to_end();
        }
    }
    pub(super) fn reset_history(&mut self) {
        self.history = HistoryState::default();
    }
    pub(super) fn close_history(&mut self) {
        self.history.open = false;
        self.history.subscription = None;
        self.history.updates = None;
        self.history.overview_subscription = None;
        self.history.overview_updates = None;
        self.history.source = None;
        self.history.hovered = None;
        self.history.hover_close = None;
        self.history.detail = None;
        self.history.agent_detail = None;
    }
    pub(super) fn toggle_history(&mut self, session: &str, cx: &mut Context<Self>) {
        if session.is_empty() {
            return;
        }
        if self.history.open && self.history.session.as_deref() == Some(session) {
            self.close_history();
            self.browser
                .update(cx, |panel, cx| panel.close_native_page("history", cx));
            zork_ui::components::region::invalidate_all(cx);
        } else {
            self.open_history(session, cx);
        }
    }
    pub(super) fn open_history(&mut self, session: &str, cx: &mut Context<Self>) {
        if self.history.session.as_deref() != Some(session) {
            self.reset_history();
            self.history.session = Some(session.into());
        }
        self.history.open = true;
        self.open_history_tab(cx);
        self.load_history(false, cx);
        self.history.scroll.scroll_to_end();
        zork_ui::components::region::invalidate(cx, &["history", "header"]);
    }
    pub(super) fn open_history_tab(&mut self, cx: &mut Context<Self>) {
        let page = crate::browser::NativePage {
            id: "history".into(),
            title: self.history_name().into(),
            icon: "icons/history.svg",
        };
        self.browser
            .update(cx, |panel, cx| panel.open_native_page(page, cx));
        zork_ui::components::region::invalidate_all(cx);
    }
    fn load_history(&mut self, older: bool, cx: &mut Context<Self>) {
        let Some(core) = self.observe_history(cx) else {
            return;
        };
        let source = self.history.source.as_ref().unwrap();
        #[cfg(not(feature = "headless-bench"))]
        source.start();
        #[cfg(feature = "headless-bench")]
        if !self.benchmark_offline {
            source.start();
        }
        core.load(older);
    }

    fn observe_history(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<Arc<zork_client_core::state::History>> {
        let sid = self.history.session.clone()?;
        let source = self.core_device.conversation(&sid);
        let core = source.history();
        if self.history.overview_subscription.is_none() {
            let mut changes =
                source.subscribe_topics(zork_client_core::state::ConversationTopics::OVERVIEW);
            if let Some(update) = changes.prepare() {
                self.apply_history_overview(&update.state.overview, cx);
                changes.acknowledge(update.batch.unwrap());
            }
            let mut readiness = changes.readiness();
            self.history.overview_updates = Some(changes);
            self.history.overview_subscription = Some(cx.spawn(async move |this, cx| {
                while readiness.changed().await.is_ok() {
                    if readiness.take_urgent() {
                        if this
                            .update(cx, |view, cx| view.deliver_core_updates(cx))
                            .is_err()
                        {
                            return;
                        }
                    } else if !zork_ui::components::frame_delivery::FrameDelivery::request(
                        &this,
                        cx,
                        |view| &mut view.frame_delivery,
                        Self::deliver_core_updates,
                    ) {
                        return;
                    }
                }
            }));
        }
        if self.history.subscription.is_none() {
            let mut changes = core.subscribe();
            if let Some(update) = changes.prepare() {
                let batch = update.batch.unwrap();
                self.apply_history_update(update, cx);
                changes.acknowledge(batch);
            }
            let mut readiness = changes.readiness();
            self.history.updates = Some(changes);
            self.history.subscription = Some(cx.spawn(async move |this, cx| {
                while readiness.changed().await.is_ok() {
                    if readiness.take_urgent() {
                        if this
                            .update(cx, |view, cx| view.deliver_core_updates(cx))
                            .is_err()
                        {
                            return;
                        }
                        continue;
                    }
                    if !zork_ui::components::frame_delivery::FrameDelivery::request(
                        &this,
                        cx,
                        |view| &mut view.frame_delivery,
                        Self::deliver_core_updates,
                    ) {
                        return;
                    }
                }
            }));
        }
        self.history.source = Some(source);
        Some(core)
    }
    pub(super) fn deliver_history_updates(&mut self, cx: &mut Context<Self>) {
        if let Some(mut updates) = self.history.overview_updates.take() {
            if let Some(update) = updates.prepare() {
                self.apply_history_overview(&update.state.overview, cx);
                updates.acknowledge(update.batch.unwrap());
            }
            self.history.overview_updates = Some(updates);
        }
        if let Some(mut updates) = self.history.updates.take() {
            if let Some(update) = updates.prepare() {
                let batch = update.batch.unwrap();
                self.apply_history_update(update, cx);
                updates.acknowledge(batch);
            }
            self.history.updates = Some(updates);
        }
    }
    fn apply_history_update(
        &mut self,
        update: zork_client_core::state::HistoryUpdate,
        cx: &mut Context<Self>,
    ) {
        let h = &mut self.history;
        let changed = live::HistoryChanged::from_update(
            &update,
            h.clock_offset != update.state.clock_offset_ms,
        );
        let follow = !h.loaded || h.scroll.is_scrolled_to_end().unwrap_or(true);
        let anchor_offset = h.scroll.logical_scroll_top();
        // The paging control occupies row zero. Anchor the first real record
        // while it is visible, retaining the space above it as a negative offset.
        let anchor_row = Some(anchor_offset.item_ix.max(1) - 1);
        let offset_in_item = if anchor_offset.item_ix == 0 {
            anchor_offset.offset_in_item
                - h.scroll
                    .bounds_for_item(0)
                    .map(|b| b.size.height)
                    .unwrap_or_default()
        } else {
            anchor_offset.offset_in_item
        };
        let reading_entry = anchor_row
            .and_then(|index| h.rows.get(index))
            .is_some_and(|row| row.activity.is_some());
        let anchor = anchor_row
            .and_then(|index| h.row_entry(index))
            .map(|e| e.id.clone());
        let previous_count = h.rows.len();
        h.records = update.state.records.clone();
        h.entries = update.state.entries.clone();
        h.older = update.state.older.clone();
        h.latest = update.state.latest.clone();
        h.busy = update.state.loading;
        h.loading_older = update.state.loading_older;
        h.loaded = update.state.loaded;
        h.error = update.state.error.clone();
        h.clock_offset = update.state.clock_offset_ms;
        if update.reset || update.entries.is_some() {
            h.projection = Projection::new(h.entries.iter());
            h.expanded = h.projection.restore_expansion(
                h.entries.iter(),
                &h.expanded,
                if reading_entry && (update.prepended || !follow) {
                    anchor.as_deref()
                } else {
                    None
                },
            );
            h.rows = h.projection.rows(h.entries.iter(), &h.expanded);
            h.scroll.splice(1..previous_count + 1, h.rows.len());
            if update.prepended || !follow {
                if let Some(index) = anchor.and_then(|id| h.row_for_id(&id)) {
                    h.scroll.scroll_to(ListOffset {
                        item_ix: index + 1,
                        offset_in_item,
                    });
                }
            } else {
                h.scroll.scroll_to_end();
            }
        }
        if update.reset || update.entries.is_some() {
            self.refresh_history_sources();
        }
        cx.emit(changed);
        zork_ui::components::region::invalidate(cx, &["history", "header"]);
    }
    fn apply_history_overview(
        &mut self,
        overview: &zork_client_core::state::SessionOverview,
        cx: &mut Context<Self>,
    ) {
        if self.history.runtime != overview.runtime {
            self.history.quota = None;
        }
        self.history.runtime = overview.runtime.clone();
        self.history.usage = overview.usage();
        self.history.usage_complete = overview.aggregates.complete;
        self.history.overview_loaded = overview.loaded;
        zork_ui::components::region::invalidate(cx, &["history"]);
    }
    fn history_name(&self) -> String {
        if let Some(p) = self
            .participants
            .iter()
            .find(|p| Some(&p.session_id) == self.history.session.as_ref())
        {
            return p.name.clone();
        }
        self.node_agents
            .iter()
            .find(|a| a["session_id"].as_str() == self.history.session.as_deref())
            .and_then(|a| a["name"].as_str())
            .unwrap_or("小伙伴")
            .to_owned()
    }

    fn history_action(&self, e: &Entry) -> String {
        match e.lane {
            1 => self
                .locale
                .text(match e.action.as_str() {
                    "compaction" => "history_compaction",
                    "handoff" => "history_handoff",
                    _ => "history_model",
                })
                .into(),
            0 if e.action == "input" => self.locale.text("history_input").into(),
            _ => e.action.clone(),
        }
    }
    fn history_state(&self, e: &Entry) -> &'static str {
        self.locale.text(match e.state.as_str() {
            "running" => "history_running",
            "succeeded" => "history_success",
            "received" => "history_received",
            "cancelled" | "interrupted" => "history_cancelled",
            "failed" | "timed_out" => "history_error",
            _ => "history_notice",
        })
    }
    fn history_select(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(e) = self.history.entries.get(index) {
            let id = e.id.clone();
            self.history.selected = Some(id.clone());
            if let Some(block) = self
                .history
                .projection
                .entry_to_block
                .get(index)
                .copied()
                .flatten()
            {
                let b = &self.history.projection.blocks[block];
                if b.is_group() {
                    let first = self.history.projection.activities[b.start].entry;
                    self.history
                        .expanded
                        .insert(self.history.entries[first].id.clone());
                    self.history.rebuild_rows();
                }
                if let Some(row) = self.history.row_for_id(&id) {
                    self.history.scroll.scroll_to_reveal_item(row + 1);
                }
            }
            zork_ui::components::region::invalidate(cx, &["history", "header"]);
        }
    }
    fn render_history_bars(
        &self,
        lane: usize,
        row_height: f32,
        timeline: &model::Timeline,
        position: &impl Fn(i64) -> f64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let automated = crate::automation::is_enabled(cx);
        let bars = Rc::new(
            timeline
                .spans
                .iter()
                .filter_map(|span| {
                    let entry = &self.history.entries[span.index];
                    let left = position(span.start);
                    let right = position(span.end);
                    if entry.lane != lane || right < 0. || left > 1. {
                        return None;
                    }
                    Some(HistoryBar {
                        index: span.index,
                        id: entry.id.clone(),
                        left: left.max(0.) as f32,
                        width: (right.min(1.) - left.max(0.)).max(0.) as f32,
                        top: span.track as f32 * row_height,
                        height: 11.,
                        color: history_color(entry),
                        open: entry.end.is_none(),
                        selected: self.history.selected.as_ref() == Some(&entry.id),
                        layout: Default::default(),
                        label: automated.then(|| {
                            format!(
                                "{} · {} · {}",
                                self.history_action(entry),
                                self.history_state(entry),
                                entry.summary.chars().take(80).collect::<String>()
                            )
                        }),
                    })
                })
                .collect::<Vec<_>>(),
        );
        let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let hover_bars = bars.clone();
        let hover_bounds = bounds.clone();
        let leave_bounds = bounds.clone();
        let click_bars = bars.clone();
        let click_bounds = bounds.clone();
        let layout_owner = cx.entity().downgrade();
        div()
            .id(("history-bars", lane))
            .absolute()
            .size_full()
            .on_mouse_move(cx.listener(move |v, event: &gpui::MouseMoveEvent, _, cx| {
                if let Some(bar) = history_bar_at(&hover_bars, hover_bounds.get(), event.position) {
                    v.history.hover_close.take();
                    let anchor = bar.bounds(hover_bounds.get());
                    if v.history.hovered.as_ref() != Some(&bar.id)
                        || v.history.hover_anchor != anchor
                    {
                        v.history.hovered = Some(bar.id.clone());
                        v.history.hover_anchor = anchor;
                        zork_ui::components::region::invalidate(cx, &["history"]);
                    }
                } else {
                    v.history_leave_hover(cx);
                }
            }))
            .on_hover(cx.listener(move |v, hovered, _, cx| {
                if !hovered
                    && leave_bounds
                        .get()
                        .contains(&v.history.hover_anchor.center())
                {
                    v.history_leave_hover(cx);
                }
            }))
            .on_click(cx.listener(move |v, event: &gpui::ClickEvent, _, cx| {
                if let Some(bar) = history_bar_at(&click_bars, click_bounds.get(), event.position())
                {
                    v.history_select(bar.index, cx);
                }
            }))
            .child(
                gpui::canvas(
                    move |area, window, cx| {
                        bounds.set(area);
                        layout_history_bars(&bars, area, lane == 1);
                        // Wheel zoom/pan, new data and a moving axis can change
                        // the hit target without any MouseMoveEvent.
                        let hovered = history_bar_at(&bars, area, window.mouse_position())
                            .map(|bar| (bar.id.clone(), bar.bounds(area)));
                        let owner = layout_owner.clone();
                        cx.defer(move |cx| {
                            let _ = owner.update(cx, |v, cx| {
                                if let Some((id, anchor)) = hovered {
                                    v.history.hover_close.take();
                                    if v.history.hovered.as_ref() != Some(&id)
                                        || v.history.hover_anchor != anchor
                                    {
                                        v.history.hovered = Some(id);
                                        v.history.hover_anchor = anchor;
                                        zork_ui::components::region::invalidate(cx, &["history"]);
                                    }
                                } else if area.contains(&v.history.hover_anchor.center()) {
                                    v.history_leave_hover(cx);
                                }
                            });
                        });
                        let hitboxes = bars
                            .iter()
                            .map(|bar| {
                                let rect = bar.bounds(area);
                                if let Some(label) = &bar.label {
                                    crate::automation::record_canvas_control(
                                        cx,
                                        window,
                                        format!("{}-{}", "history-bar", bar.index),
                                        label.clone(),
                                        rect,
                                    );
                                }
                                window.insert_hitbox(rect, gpui::HitboxBehavior::Normal)
                            })
                            .collect::<Vec<_>>();
                        (bars, hitboxes)
                    },
                    move |area, (bars, hitboxes), window, _| {
                        for hitbox in &hitboxes {
                            window.set_cursor_style(gpui::CursorStyle::PointingHand, hitbox);
                        }
                        // Batch consecutive quads in one layer, avoiding a scene ordering
                        // tree insertion for every overlapping point marker. Open paths
                        // retain separate ordering relative to the quads around them.
                        let mut start = 0;
                        while start < bars.len() {
                            if bars[start].open {
                                bars[start].paint(area, window);
                                start += 1;
                            } else {
                                let end = bars[start..]
                                    .iter()
                                    .position(|bar| bar.open)
                                    .map_or(bars.len(), |offset| start + offset);
                                window.paint_layer(area, |window| {
                                    for bar in &bars[start..end] {
                                        bar.paint(area, window);
                                    }
                                });
                                start = end;
                            }
                        }
                    },
                )
                .size_full(),
            )
    }

    fn render_history_timeline(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let now = self.history.now();
        let t = model::timeline(self.history.entries.iter(), now);
        let axis = model::Axis::new(self.history.entries.iter(), &t);
        let zoom = self.history.zoom;
        let pan = self.history.pan;
        let start = axis.time_at(pan);
        let end = axis.time_at(pan + 1. / zoom);
        let position = |time: i64| (axis.position(time) - pan) * zoom;
        let row_height = 11.;
        let mut offsets = [0.; 3];
        let mut height = 4.;
        for lane in 0..3 {
            offsets[lane] = height;
            height += t.tracks[lane].max(1) as f32 * row_height;
        }
        let bounds = self.history.bounds.clone();
        div()
            .flex()
            .flex_col()
            .relative()
            .map(|d| {
                d.child(
                    div()
                        .h(px(18.))
                        .ml(px(44.))
                        .pr(px(12.))
                        .flex()
                        .justify_between()
                        .pt(px(3.))
                        .text_size(px(10.))
                        .font_family("Menlo")
                        .text_color(rgb(DIM))
                        .children((0..5).map(|i| {
                            let fraction = i as f64 / 4.;
                            let time = axis.time_at(pan + fraction / zoom);
                            div().child(model::axis_clock(time, end - start))
                        })),
                )
            })
            .child(
                div()
                    .id("history-plot")
                    .max_h(px(128.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .h(px(height))
                            .map(|d| {
                                d.child(
                                    gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                                        .absolute()
                                        .size_full(),
                                )
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(44.))
                                    .right_0()
                                    .h(px(3.))
                                    .children(axis.gaps.iter().filter_map(|gap| {
                                        let x = position(gap.0);
                                        (0. ..=1.).contains(&x).then(|| {
                                            div()
                                                .absolute()
                                                .left(relative(x as f32))
                                                .ml(px(-1.5))
                                                .size(px(3.))
                                                .rounded(px(1.5))
                                                .bg(rgb(0x9CA3AF))
                                        })
                                    })),
                            )
                            .children((0..3).map(|lane| {
                                let lane_height = t.tracks[lane].max(1) as f32 * row_height;
                                div()
                                    .absolute()
                                    .top(px(offsets[lane]))
                                    .h(px(lane_height))
                                    .w_full()
                                    .flex()

                                    .child(
                                        div()
                                            .w(px(44.))
                                            .h_full()
                                            .flex_shrink_0()

                                            .px(px(6.))
                                            .text_right()
                                            .text_size(px(10.))
                                            .line_height(px(11.))
                                            .text_color(rgb(DIM))
                                            .child(self.locale.text(
                                                [
                                                    "history_lane_input",
                                                    "history_lane_model",
                                                    "history_tool",
                                                ][lane],
                                            )),
                                    )
                                    .child(
                                        div()
                                            .relative()
                                            .flex_1()
                                            .h_full()
                                            .overflow_hidden()
                                            .bg(rgb(CUE_UI.palette.canvas))
                                            .map(|d| {
                                                d.children(self.history.range.map(|(a, b)| {
                                                    let left =
                                                        ((a.min(b) - pan) * zoom).clamp(0., 1.);
                                                    let right =
                                                        ((a.max(b) - pan) * zoom).clamp(0., 1.);
                                                    div()
                                                        .absolute()
                                                        .h_full()
                                                        .left(relative(left as f32))
                                                        .w(relative((right - left) as f32))
                                                        .bg(rgba((zork_ui::components::history::SEND_COLOR << 8) | 0x1A))
                                                        .border_x_1()
                                                        .border_color(rgba((zork_ui::components::history::SEND_COLOR << 8) | 0x4D))
                                                }))
                                            })
                                            .child(self.render_history_bars(
                                                lane, row_height, &t, &position, cx,
                                            )),

                                    )
                            })),
                    ),
            )
            .when_some(
                self.history.hovered.as_ref().and_then(|id| {
                    t.spans
                        .iter()
                        .find(|s| &self.history.entries[s.index].id == id)
                }),
                |d, span| {
                    let id = self.history.entries[span.index].id.clone();
                    let mut anchor = self.history.hover_anchor;
                    // Keep the moving card above all lanes so it cannot steal
                    // the pointer from nodes during a cross-lane reversal.
                    anchor.origin.y = self.history.bounds.get().top();
                    d.child(live::popup(id, anchor, window, cx))
                },
            )
    }
    fn history_leave_hover(&mut self, cx: &mut Context<Self>) {
        if self.history.hovered.is_none() || self.history.hover_close.is_some() {
            return;
        }
        self.history.hover_close = Some(cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(80))
                .await;
            let _ = view.update(cx, |v, cx| {
                v.history.hovered = None;
                v.history.hover_close = None;
                zork_ui::components::region::invalidate(cx, &["history"]);
            });
        }));
    }
    fn render_history_span_detail(&self, e: &Entry) -> impl IntoElement {
        let now = self.history.now();
        let color = history_color(e);
        div()
            .id("history-span-detail")
            .w_full()
            .px(px(14.))
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .line_height(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(div().size(px(7.)).rounded(px(2.)).bg(rgb(color)))
                            .child(if e.lane == 2 {
                                self.locale.text("history_tool").to_owned()
                            } else {
                                self.history_action(e)
                            }),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(rgb(zork_ui::design::CUE_UI.palette.sidebar_hover))
                            .text_size(px(10.))
                            .text_color(rgb(DIM))
                            .child(self.history_state(e)),
                    ),
            )
            .when(e.lane == 2, |d| {
                d.child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(e.action.clone()),
                )
            })
            .child(
                div()
                    .max_h(px(54.))
                    .overflow_hidden()
                    .child(e.summary.clone()),
            )
            .when_some(e.duration(now).filter(|_| e.lane != 0), |d, ms| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(DIM))
                                .child(self.locale.text("history_duration")),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(model::duration(ms)),
                        ),
                )
            })
            .when(e.lane == 1, |d| d.child(self.render_history_metrics(e)))
            .child(div().text_size(px(10.)).text_color(rgb(DIM)).child(format!(
                "{}  →  {}",
                model::clock(e.start),
                model::clock(e.end)
            )))
            .automation(
                AutomationRole::Status,
                format!(
                    "{} · {}{}",
                    e.summary,
                    self.history_state(e),
                    e.duration(now)
                        .filter(|_| e.lane != 0)
                        .map(|ms| format!(" · {}", model::duration(ms)))
                        .unwrap_or_default()
                ),
            )
    }
    fn render_history_metrics(&self, e: &Entry) -> Div {
        zork_ui::components::history::metrics(
            e,
            self.locale.text("history_input_tokens"),
            self.locale.text("history_output_tokens"),
            self.locale.text("history_cache"),
        )
    }
    fn render_history_json(
        &self,
        value: &serde_json::Value,
        path: String,
        depth: usize,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let collection = match value {
            serde_json::Value::Object(v) => Some(("{", "}", v.len())),
            serde_json::Value::Array(v) => Some(("[", "]", v.len())),
            _ => None,
        };
        let open = if depth == 0 {
            !self.history.json_open.contains(&path)
        } else {
            self.history.json_open.contains(&path)
        };
        let mut row = div()
            .flex()
            .items_center()
            .min_h(px(18.))
            .line_height(px(18.))
            .gap(px(4.));
        if collection.is_some() {
            row = row.child(ui::icon("icons/chevron-down.svg", 10.).when(!open, |s| {
                s.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    -std::f32::consts::FRAC_PI_2,
                )))
            }));
        }
        if let Some(label) = label {
            row = row.child(div().text_color(rgb(CUE_UI.palette.muted)).child(format!(
                "{}:",
                serde_json::to_string(&label).unwrap_or_default()
            )));
        }
        if let Some((a, b, n)) = collection {
            let key = path.clone();
            row = row.child(if open {
                a.to_owned()
            } else {
                format!("{a}…{b} ({n})")
            });
            let mut tree = div().child(
                div()
                    .id(gpui::SharedString::from(path.clone()))
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, _, _, cx| {
                        if !v.history.json_open.remove(&key) {
                            v.history.json_open.insert(key.clone());
                        }
                        zork_ui::components::region::invalidate(cx, &["history", "header"]);
                    }))
                    .child(row)
                    .automation(AutomationRole::Button, format!("JSON {path}")),
            );
            if open {
                let children: Vec<(String, &serde_json::Value)> = match value {
                    serde_json::Value::Object(v) => v.iter().map(|(k, v)| (k.clone(), v)).collect(),
                    serde_json::Value::Array(v) => v
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v))
                        .collect(),
                    _ => vec![],
                };
                tree =
                    tree.child(div().pl(px(16.)).children(children.into_iter().map(
                        |(key, val)| {
                            self.render_history_json(
                                val,
                                format!("{path}/{key}"),
                                depth + 1,
                                if value.is_array() { None } else { Some(key) },
                                cx,
                            )
                        },
                    )))
                    .child(b);
            }
            tree
        } else {
            let color = match value {
                serde_json::Value::String(_) => CUE_UI.palette.text,
                serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
                    zork_ui::components::history::SEND_COLOR
                }
                _ => DIM,
            };
            div().child(row.child(div().text_color(rgb(color)).child(value.to_string())))
        }
    }
    fn render_history_older(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("history-older")
            .w_full()
            .py(px(3.))
            .line_height(px(16.5))
            .text_center()
            .text_size(px(11.))
            .text_color(rgb(DIM))
            .when(
                !self.history.busy
                    && (self.history.older.is_some() || self.history.error.is_some()),
                |v| v.cursor_pointer(),
            )
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .when(
                self.history.busy && (!self.history.loaded || self.history.loading_older),
                |v| v.child(loading::indicator("history-loading", 14.)),
            )
            .child(self.locale.text(
                if self.history.busy && (!self.history.loaded || self.history.loading_older) {
                    "history_loading"
                } else if self.history.error.is_some() {
                    "history_retry"
                } else if self.history.older.is_some() {
                    "history_older"
                } else {
                    "history_start"
                },
            ))
            .on_click(cx.listener(|v, _, _, cx| {
                if !v.history.busy && (v.history.older.is_some() || v.history.error.is_some()) {
                    v.load_history(v.history.older.is_some(), cx);
                }
            }))
            .automation_enabled(
                !self.history.busy
                    && (self.history.older.is_some() || self.history.error.is_some()),
                AutomationRole::Button,
                self.locale.text("history_older"),
            )
    }
    pub(super) fn render_history_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if !self.history.scroll_observed {
            Self::observe_scroll(&self.history.scroll, cx);
            self.history.scroll_observed = true;
        }
        let selection_range = self.history.range.map(|(a, b)| {
            let timeline = model::timeline(self.history.entries.iter(), self.history.now());
            let axis = model::Axis::new(self.history.entries.iter(), &timeline);
            (axis.time_at(a.min(b)), axis.time_at(a.max(b)))
        });
        div()
            .relative()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .font_weight(FontWeight(450.))
            .child(
                div()
                    .id("history-ledger")
                    .when(
                        self.history.loaded
                            && self.history.rows.is_empty()
                            && self.history.error.is_none(),
                        |v| {
                            v.child(
                                div()
                                    .p_4()
                                    .text_size(px(12.))
                                    .text_color(rgb(DIM))
                                    .child(self.locale.text("history_no_activity")),
                            )
                        },
                    )
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        gpui::list(
                            self.history.scroll.clone(),
                            cx.processor(move |v, index, window, cx| {
                                if index == 0 {
                                    v.render_history_older(cx).into_any_element()
                                } else if index <= v.history.rows.len() {
                                    live::row(v, index - 1, selection_range, window, cx)
                                } else {
                                    div().into_any_element()
                                }
                            }),
                        )
                        .flex_1()
                        .min_h_0(),
                    )
                    .automation(AutomationRole::ScrollArea, "History records"),
            )
            .child(self.render_history_statistics())
            .child(live::timeline(window, cx))
    }

    fn render_history_timeline_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("history-timeline-panel")
            .pb(px(4.))
            .flex_shrink_0()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .id("history-axis")
                    .relative()
                    .on_scroll_wheel(cx.listener(|v, event: &gpui::ScrollWheelEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(16.));
                        let h = &mut v.history;
                        let bounds = h.bounds.get();
                        let width = (f32::from(bounds.size.width) - 44.).max(1.) as f64;
                        let x = f32::from(delta.x) as f64;
                        let y = f32::from(delta.y) as f64;
                        if event.modifiers.shift || x.abs() > y.abs() {
                            h.pan -= if x.abs() > 0. { x } else { y } / width / h.zoom;
                        } else if y != 0. {
                            let pointer = ((f32::from(event.position.x - bounds.origin.x) as f64
                                - 44.)
                                / width)
                                .clamp(0., 1.);
                            let old = 1. / h.zoom;
                            h.zoom = (h.zoom * (y * 0.006).exp()).clamp(1., 64.);
                            h.pan += pointer * (old - 1. / h.zoom);
                        }
                        h.pan = h.pan.clamp(0., 1. - 1. / h.zoom);
                        h.range = None;
                        cx.stop_propagation();
                        zork_ui::components::region::invalidate(cx, &["history", "header"]);
                    }))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|v, event: &gpui::MouseDownEvent, _, cx| {
                            let h = &mut v.history;
                            let b = h.bounds.get();
                            let width = f32::from(b.size.width) - 44.;
                            if width > 0. {
                                let f = ((f32::from(event.position.x - b.origin.x) - 44.) / width)
                                    .clamp(0., 1.) as f64
                                    / h.zoom
                                    + h.pan;
                                h.range = Some((f, f));
                                h.dragging = true;
                            }
                            zork_ui::components::region::invalidate(cx, &["history", "header"]);
                        }),
                    )
                    .on_mouse_move(cx.listener(|v, event: &gpui::MouseMoveEvent, _, cx| {
                        let h = &mut v.history;
                        if !h.dragging {
                            return;
                        }
                        let b = h.bounds.get();
                        let width = f32::from(b.size.width) - 44.;
                        if width > 0. {
                            let f = ((f32::from(event.position.x - b.origin.x) - 44.) / width)
                                .clamp(0., 1.) as f64
                                / h.zoom
                                + h.pan;
                            if let Some((a, _)) = h.range {
                                h.range = Some((a, f));
                            }
                        }
                        zork_ui::components::region::invalidate(cx, &["history", "header"]);
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|v, _, _, cx| {
                            v.history.dragging = false;
                            if v.history.range.is_some_and(|(a, b)| (a - b).abs() < 0.004) {
                                v.history.range = None;
                            }
                            zork_ui::components::region::invalidate(cx, &["history", "header"]);
                        }),
                    )
                    .on_mouse_up_out(
                        gpui::MouseButton::Left,
                        cx.listener(|v, _, _, cx| {
                            v.history.dragging = false;
                            zork_ui::components::region::invalidate(cx, &["history", "header"]);
                        }),
                    )
                    .child(self.render_history_timeline(window, cx)),
            )
            .automation(AutomationRole::ScrollArea, "History timeline")
    }
}
fn history_color(e: &Entry) -> u32 {
    // Timeline marks need stronger separation than the reading-list accents.
    if matches!(e.state.as_str(), "failed" | "timed_out") {
        0xD43D45
    } else {
        [0x2878CE, 0x8056C4, 0x21865B][e.lane.min(2)]
    }
}

const HISTORY_BAR_MIN_WIDTH: f32 = 2.;

struct HistoryBar {
    index: usize,
    id: String,
    left: f32,
    width: f32,
    top: f32,
    height: f32,
    color: u32,
    open: bool,
    selected: bool,
    label: Option<String>,
    layout: std::cell::Cell<Option<(gpui::Pixels, gpui::Pixels)>>,
}
impl HistoryBar {
    fn paint(&self, area: gpui::Bounds<gpui::Pixels>, window: &mut Window) {
        let rect = self.bounds(area);
        let inset = 0.5;
        let fill = gpui::Bounds::new(
            rect.origin + gpui::point(px(0.), px(inset)),
            gpui::size(rect.size.width, rect.size.height - px(inset * 2.)),
        );
        let radius = px(HISTORY_BAR_MIN_WIDTH / 2.);
        if self.selected {
            window.paint_quad(
                gpui::fill(fill.dilate(px(3.)), rgba((self.color << 8) | 27))
                    .corner_radii(radius + px(3.)),
            );
            window.paint_quad(
                gpui::fill(fill.dilate(px(1.)), rgba((self.color << 8) | 191))
                    .corner_radii(radius + px(1.)),
            );
        }
        window.paint_quad(gpui::fill(fill, rgb(self.color)).corner_radii(radius));
    }

    fn bounds(&self, lane: gpui::Bounds<gpui::Pixels>) -> gpui::Bounds<gpui::Pixels> {
        let (left, width) = self.layout.get().unwrap_or((
            lane.size.width * self.left,
            (lane.size.width * self.width).max(px(HISTORY_BAR_MIN_WIDTH)),
        ));
        gpui::Bounds::new(
            lane.origin + gpui::point(left, px(self.top)),
            gpui::size(width, px(self.height)),
        )
    }
}
// Resolve spacing in pixels after idle compression and zoom. Use the same
// geometry for painting, pointer picking and accessibility hitboxes.
fn layout_history_bars(bars: &[HistoryBar], area: gpui::Bounds<gpui::Pixels>, separate: bool) {
    for bar in bars {
        bar.layout.set(None);
    }
    if !separate {
        return;
    }
    let mut order = bars.iter().collect::<Vec<_>>();
    order.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left)));
    let mut previous: Option<(f32, gpui::Pixels)> = None;
    for bar in order {
        let mut left = area.size.width * bar.left + px(0.5);
        if let Some((track, right)) = previous {
            if track == bar.top {
                left = left.max(right + px(1.));
            }
        }
        let right = (area.size.width * (bar.left + bar.width) - px(0.5))
            .max(left + px(HISTORY_BAR_MIN_WIDTH));
        bar.layout.set(Some((left, right - left)));
        previous = Some((bar.top, right));
    }
}

fn history_bar_at(
    bars: &[HistoryBar],
    lane: gpui::Bounds<gpui::Pixels>,
    point: gpui::Point<gpui::Pixels>,
) -> Option<&HistoryBar> {
    if !lane.contains(&point) {
        return None;
    }
    // Later bars paint above earlier bars, including overlapping point markers.
    bars.iter()
        .rev()
        .find(|bar| bar.bounds(lane).contains(&point))
}

#[cfg(test)]
mod canvas_tests {
    use super::*;
    #[gpui::test]
    fn partner_history_tracks_clicked_session_across_switches_and_chat_restore(
        cx: &mut gpui::TestAppContext,
    ) {
        let view = cx.new(|cx| {
            RootView::new(
                Arc::new(GatewayClient::new("http://127.0.0.1:9", None)),
                None,
                cx,
            )
        });
        view.update(cx, |view, cx| {
            view.selected_session = Some("chat".into());
            view.node_agents = vec![
                serde_json::json!({"id":"a", "name":"Alice", "session_id":"session-a"}),
                serde_json::json!({"id":"b", "name":"Bob", "session_id":"session-b"}),
            ]
            .into();
            view.restore_chat_history(cx);
            view.toggle_history("session-a", cx);
            assert_eq!(view.history.session.as_deref(), Some("session-a"));
            assert_eq!(view.history_name(), "Alice");
            assert!(view.history.subscription.is_some());
            view.history.detail = Some("alice-event".into());

            // Clicking another avatar switches the open page instead of closing it.
            view.toggle_history("session-b", cx);
            assert!(view.history.open);
            assert_eq!(view.history.session.as_deref(), Some("session-b"));
            assert_eq!(view.history_name(), "Bob");
            assert!(view.history.detail.is_none());
            assert_eq!(view.selected_session.as_deref(), Some("chat"));

            // Empty identities never fall back to an unrelated conversation.
            view.toggle_history("", cx);
            assert_eq!(view.history.session.as_deref(), Some("session-b"));
            view.save_chat_history();
            view.selected_session = Some("other-chat".into());
            view.restore_chat_history(cx);
            assert!(!view.history.open);
            view.save_chat_history();
            view.selected_session = Some("chat".into());
            view.restore_chat_history(cx);
            assert_eq!(view.history.session.as_deref(), Some("session-b"));
            assert_eq!(view.history_name(), "Bob");
            assert!(view.history.open);

            view.toggle_history("session-b", cx);
            assert!(!view.history.open);
            assert!(view.history.subscription.is_none());
            view.toggle_history("session-b", cx);
            assert!(view.history.open);
            assert_eq!(view.history.session.as_deref(), Some("session-b"));
        });
    }

    #[test]
    fn point_markers_pick_the_topmost_visible_bar_after_scaling() {
        let bar = |index, left| HistoryBar {
            index,
            id: index.to_string(),
            left,
            width: 0.,
            top: 4.,
            height: 16.,
            color: 0,
            open: false,
            selected: false,
            label: None,
            layout: Default::default(),
        };
        let bars = [bar(0, 0.5), bar(1, 0.505), bar(2, 1.)];
        let area = gpui::Bounds::new(
            gpui::point(px(100.), px(50.)),
            gpui::size(px(200.), px(32.)),
        );
        assert_eq!(
            history_bar_at(&bars, area, gpui::point(px(202.), px(60.)))
                .unwrap()
                .index,
            1
        );
        assert_eq!(
            history_bar_at(&bars, area, gpui::point(px(200.5), px(60.)))
                .unwrap()
                .index,
            0
        );
        assert!(history_bar_at(&bars, area, gpui::point(px(302.), px(60.))).is_none());
        assert!(history_bar_at(&bars, area, gpui::point(px(202.), px(52.))).is_none());

        // Two sub-pixel model requests keep their minimum width and gain a
        // real empty pixel, including in pointer picking after zoom/resize.
        layout_history_bars(&bars, area, true);
        let first = bars[0].bounds(area);
        let second = bars[1].bounds(area);
        assert_eq!(first.size.width, px(2.));
        assert_eq!(second.origin.x - first.right(), px(1.));
        assert!(
            history_bar_at(&bars, area, gpui::point(first.right() + px(0.5), px(60.))).is_none()
        );
        assert_eq!(
            history_bar_at(&bars, area, second.center()).unwrap().index,
            1
        );
    }
}

#[cfg(feature = "headless-bench")]
impl RootView {
    pub fn benchmark_observe_history(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Arc<zork_client_core::state::History> {
        let conversation = self
            .core_device
            .conversation(self.history.session.as_deref().unwrap());
        let source = conversation.history();
        let fixture = zork_client_core::state::HistoryData {
            records: self.history.records.clone(),
            entries: self.history.entries.clone(),
            runtime: self.history.runtime.clone(),
            loaded: true,
            ..Default::default()
        };
        let mut state = (*conversation.snapshot()).clone();
        state.overview = Arc::new(zork_client_core::state::SessionOverview::fixture(&fixture));
        conversation.seed(state);
        source.seed(fixture);
        self.observe_history(cx).unwrap()
    }

    pub fn benchmark_history_live_clock(&mut self, cx: &mut Context<Self>) {
        self.history.clock_offset = self.history.now() - model::now();
        self.history.fixed_now = None;
        zork_ui::components::region::invalidate(cx, &["history"]);
    }

    pub fn benchmark_history_runtime(
        &mut self,
        runtime: zork_client_core::state::HistoryRuntime,
        cx: &mut Context<Self>,
    ) {
        self.history.runtime = Some(runtime);
        self.history.quota = None;
        zork_ui::components::region::invalidate(cx, &["history"]);
    }

    pub fn benchmark_prepend_history(
        &mut self,
        records: Vec<crate::session_history::Record>,
        cx: &mut Context<Self>,
    ) {
        let entries = crate::session_history::entries(&records);
        self.apply_history_update(
            zork_client_core::state::HistoryUpdate {
                state: Arc::new(zork_client_core::state::HistoryData {
                    records: records.into(),
                    entries: entries.into(),
                    loaded: true,
                    older: Some("fixture-older".into()),
                    ..Default::default()
                }),
                entries: None,
                prepended: true,
                reset: true,
                batch: None,
                cursor: zork_client_core::observe::Cursor {
                    source: 0,
                    revision: 0,
                },
            },
            cx,
        );
    }

    pub fn benchmark_scroll_history(&mut self, item: usize, offset: f32, cx: &mut Context<Self>) {
        self.history.scroll.scroll_to(gpui::ListOffset {
            item_ix: item,
            offset_in_item: px(offset),
        });
        zork_ui::components::region::invalidate(cx, &["history"]);
    }
}
