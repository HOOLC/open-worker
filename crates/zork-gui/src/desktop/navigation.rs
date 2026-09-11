//! Device → Leader → Task navigation, shared by every retained device view.
use super::{
    store::{ClientStore, SavedNode},
    ui,
};
use crate::{
    api::ProductTask,
    automation::{AutomationElementExt, AutomationRole},
    design::CUE_UI,
    i18n::Locale,
    shell::ShellRoute,
};
use gpui::{div, prelude::*, px, rgb, Context, Div, FontWeight, Window};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use zork_client_core::preferences::{read_view_state, save_view_state, ViewState};

#[derive(Clone)]
#[allow(dead_code)] // Compatibility routes remain available to non-desktop clients.
pub enum Destination {
    Home,
    Conversation {
        session: String,
        leader: Option<String>,
    },
    Leader(String),
    PrepareDevice(String),
    Task {
        leader: String,
        session: String,
    },
    Page(ShellRoute),
    Manage(usize),
}
#[derive(Clone)]
pub struct Navigate {
    pub node: Option<String>,
    pub destination: Destination,
}
#[derive(Clone, Default, PartialEq)]
pub struct Selection {
    pub selected_leader: Option<String>,
    pub selected_session: Option<String>,
    pub route: Option<ShellRoute>,
    pub reading_tail: bool,
}
struct Device {
    node: SavedNode,
    data: Arc<zork_client_core::state::NavigationData>,
    selection: Selection,
    core: Option<Arc<zork_client_core::state::Device>>,
    subscription: Option<gpui::Task<()>>,
    notification_subscription: Option<gpui::Task<()>>,
}
pub struct DeviceNavigation {
    store: Arc<ClientStore>,
    regions: zork_ui::components::region::Regions<Self>,
    devices: Vec<Device>,
    active: Option<String>,
    collapsed: HashSet<String>,
    show_all: HashSet<String>,
    scroll: gpui::ScrollHandle,
    locale: Locale,
    width: f32,
    pub resizing: bool,
    brand: Option<gpui::Entity<crate::components::brand::Brand>>,
    viewing: bool,
    tabs: TabGroup,
    details_overlay: Option<gpui::Entity<zork_ui::components::tooltip::DetailsOverlay>>,
}
impl gpui::EventEmitter<Navigate> for DeviceNavigation {}
impl DeviceNavigation {
    #[cfg(feature = "headless-bench")]
    pub(crate) fn benchmark_region_counts(
        &self,
        cx: &gpui::App,
    ) -> std::collections::HashMap<String, [usize; 4]> {
        self.regions.counters(cx)
    }
    pub fn new(store: Arc<ClientStore>, nodes: &[SavedNode], cx: &mut gpui::App) -> Self {
        let collapsed = read_view_state(&store, "device", ViewState::NavigationCollapsed)
            .ok()
            .flatten()
            .unwrap_or_default();
        let width = read_view_state::<f32>(&store, "device", ViewState::SidebarWidth)
            .ok()
            .flatten()
            .unwrap_or(crate::design::DEVICE_SIDEBAR_WIDTH)
            .clamp(200., 420.);
        let mut view = Self {
            store,
            regions: Default::default(),
            width,
            resizing: false,
            brand: None,
            viewing: true,
            details_overlay: None,
            tabs: TabGroup::new(cx),
            devices: Vec::new(),
            active: None,
            collapsed,
            show_all: HashSet::new(),
            scroll: gpui::ScrollHandle::new(),
            locale: crate::i18n::load_locale(
                &crate::i18n::preferences_path(),
                std::env::var("ZORK_GUI_LOCALE").ok().as_deref(),
            ),
        };
        view.set_nodes(nodes);
        view
    }
    pub fn width(&self, available: f32) -> f32 {
        self.width.min((available - 360.).max(200.))
    }
    pub fn resize(&mut self, x: f32, available: f32, cx: &mut Context<Self>) {
        if self.resizing {
            self.width = x.clamp(200., (available - 360.).clamp(200., 420.));
            zork_ui::components::region::invalidate_all(cx);
        }
    }
    pub fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if self.resizing {
            self.resizing = false;
            let _ = save_view_state(&self.store, "device", ViewState::SidebarWidth, &self.width);
            zork_ui::components::region::invalidate_all(cx);
        }
    }
    pub fn set_nodes(&mut self, nodes: &[SavedNode]) {
        self.devices
            .retain(|d| nodes.iter().any(|n| n.id == d.node.id));
        for node in nodes {
            if let Some(device) = self.devices.iter_mut().find(|d| d.node.id == node.id) {
                device.node = node.clone();
                continue;
            }
            self.devices.push(Device {
                node: node.clone(),
                data: Arc::new(Default::default()),
                selection: Selection::default(),
                core: None,
                subscription: None,
                notification_subscription: None,
            });
        }
    }
    pub fn update_nodes(&mut self, nodes: &[SavedNode], cx: &mut Context<Self>) {
        for device in &self.devices {
            if !nodes.iter().any(|node| node.id == device.node.id) {
                if let Some(core) = &device.core {
                    let ledger = core.notifications().snapshot();
                    for tag in ledger.pending.keys().chain(ledger.presented.keys()) {
                        cx.dismiss_system_notification(tag);
                    }
                }
            }
        }
        self.set_nodes(nodes);
        let names = self
            .devices
            .iter()
            .map(|device| format!("device/{}", device.node.id))
            .collect::<HashSet<_>>();
        self.regions
            .retain(|name| !name.starts_with("device/") || names.contains(name));
        if names.is_empty() {
            cx.notify();
        } else {
            let names = names.iter().map(String::as_str).collect::<Vec<_>>();
            zork_ui::components::region::invalidate(cx, &names);
        }
    }
    pub fn bind_node(
        &mut self,
        id: &str,
        core: Arc<zork_client_core::state::Device>,
        cx: &mut Context<Self>,
    ) {
        let Some(device) = self.devices.iter_mut().find(|d| d.node.id == id) else {
            return;
        };
        if device
            .core
            .as_ref()
            .is_some_and(|existing| Arc::ptr_eq(existing, &core))
        {
            return;
        }
        let mut subscription = core.navigation();
        device.data = subscription.snapshot();
        device.core = Some(core.clone());
        let notification_core = core.clone();
        let notification_store = self.store.clone();
        let mut notifications = core.notifications();
        device.notification_subscription = Some(cx.spawn(async move |this, cx| {
            let mut previous = notifications
                .snapshot()
                .presented
                .keys()
                .cloned()
                .collect::<HashSet<_>>();
            let mut retries = 0;
            loop {
                // Coalesce a burst of catalog changes before crossing the OS boundary.
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                notification_core.refresh_notifications();
                let mut ledger = notifications.snapshot();
                let mut failed = false;
                let permission = if ledger.pending.is_empty() {
                    super::notifications::Permission::Allowed
                } else {
                    super::notifications::permission(true)
                        .await
                        .unwrap_or_else(|error| {
                            eprintln!("notification permission: {error}");
                            failed = true;
                            super::notifications::Permission::Unknown
                        })
                };
                // Permission UI may have been open while the user read/muted this chat.
                notification_core.refresh_notifications();
                ledger = notifications.snapshot();
                let Ok(locale) = this.update(cx, |view, cx| {
                    let current = ledger
                        .pending
                        .keys()
                        .chain(ledger.presented.keys())
                        .cloned()
                        .collect::<HashSet<_>>();
                    for tag in previous.difference(&current) {
                        cx.dismiss_system_notification(tag);
                    }
                    previous = current;
                    view.locale
                }) else {
                    return;
                };
                for notice in ledger.pending.values() {
                    if matches!(
                        permission,
                        super::notifications::Permission::Denied
                            | super::notifications::Permission::Unavailable
                    ) {
                        if let Err(error) = notification_core.discard_notification(&notice.id) {
                            eprintln!("notification suppression: {error}");
                            failed = true;
                        }
                        continue;
                    }
                    if permission != super::notifications::Permission::Allowed {
                        continue;
                    }
                    #[cfg(target_os = "macos")]
                    match super::notifications::accepted(&notice.tag(), &notice.id).await {
                        Ok(true) => {
                            if let Err(error) =
                                notification_core.acknowledge_notification(&notice.id)
                            {
                                eprintln!("notification recovery: {error}");
                                failed = true;
                            }
                            continue;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("notification recovery: {error}");
                            failed = true;
                            continue;
                        }
                    }
                    // Recheck privacy and visibility after all asynchronous OS queries.
                    notification_core.refresh_notifications();
                    if !notification_core
                        .notifications()
                        .snapshot()
                        .pending
                        .values()
                        .any(|n| n.id == notice.id)
                    {
                        continue;
                    }
                    let prefs =
                        match zork_client_core::notifications::preferences(&notification_store) {
                            Ok(prefs) => prefs,
                            Err(error) => {
                                eprintln!("notification preferences: {error}");
                                failed = true;
                                continue;
                            }
                        };
                    let key = match notice.kind {
                        zork_client_core::notifications::Kind::Reply => "notification_reply",
                        zork_client_core::notifications::Kind::Review => "notification_review",
                        zork_client_core::notifications::Kind::Attention => {
                            "notification_attention"
                        }
                    };
                    let notification = gpui::SystemNotification {
                        tag: notice.tag().into(),
                        title: if prefs.preview && !notice.title.is_empty() {
                            notice.title.clone().into()
                        } else {
                            "Zork".into()
                        },
                        body: locale.text(key).into(),
                        actions: vec![],
                    };
                    #[cfg(target_os = "macos")]
                    let result =
                        super::notifications::post(notification, prefs.sound, &notice.id).await;
                    #[cfg(not(target_os = "macos"))]
                    let result = this.update(cx, |_, cx| cx.show_system_notification(notification));
                    match result {
                        Ok(()) => {
                            if let Err(error) =
                                notification_core.acknowledge_notification(&notice.id)
                            {
                                eprintln!("notification receipt: {error}");
                                failed = true;
                            }
                        }
                        Err(error) => {
                            eprintln!("notification delivery: {error}");
                            failed = true;
                        }
                    }
                }
                if failed && retries < 3 {
                    retries += 1;
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1 << retries))
                        .await;
                    continue;
                }
                retries = 0;
                if notifications.changed().await.is_none() {
                    return;
                }
            }
        }));
        let region = format!("device/{id}");
        let id = id.to_owned();
        device.subscription = Some(cx.spawn(async move |this, cx| {
            while let Some(data) = subscription.changed().await {
                if this
                    .update(cx, |view, cx| {
                        if let Some(device) = view.devices.iter_mut().find(|d| d.node.id == id) {
                            device.data = data;
                            zork_ui::components::region::invalidate(cx, &[&format!("device/{id}")]);
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
        zork_ui::components::region::invalidate(cx, &[&region]);
    }
    pub fn set_selection(
        &mut self,
        id: &str,
        selection: Selection,
        locale: Locale,
        cx: &mut Context<Self>,
    ) {
        let Some(device) = self.devices.iter_mut().find(|d| d.node.id == id) else {
            return;
        };
        let changed = device.selection != selection
            || (self.active.as_deref() == Some(id) && self.locale != locale);
        device.selection = selection;
        if self.active.as_deref() == Some(id) {
            self.locale = locale;
        }
        self.mark_viewed();
        if changed {
            zork_ui::components::region::invalidate_all(cx);
        }
    }
    #[cfg(feature = "headless-bench")]
    pub fn set_preview(
        &mut self,
        id: &str,
        data: zork_client_core::state::NavigationData,
        selection: Selection,
        locale: Locale,
        cx: &mut Context<Self>,
    ) {
        if let Some(device) = self.devices.iter_mut().find(|d| d.node.id == id) {
            device.data = Arc::new(data);
        }
        self.set_selection(id, selection, locale, cx);
    }
    pub fn activate(&mut self, id: &str, cx: &mut Context<Self>) {
        self.active = Some(id.to_owned());
        self.mark_viewed();
        zork_ui::components::region::invalidate_all(cx);
    }
    pub fn set_viewing(&mut self, viewing: bool, cx: &mut Context<Self>) {
        if self.viewing != viewing {
            self.viewing = viewing;
            self.mark_viewed();
            zork_ui::components::region::invalidate_all(cx);
        }
    }
    fn mark_viewed(&self) {
        for device in &self.devices {
            if let Some(core) = &device.core {
                core.report_view(
                    device.selection.selected_session.clone(),
                    self.viewing
                        && self.active.as_ref() == Some(&device.node.id)
                        && matches!(device.selection.route, Some(ShellRoute::Task(_))),
                    device.selection.reading_tail,
                );
            }
        }
    }
    fn unread<'a>(&self, device: &'a Device) -> &'a HashSet<String> {
        &device.data.unread
    }
    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        if let Err(error) = save_view_state(
            &self.store,
            "device",
            ViewState::NavigationCollapsed,
            &self.collapsed,
        ) {
            eprintln!("Could not save device navigation state: {error}");
        }
        zork_ui::components::region::invalidate_all(cx);
    }
    fn go(&self, node: Option<String>, destination: Destination, cx: &mut Context<Self>) {
        cx.emit(Navigate { node, destination });
    }
    fn brand(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let brand = self
            .brand
            .get_or_insert_with(|| {
                cx.new(|_| {
                    crate::components::brand::Brand::new(
                        crate::components::brand::BrandMotion::Header,
                        CUE_UI.palette.sidebar,
                    )
                })
            })
            .clone();
        div()
            .id("zork-brand")
            .h(px(48.))
            .pl(px(64.))
            .mb_2()
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.start_window_move()
            })
            .child(brand)
            .automation(AutomationRole::Status, "Zork")
            .into_any_element()
    }
    fn device(&self, device: &Device, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let node = &device.node;
        let expanded = !self.collapsed.contains(&node.id);
        let fold = zork_ui::components::collapse::Collapse::new(
            format!("device-fold-{}", node.id),
            expanded,
            self.width(window.viewport_size().width.as_f32()) - 16.,
            window,
            cx,
        );
        let header_focus = fold.header_focus(cx);
        let interactive = fold.interactive(cx);
        let key = node.id.clone();
        let state = self.locale.text(match device.data.online {
            Some(true) => "device_online",
            Some(false) => "device_offline",
            None => "device_not_connected",
        });
        let unread = self.unread(device);
        let mut leaders: Vec<_> = device.data.agents.iter().collect();
        leaders.sort_by_key(|a| {
            !a["session_id"]
                .as_str()
                .is_some_and(|id| unread.contains(id))
                && !device
                    .data
                    .tasks
                    .get(a["id"].as_str().unwrap_or_default())
                    .is_some_and(|tasks| {
                        tasks
                            .iter()
                            .any(|t| t.session_id.as_ref().is_some_and(|id| unread.contains(id)))
                    })
        });
        self.tabs
            .column()
            .gap_0()
            .pb(px(2.))
            .child(
                self.tabs
                    .tab(format!("device-{}", node.id), false)
                    .track_focus(&header_focus)
                    .child(ui::icon("icons/node.svg", 20.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .font_weight(FontWeight::MEDIUM)
                            .child(node.name.clone()),
                    )
                    .child(
                        div()
                            .size(px(5.))
                            .rounded_full()
                            .bg(rgb(match device.data.online {
                                Some(true) if device.data.route.direct => CUE_UI.palette.success,
                                Some(true) => 0xD9A023,
                                _ => CUE_UI.palette.subtle,
                            })),
                    )
                    .when(
                        device.data.online == Some(true)
                            && device.data.route.scope == crate::api::ConnectionScope::Public,
                        |row| {
                            row.child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(CUE_UI.palette.muted))
                                    .child(self.locale.text("device_public_network")),
                            )
                        },
                    )
                    .on_click(cx.listener(move |v, _, _, cx| v.toggle(key.clone(), cx)))
                    .automation(AutomationRole::Button, format!("{} · {state}", node.name)),
            )
            .child({
                let content = fold.mounted(cx).then(|| {
                    self.tabs
                        .column()
                        .when(!leaders.is_empty(), |v| {
                            v.pt(px(zork_ui::navigation::TAB_GAP))
                        })
                        .children(
                            leaders
                                .iter()
                                .map(|a| self.leader(device, a, interactive, cx)),
                        )
                        .into_any_element()
                });
                let owner = cx.entity().downgrade();
                let region = format!("device/{}", node.id);
                fold.element(
                    content,
                    move |_, cx| {
                        let _ = owner.update(cx, |_, cx| {
                            zork_ui::components::region::invalidate(cx, &[&region]);
                        });
                    },
                    cx,
                )
            })
    }
    fn leader(
        &self,
        device: &Device,
        agent: &Value,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = agent["id"].as_str().unwrap_or_default();
        let name = agent["name"].as_str().unwrap_or("领队");
        let key = format!("{}/{id}", device.node.id);
        let active = self.active.as_deref() == Some(&device.node.id);
        let current = active && device.selection.selected_leader.as_deref() == Some(id);
        let chatting = active
            && agent["session_id"].as_str() == device.selection.selected_session.as_deref()
            && matches!(device.selection.route, Some(ShellRoute::Task(_)));
        let tasks = device
            .data
            .tasks
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let all = self.show_all.contains(&key);
        let unread = self.unread(device);
        let visible = ordered_visible_tasks(
            tasks,
            all,
            device.selection.selected_session.as_deref(),
            &unread,
        );
        let leader_id = id.to_owned();
        let node_id = device.node.id.clone();
        let details = zork_ui::components::tooltip::DetailsTooltip {
            key: format!("leader-{key}"),
            title: name.to_owned(),
            kind: "领队".into(),
            avatar: agent["avatar"].as_str().map(str::to_owned),
            description: agent["instructions"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            rows: vec![
                ("设备".into(), device.node.name.clone()),
                (
                    "连接".into(),
                    agent["profile_id"].as_str().unwrap_or_default().to_owned(),
                ),
                (
                    "模型".into(),
                    agent["model"].as_str().unwrap_or_default().to_owned(),
                ),
            ],
        };
        self.tabs
            .column()
            .child(
                self.tabs
                    .tab(format!("leader-{}-{id}", device.node.id), chatting)
                    .tab_stop(interactive)
                    .child(ui::agent_avatar(agent["avatar"].as_str(), 20.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(name.to_owned()),
                    )
                    .when(
                        agent["session_id"]
                            .as_str()
                            .is_some_and(|id| unread.contains(id)),
                        |v| {
                            v.child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(rgb(CUE_UI.palette.text)),
                            )
                        },
                    )
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.go(
                            Some(node_id.clone()),
                            Destination::Leader(leader_id.clone()),
                            cx,
                        )
                    }))
                    .automation(
                        AutomationRole::Button,
                        if current {
                            format!("{name} · {}", self.locale.text("device_current_leader"))
                        } else {
                            name.to_owned()
                        },
                    )
                    .map(|row| {
                        zork_ui::components::tooltip::trigger(
                            row,
                            details,
                            self.details_overlay.as_ref().unwrap().clone(),
                        )
                    }),
            )
            .map(|panel| {
                panel
                    .children(visible.iter().map(|task| {
                        let selected = active
                            && task.session_id.is_some()
                            && task.session_id == device.selection.selected_session
                            && matches!(device.selection.route, Some(ShellRoute::Task(_)));
                        let title = if task.title.is_empty() {
                            self.locale.text("device_untitled_task").to_owned()
                        } else {
                            task.title.clone()
                        };
                        let node = device.node.id.clone();
                        let leader = id.to_owned();
                        let session = task.session_id.clone();
                        let label = task
                            .mesh
                            .as_ref()
                            .and_then(|mesh| device.data.executors.get(&mesh.executor_origin))
                            .map(|(name, _)| format!("{title} · {name}"))
                            .unwrap_or_else(|| title.clone());
                        let mut detail_rows = vec![
                            ("领队".into(), name.to_owned()),
                            (
                                "状态".into(),
                                self.locale.text(task_status(task)).to_owned(),
                            ),
                            ("工作目录".into(), task.workspace.clone()),
                        ];
                        if let Some((executor, _)) = task
                            .mesh
                            .as_ref()
                            .and_then(|m| device.data.executors.get(&m.executor_origin))
                        {
                            detail_rows.push(("执行设备".into(), executor.clone()));
                        }
                        let details = zork_ui::components::tooltip::DetailsTooltip {
                            key: format!("task-{}-{}", device.node.id, task.task_id),
                            title: title.clone(),
                            kind: "Task".into(),
                            avatar: None,
                            description: task.goal.clone(),
                            rows: detail_rows,
                        };
                        self.tabs
                            .tab(
                                format!("leader-task-{}-{}", device.node.id, task.task_id),
                                selected,
                            )
                            .pl(px(36.))
                            .tab_stop(interactive && session.is_some())
                            .when(session.is_none(), |r| r.opacity(0.5))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .child(title.clone()),
                            )
                            .when(
                                task.session_id
                                    .as_ref()
                                    .is_some_and(|id| unread.contains(id)),
                                |v| {
                                    v.child(
                                        div()
                                            .size(px(6.))
                                            .rounded_full()
                                            .bg(rgb(CUE_UI.palette.text)),
                                    )
                                },
                            )
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let Some(session) = &session {
                                    v.go(
                                        Some(node.clone()),
                                        Destination::Task {
                                            leader: leader.clone(),
                                            session: session.clone(),
                                        },
                                        cx,
                                    );
                                }
                            }))
                            .automation_enabled(
                                task.session_id.is_some(),
                                AutomationRole::Button,
                                label,
                            )
                            .map(|row| {
                                zork_ui::components::tooltip::trigger(
                                    row,
                                    details,
                                    self.details_overlay.as_ref().unwrap().clone(),
                                )
                            })
                    }))
                    .when(tasks.len() > visible.len() || all, |panel| {
                        panel.child(
                            self.tabs
                                .tab(format!("leader-more-{key}"), false)
                                .tab_stop(interactive)
                                .pl(px(36.))
                                .text_color(rgb(CUE_UI.palette.muted))
                                .child(self.locale.text(if all {
                                    "device_fewer_tasks"
                                } else {
                                    "device_more_tasks"
                                }))
                                .on_click(cx.listener(move |v, _, _, cx| {
                                    if !v.show_all.remove(&key) {
                                        v.show_all.insert(key.clone());
                                    }
                                    zork_ui::components::region::invalidate_all(cx);
                                })),
                        )
                    })
            })
    }
}
use crate::api::TaskState;
pub(crate) use zork_ui::navigation::TabGroup;
fn task_status(task: &ProductTask) -> &'static str {
    match task.state {
        TaskState::Completed => "task_completed",
        TaskState::Cancelled => "task_cancelled",
        _ if matches!(task.last_run_status.as_deref(), Some("running" | "working")) => {
            "task_working"
        }
        TaskState::Review => "task_review",
        _ if task
            .mesh
            .as_ref()
            .is_some_and(|m| m.state == "needs_attention") =>
        {
            "task_blocked"
        }
        TaskState::Open => match task.last_run_status.as_deref() {
            Some("failed" | "interrupted" | "cancelled") => "task_blocked",
            _ => "task_open",
        },
    }
}
#[cfg(test)]
fn visible_tasks<'a>(
    tasks: &'a [ProductTask],
    all: bool,
    selected: Option<&str>,
) -> Vec<&'a ProductTask> {
    ordered_visible_tasks(tasks, all, selected, &HashSet::new())
}
fn ordered_visible_tasks<'a>(
    tasks: &'a [ProductTask],
    all: bool,
    selected: Option<&str>,
    unread: &HashSet<String>,
) -> Vec<&'a ProductTask> {
    let mut sorted: Vec<_> = tasks.iter().collect();
    sorted.sort_by(|a, b| {
        let a_unread = a.session_id.as_ref().is_some_and(|id| unread.contains(id));
        let b_unread = b.session_id.as_ref().is_some_and(|id| unread.contains(id));
        b_unread
            .cmp(&a_unread)
            .then_with(|| a.state.is_closed().cmp(&b.state.is_closed()))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    if !all {
        let mut closed = 0;
        sorted.retain(|task| {
            if task.state.is_closed() {
                closed += 1;
            }
            !task.state.is_closed()
                || task
                    .session_id
                    .as_ref()
                    .is_some_and(|id| unread.contains(id))
                || closed <= 3
                || (task.session_id.is_some() && task.session_id.as_deref() == selected)
        });
    }
    sorted
}
impl Render for DeviceNavigation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay = self
            .details_overlay
            .get_or_insert_with(|| cx.new(|_| Default::default()))
            .clone();
        let width = self.width(window.viewport_size().width.as_f32()) - 16.;
        let ids = self
            .devices
            .iter()
            .map(|d| d.node.id.clone())
            .collect::<Vec<_>>();
        let mut names = ids
            .iter()
            .map(|id| format!("device/{id}"))
            .collect::<HashSet<_>>();
        names.insert("footer".into());
        self.regions.retain(|key| names.contains(key));
        let rows = ids
            .into_iter()
            .map(|id| {
                self.regions.auto_height(
                    &format!("device/{id}"),
                    width,
                    cx,
                    move |v, window, cx| {
                        v.devices
                            .iter()
                            .find(|d| d.node.id == id)
                            .map(|d| v.device(d, window, cx).into_any_element())
                            .unwrap_or_else(|| gpui::Empty.into_any_element())
                    },
                )
            })
            .collect::<Vec<_>>();
        let footer = self.regions.auto_height("footer", width, cx, |v, _, cx| {
            v.render_footer(cx).into_any_element()
        });
        let brand = self.brand(cx);
        let tabs = self.tabs.clone();
        tabs.surface(
            div()
                .id("device-sidebar")
                .relative()
                .w(px(self.width(window.viewport_size().width.as_f32())))
                .h_full()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .px_2()
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .h_full()
                        .w(px(5.))
                        .id("device-sidebar-resize")
                        .cursor_col_resize()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|v, _, _, cx| {
                                v.resizing = true;
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(brand)
                .child(
                    div()
                        .id("device-sidebar-scroll")
                        .track_scroll(&self.scroll)
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(rows),
                )
                .child(footer)
                .child(overlay),
        )
    }
}

