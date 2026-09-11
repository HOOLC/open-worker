//! Visible history leaves observe the core projection by stable entry ID.
//! Their clocks wake only when their own displayed time can change.
use super::*;
use std::collections::HashSet;

pub(super) struct HistoryChanged {
    entries: HashSet<String>,
    structure: bool,
    clock: bool,
}

impl gpui::EventEmitter<HistoryChanged> for RootView {}

impl HistoryChanged {
    pub(super) fn from_update(
        update: &zork_client_core::state::HistoryUpdate,
        clock: bool,
    ) -> Self {
        let mut entries = HashSet::new();
        let mut structure = update.reset;
        if let Some(changes) = &update.entries {
            entries.extend(changes.changed_ids.iter().cloned());
            structure |= changes.structure_changed;
        }
        Self {
            entries,
            structure,
            clock,
        }
    }
}

#[derive(Clone, PartialEq)]
enum Target {
    Row {
        id: String,
        group: bool,
        index: usize,
        selection: Option<(i64, i64)>,
    },
    Popup {
        id: String,
        anchor: gpui::Bounds<gpui::Pixels>,
    },
    Timeline,
}

struct Leaf {
    root: gpui::WeakEntity<RootView>,
    target: Target,
    observed: Vec<String>,
    source_changed: bool,
    revision: u64,
    _subscription: gpui::Subscription,
    clock: Option<Task<()>>,
}

fn element(target: Target, window: &mut Window, cx: &mut Context<RootView>) -> gpui::AnyElement {
    let key = match &target {
        Target::Row { id, group, .. } => format!("history-live-row-{group}-{id}"),
        Target::Popup { .. } => "history-live-popup".into(),
        Target::Timeline => "history-live-timeline".into(),
    };
    let root = cx.entity();
    let initial = target.clone();
    // The keyed holder owns the leaf's lifetime without observing its redraws.
    // A ticking item must not notify the list and restart all sibling clocks.
    let holder = window.use_keyed_state(key, cx, |_, cx| {
        cx.new(|cx| {
            zork_ui::components::region::forget_on_release(cx);
            let subscription =
                cx.subscribe(&root, |v: &mut Leaf, _, update: &HistoryChanged, cx| {
                    if update.structure
                        || update.clock
                        || matches!(v.target, Target::Timeline)
                        || v.observed.iter().any(|id| update.entries.contains(id))
                    {
                        v.revision = v.revision.wrapping_add(1);
                        v.source_changed = true;
                        cx.notify();
                    }
                });
            Leaf {
                root: root.downgrade(),
                target: initial,
                observed: Vec::new(),
                source_changed: true,
                revision: 0,
                _subscription: subscription,
                clock: None,
            }
        })
    });
    let leaf = holder.read(cx).clone();
    leaf.update(cx, |v, cx| {
        if v.target != target {
            let same_content = matches!((&v.target, &target),
                (Target::Popup { id: previous, .. }, Target::Popup { id, .. }) if previous == id);
            v.target = target;
            if !same_content {
                v.source_changed = true;
                v.revision = v.revision.wrapping_add(1);
            }
            cx.notify();
        }
    });
    zork_ui::components::region::tracked_view(leaf)
}

pub(super) fn row(
    view: &RootView,
    index: usize,
    selection: Option<(i64, i64)>,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    element(
        Target::Row {
            id: view.history.row_entry(index).unwrap().id.clone(),
            group: view.history.rows[index].activity.is_none(),
            index,
            selection,
        },
        window,
        cx,
    )
}

pub(super) fn timeline(window: &mut Window, cx: &mut Context<RootView>) -> gpui::AnyElement {
    element(Target::Timeline, window, cx)
}