impl DeviceNavigation {
    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        self.tabs
            .column()
            .py_2()
            .child(
                self.tabs
                    .tab("device-add".into(), false)
                    .child(ui::icon("icons/plus.svg", 20.))
                    .child(self.locale.text("device_add"))
                    .on_click(cx.listener(|v, _, _, cx| v.go(None, Destination::Manage(3), cx)))
                    .automation(AutomationRole::Button, self.locale.text("device_add")),
            )
            .child(
                self.tabs
                    .tab("desktop-manage".into(), false)
                    .child(ui::icon("icons/settings.svg", 20.))
                    .child(self.locale.text("nav_settings"))
                    .on_click(
                        cx.listener(|v, _, _, cx| {
                            v.go(v.active.clone(), Destination::Manage(4), cx)
                        }),
                    )
                    .automation(AutomationRole::Button, self.locale.text("nav_settings")),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TaskState;

    fn task(id: &str, state: TaskState) -> ProductTask {
        serde_json::from_value(serde_json::json!({
            "task_id":id,"session_id":id,"conversation_id":id,"title":id,"workspace":"",
            "state":state,"revision":1,"result_message_id":null,"result_text":null,
            "last_run_status":null,"run_count":0,"created_at":id,"updated_at":id
        }))
        .unwrap()
    }

    #[test]
    fn history_limit_keeps_every_open_task_and_the_selected_old_task() {
        let mut tasks: Vec<_> = (0..8)
            .map(|i| task(&format!("closed-{i}"), TaskState::Completed))
            .collect();
        tasks.push(task("active", TaskState::Open));
        tasks.push(task("review", TaskState::Review));
        let visible = visible_tasks(&tasks, false, Some("closed-0"));
        let ids: Vec<_> = visible.iter().map(|t| t.task_id.as_str()).collect();
        assert_eq!(
            ids,
            ["review", "active", "closed-7", "closed-6", "closed-5", "closed-0"]
        );
        assert_eq!(visible_tasks(&tasks, true, None).len(), tasks.len());
    }

    #[test]
    fn execution_does_not_override_a_closed_task_but_does_override_old_review() {
        let mut task = task("task", TaskState::Review);
        task.last_run_status = Some("running".into());
        assert_eq!(task_status(&task), "task_working");
        task.state = TaskState::Completed;
        assert_eq!(task_status(&task), "task_completed");
        task.state = TaskState::Open;
        task.last_run_status = Some("cancelled".into());
        assert_eq!(task_status(&task), "task_blocked");
    }
    #[test]
    fn unread_tasks_precede_newer_read_items_and_are_not_hidden_as_old_history() {
        let mut tasks = (0..8)
            .map(|i| task(&format!("closed-{i}"), TaskState::Completed))
            .collect::<Vec<_>>();
        tasks.push(task("open", TaskState::Open));
        let unread = HashSet::from(["closed-0".to_owned()]);
        let ordered = ordered_visible_tasks(&tasks, false, None, &unread);
        assert_eq!(ordered[0].task_id, "closed-0");
        assert_eq!(ordered[1].task_id, "open");
    }
}