pub(super) fn popup(
    id: String,
    anchor: gpui::Bounds<gpui::Pixels>,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> gpui::AnyElement {
    element(Target::Popup { id, anchor }, window, cx)
}

impl Render for Leaf {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.clock = None;
        let mut delay = None;
        let mut observed = std::mem::take(&mut self.observed);
        let source_changed = std::mem::take(&mut self.source_changed);
        if source_changed {
            observed.clear();
        }
        let revision = self.revision;
        let root = self.root.clone();
        let content = self
            .root
            .update(cx, |v, cx| {
                let now = v.history.now();
                match &mut self.target {
                    Target::Row {
                        id,
                        group,
                        index,
                        selection,
                    } => {
                        let matches = |i: usize| {
                            v.history.row_entry(i).is_some_and(|e| &e.id == id)
                                && v.history.rows[i].activity.is_none() == *group
                        };
                        if !matches(*index) {
                            let Some(current) = (0..v.history.rows.len()).find(|i| matches(*i))
                            else {
                                return gpui::Empty.into_any_element();
                            };
                            *index = current;
                        }
                        let row = v.history.rows[*index];
                        let block = &v.history.projection.blocks[row.block];
                        let entry = v.history.row_entry(*index).unwrap();
                        delay = next_relative_tick(entry.start.or(entry.end), now);
                        if *group && source_changed {
                            observed.extend(
                                v.history.projection.activities[block.start..block.end]
                                    .iter()
                                    .map(|a| v.history.entries[a.entry].id.clone()),
                            );
                        } else if !*group {
                            if source_changed {
                                observed.push(id.clone());
                            }
                            if entry.state == "running" && entry.end.is_none() {
                                delay = Some(
                                    delay
                                        .unwrap_or(Duration::from_secs(1))
                                        .min(Duration::from_secs(1)),
                                );
                            }
                        }
                        v.render_history_activity(*index, now, *selection, cx)
                            .into_any_element()
                    }
                    Target::Timeline => {
                        if v.history
                            .entries
                            .iter()
                            .any(|e| e.state == "running" && e.end.is_none())
                        {
                            delay = Some(Duration::from_secs(1));
                        }
                        v.render_history_timeline_panel(window, cx)
                            .into_any_element()
                    }
                    Target::Popup { id, anchor } => {
                        let Some(entry) = v.history.entries.iter().find(|e| &e.id == id) else {
                            return gpui::Empty.into_any_element();
                        };
                        if source_changed {
                            observed.push(id.clone());
                        }
                        if entry.state == "running" && entry.end.is_none() {
                            delay = Some(Duration::from_secs(1));
                        }
                        let id = id.clone();
                        zork_ui::components::tooltip::sliding_popup(
                            "history-hover-popup",
                            format!("{id}-{revision}"),
                            *anchor,
                            284.,
                            move |_, cx| {
                                root.update(cx, |v, _| {
                                    v.history
                                        .entries
                                        .iter()
                                        .find(|e| e.id == id)
                                        .map(|e| v.render_history_span_detail(e).into_any_element())
                                        .unwrap_or_else(|| gpui::Empty.into_any_element())
                                })
                                .unwrap_or_else(|_| gpui::Empty.into_any_element())
                            },
                        )
                        .into_any_element()
                    }
                }
            })
            .unwrap_or_else(|_| gpui::Empty.into_any_element());
        self.observed = observed;
        #[cfg(feature = "headless-bench")]
        if self
            .root
            .read_with(cx, |v, _| v.history.fixed_now.is_some())
            .unwrap_or(true)
        {
            delay = None;
        }
        if let Some(delay) = delay {
            self.clock = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |v, cx| {
                    v.revision = v.revision.wrapping_add(1);
                    cx.notify();
                });
            }));
        }
        content
    }
}

fn next_relative_tick(timestamp: Option<i64>, now: i64) -> Option<Duration> {
    let age = now.saturating_sub(timestamp?);
    let wait = if age < 0 {
        age.saturating_neg()
    } else if age < 5000 {
        5000 - age
    } else {
        let unit = match age {
            0..=59999 => 1000,
            60000..=3599999 => 60000,
            3600000..=86399999 => 3600000,
            _ => 86400000,
        };
        unit - age % unit
    };
    Some(Duration::from_millis(wait.max(1) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_clock_wakes_at_the_next_visible_boundary() {
        for (age, delay) in [
            (-1000, 1000),
            (0, 5000),
            (4900, 100),
            (5500, 500),
            (59999, 1),
            (61000, 59000),
            (3600000, 3600000),
        ] {
            assert_eq!(
                next_relative_tick(Some(0), age),
                Some(Duration::from_millis(delay))
            );
        }
        assert_eq!(next_relative_tick(None, 1000), None);
    }
}
