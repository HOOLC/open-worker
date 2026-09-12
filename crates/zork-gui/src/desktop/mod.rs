//! Desktop presentation of the core-owned device directory and host operations.
pub mod account;
mod agents;
pub mod client_settings;
mod mesh_settings;
pub(crate) mod navigation;
pub mod node;
mod notifications;
pub(crate) mod profile_quota;
mod profiles;
mod resources;
pub mod store;
pub mod transport;
pub(crate) mod ui;
#[cfg(feature = "headless-bench")]
pub use agents::AgentsView as HeadlessAgentsView;
#[cfg(feature = "headless-bench")]
pub use profiles::ProfilesView as HeadlessProfilesView;
#[cfg(feature = "headless-bench")]
pub use resources::ResourcesView as HeadlessResourcesView;

use crate::components::text_input::ComposerInput;
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    design::CUE_UI,
    views::RootView,
};
use gpui::{div, prelude::*, px, rgb, Context, Div, Entity, FontWeight, Window};
use node::LocalNode;
use std::sync::Arc;
use store::{ClientStore, SavedNode};
use zork_client_core::desktop::directory::{Directory, DirectoryData};

pub use zork_client_core::desktop::{client_root, load_services};

pub struct DesktopRoot {
    source: Arc<Directory>,
    directory_updates: Option<gpui::Task<()>>,
    store: Arc<ClientStore>,
    local: Arc<LocalNode>,
    local_enabled: bool,
    mesh_identity: Option<String>,
    pairing: bool,
    rename_input: Entity<ComposerInput>,
    rename_node_id: Option<String>,
    rename_busy: bool,
    rename_error: Option<String>,
    rename_modal: ui::ModalState,
    remote_name: Entity<ComposerInput>,
    remote_origin: Entity<ComposerInput>,
    remote_addr: Entity<ComposerInput>,
    nodes: Vec<SavedNode>,
    active: Option<Entity<RootView>>,
    active_node_id: Option<String>,
    node_views: std::collections::HashMap<String, (u64, Entity<RootView>)>,
    navigation: Entity<navigation::DeviceNavigation>,
    pending_navigation: Option<navigation::Navigate>,
    pending_notification: Option<String>,
    profiles: Option<Entity<profiles::ProfilesView>>,
    resources: Option<Entity<resources::ResourcesView>>,
    resource_inspector: Option<Entity<resources::ResourcesView>>,
    service_views: std::collections::HashMap<String, (u64, Entity<resources::ResourcesView>)>,
    applications: Arc<Vec<zork_client_core::pages::ApplicationEntry>>,
    management_views: std::collections::HashMap<
        String,
        (
            u64,
            Entity<agents::AgentsView>,
            Entity<profiles::ProfilesView>,
            Entity<mesh_settings::MeshSettings>,
        ),
    >,
    device_info: std::collections::HashMap<String, serde_json::Value>,
    agents: Option<Entity<agents::AgentsView>>,
    management_tab: usize,
    mesh_settings: Option<Entity<mesh_settings::MeshSettings>>,
    active_node_name: Option<String>,
    identity: Option<account::AccountIdentity>,
    client_settings: client_settings::State,
    settings_tabs: navigation::TabGroup,
    account_busy: bool,
    opened_account_url: Option<String>,
    managing: bool,
    add_device_open: bool,
    dialog_focus: gpui::FocusHandle,
    device_switch_focus: [gpui::FocusHandle; 2],
    notification_switch_focus: [gpui::FocusHandle; 4],
    add_device_modal: ui::ModalState,
    activation_observed: bool,
    busy: bool,
    error: Option<String>,
}
impl DesktopRoot {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let root = client_root();
        let source = Directory::open(&root).expect("open device-local client database");
        let store = source.store.clone();
        let snapshot = source.snapshot();
        let client_settings = client_settings::State {
            message_preview_height: client_settings::load_message_preview_height(&store),
            locale: crate::i18n::load_locale(
                &crate::i18n::preferences_path(),
                std::env::var("ZORK_GUI_LOCALE").ok().as_deref(),
            ),
            account_available: snapshot.account_available,
            ..Default::default()
        };
        let nodes = snapshot.nodes.as_ref().clone();
        let local_enabled = snapshot.local_enabled;
        let startup_error = snapshot.error.clone();
        let local = source.local.clone();
        let identity = snapshot.account.clone();
        let transport = source.transport.clone();
        let mut field = |label| {
            let input = cx.new(|cx| ComposerInput::new(label, cx));
            cx.observe(&input, |_, _, cx| cx.notify()).detach();
            input
        };
        let rename_input = field("设备名称");
        let remote_name = field("设备名称");
        let remote_origin = field("目标设备 key: 身份");
        let remote_addr = field("局域网地址（可选），例如 192.168.1.20:43120");
        let transport_shutdown = transport.clone();
        let shutdown = local.clone();
        cx.on_app_quit(move |view, _| {
            let local_closed = shutdown.shutdown();
            let transport_closed = transport_shutdown.shutdown();
            view.source.cancel_account();
            async move {
                let _ = local_closed.await;
                let _ = transport_closed.await;
            }
        })
        .detach();
        let navigation = cx.new(|cx| navigation::DeviceNavigation::new(store.clone(), &nodes, cx));
        cx.subscribe(&navigation, |v, _, action: &navigation::Navigate, cx| {
            v.navigate_device(action.clone(), cx);
        })
        .detach();
        let notification_root = cx.weak_entity();
        cx.on_system_notification_response(move |response, cx| {
            let _ = notification_root.update(cx, |view, cx| {
                view.open_notification(response.tag.to_string(), cx);
            });
        });
        let mut view = Self {
            source,
            directory_updates: None,
            store,
            local,
            local_enabled,
            mesh_identity: None,
            pairing: false,
            rename_input,
            rename_node_id: None,
            rename_busy: false,
            rename_error: None,
            rename_modal: ui::ModalState::new(cx),
            remote_name,
            remote_origin,
            remote_addr,
            nodes,
            active: None,
            active_node_id: None,
            node_views: std::collections::HashMap::new(),
            navigation,
            pending_navigation: None,
            pending_notification: None,
            profiles: None,
            resources: None,
            resource_inspector: None,
            service_views: Default::default(),
            applications: snapshot.applications.clone(),
            management_views: Default::default(),
            device_info: Default::default(),
            agents: None,
            management_tab: 0,
            mesh_settings: None,
            active_node_name: None,
            identity,
            client_settings,
            settings_tabs: navigation::TabGroup::new(cx),
            account_busy: false,
            opened_account_url: None,
            managing: true,
            add_device_open: false,
            dialog_focus: cx.focus_handle(),
            device_switch_focus: [cx.focus_handle(), cx.focus_handle()],
            notification_switch_focus: std::array::from_fn(|_| cx.focus_handle()),
            add_device_modal: ui::ModalState::new(cx),
            activation_observed: false,
            busy: false,
            error: startup_error,
        };
        view.watch_directory(cx);
        let source = view.source.clone();
        let work = cx
            .background_executor()
            .spawn(async move { source.restore() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(Some(node)) => view.open_node(node, cx),
                    Ok(None) => {}
                    Err(e) => view.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        view
    }
    fn watch_directory(&mut self, cx: &mut Context<Self>) {
        let mut updates = self.source.subscribe();
        self.apply_directory(updates.snapshot(), cx);
        self.directory_updates = Some(cx.spawn(async move |this, cx| {
            while let Some(snapshot) = updates.changed().await {
                if this
                    .update(cx, |view, cx| view.apply_directory(snapshot, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }
    fn apply_directory(&mut self, snapshot: Arc<DirectoryData>, cx: &mut Context<Self>) {
        if !Arc::ptr_eq(&self.applications, &snapshot.applications) {
            self.applications = snapshot.applications.clone();
            for (_, root) in self.node_views.values() {
                root.update(cx, |root, cx| {
                    root.set_applications(self.applications.clone(), cx)
                });
            }
        }
        let nodes_changed = self.nodes != *snapshot.nodes;
        self.nodes = snapshot.nodes.as_ref().clone();
        self.local_enabled = snapshot.local_enabled;
        self.mesh_identity = snapshot.mesh_identity.clone();
        self.identity = snapshot.account.clone();
        self.account_busy = snapshot.account_busy;
        if self.opened_account_url != snapshot.account_url {
            self.opened_account_url = snapshot.account_url.clone();
            if let Some(url) = &snapshot.account_url {
                cx.open_url(url);
            }
        }
        self.device_info = snapshot.info.as_ref().clone();
        if self.client_settings.message_preview_height
            != snapshot.preferences.message_preview_height
        {
            let height = snapshot.preferences.message_preview_height;
            self.client_settings.message_preview_height = height;
            for (_, root) in self.node_views.values() {
                root.update(cx, |root, cx| root.set_message_preview_height(height, cx));
            }
        }
        if let Some(error) = &snapshot.error {
            self.error = Some(error.clone());
        }
        if nodes_changed {
            self.navigation
                .update(cx, |nav, cx| nav.update_nodes(&self.nodes, cx));
            for node in self.nodes.clone() {
                self.apply_device_name(&node.id, &node.name, cx);
                self.ensure_node_view(&node, cx);
            }
        }
        cx.notify();
    }
    fn login_account(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.source.login_account() {
            self.error = Some(error.to_string());
        }
        cx.notify();
    }

    fn navigate_device(&mut self, action: navigation::Navigate, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if action.node.is_none()
            && matches!(action.destination, navigation::Destination::Manage(3))
            && self.active.is_some()
        {
            self.add_device_open = true;
            if let Some(mesh) = &self.mesh_settings {
                mesh.update(cx, |v, cx| {
                    v.enrollment_only = true;
                    cx.notify();
                });
            }
            cx.notify();
            return;
        }
        if let Some(id) = &action.node {
            let Some(node) = self.nodes.iter().find(|n| &n.id == id).cloned() else {
                return;
            };
            if node.mesh.is_some() && self.mesh_identity.is_none() {
                self.pending_navigation = Some(action);
                self.start_pairing(Some(node), cx);
                return;
            }
            if self.active_node_id.as_ref() != Some(id) {
                self.open_node(node, cx);
            }
        }
        self.apply_navigation(action.destination, cx);
    }
    fn apply_navigation(&mut self, destination: navigation::Destination, cx: &mut Context<Self>) {
        if let navigation::Destination::Manage(tab) = destination {
            self.managing = true;
            self.management_tab = tab;
            if tab == 5 {
                let locale = self.client_settings.locale;
                if let Some(resources) = &self.resources {
                    resources.update(cx, |view, cx| {
                        view.set_locale(locale, cx);
                        view.refresh(cx);
                    });
                } else {
                    let core = self.source.resources();
                    self.resources =
                        Some(cx.new(|cx| resources::ResourcesView::new(core, locale, cx)));
                }
            }
            if tab == 1 {
                if let Some(agents) = &self.agents {
                    agents.update(cx, |v, cx| v.refresh(cx));
                }
            }
        } else if let Some(active) = &self.active {
            self.managing = false;
            active.update(cx, |v, cx| v.navigate_device(&destination, cx));
        }
        cx.notify();
    }
    fn close_add_device(&mut self, cx: &mut Context<Self>) {
        self.add_device_open = false;
        if !self.managing {
            if let Some(active) = &self.active {
                active.update(cx, |v, cx| v.focus_after_dialog(cx));
            }
        }
        cx.notify();
    }
    fn ensure_node_view(
        &mut self,
        node: &SavedNode,
        cx: &mut Context<Self>,
    ) -> Option<Entity<RootView>> {
        let (binding, client) = self.source.connection(&node.id).ok()?;
        let existing = self
            .node_views
            .get(&node.id)
            .filter(|(version, _)| *version == binding)
            .map(|(_, view)| view.clone());
        Some(existing.unwrap_or_else(|| {
            let active = cx.new(|cx| {
                let mut view =
                    RootView::new_desktop(client.clone(), self.store.clone(), node.id.clone(), cx);
                view.attach_navigation(self.navigation.clone(), node.name.clone());
                view.set_applications(self.applications.clone(), cx);
                view
            });
            cx.subscribe(&active, |v, _, _: &crate::views::DesktopAction, cx| {
                v.managing = true;
                v.management_tab = 0;
                cx.notify();
            })
            .detach();
            let resource_node = node.id.clone();
            cx.subscribe(
                &active,
                move |desktop, _, event: &crate::views::InspectResource, cx| {
                    match desktop.source.inspection_node(&resource_node, &event.0) {
                        Ok(node) => {
                            let core = desktop.source.resources();
                            let query = event.0.query.clone();
                            let locale = desktop.client_settings.locale;
                            desktop.resource_inspector = Some(cx.new(|cx| {
                                resources::ResourcesView::inspector(core, node, query, locale, cx)
                            }));
                        }
                        Err(error) => {
                            let core = desktop.source.resources();
                            let locale = desktop.client_settings.locale;
                            desktop.resource_inspector = Some(cx.new(|cx| {
                                resources::ResourcesView::unavailable(
                                    core,
                                    error.to_string(),
                                    locale,
                                    cx,
                                )
                            }));
                        }
                    }
                    cx.notify();
                },
            )
            .detach();
            let sidebar = self.navigation.clone();
            let id = node.id.clone();
            let core = active.read(cx).core_device();
            sidebar.update(cx, |nav, cx| nav.bind_node(&id, core.clone(), cx));
            cx.subscribe(
                &active,
                move |desktop, _, change: &crate::views::NavigationChanged, cx| {
                    sidebar.update(cx, |nav, cx| {
                        nav.set_selection(&id, change.selection.clone(), change.locale, cx)
                    });
                    if let Some((_, _, profiles, _)) = desktop.management_views.get(&id) {
                        profiles.update(cx, |view, cx| view.set_locale(change.locale, cx));
                    }
                },
            )
            .detach();
            self.source.bind(node.id.clone(), core, client);
            self.node_views
                .insert(node.id.clone(), (binding, active.clone()));
            active.update(cx, |v, cx| v.start_device_updates(cx));
            active
        }))
    }
    fn open_node(&mut self, node: SavedNode, cx: &mut Context<Self>) {
        let Some(node) = self.source.node(&node.id) else {
            return;
        };
        let Ok((binding, _)) = self.source.connection(&node.id) else {
            return;
        };
        if let Err(error) = self.source.select(&node.id) {
            self.error = Some(error.to_string());
        }
        self.active_node_name = Some(node.name.clone());
        self.active_node_id = Some(node.id.clone());
        self.navigation
            .update(cx, |nav, cx| nav.update_nodes(&self.nodes, cx));
        let Some(retained) = self.ensure_node_view(&node, cx) else {
            return;
        };
        let profile_source = retained.read(cx).core_device().profiles();
        if let Some((_, agents, profiles, mesh)) = self
            .management_views
            .get(&node.id)
            .filter(|(version, _, _, _)| *version == binding)
        {
            self.agents = Some(agents.clone());
            self.profiles = Some(profiles.clone());
            self.mesh_settings = Some(mesh.clone());
        } else {
            let agents = cx.new(|cx| {
                let mut view = agents::AgentsView::new_with_source(
                    retained.read(cx).core_device().agents(),
                    cx,
                );
                view.set_device_name(node.name.clone());
                view.set_resources(
                    self.source.resources(),
                    node.id.clone(),
                    self.client_settings.locale,
                );
                view
            });
            let profiles = cx.new(|cx| {
                let mut view = profiles::ProfilesView::new_with_source(profile_source.clone(), cx);
                view.set_device_name(node.name.clone());
                view
            });
            let mesh = cx.new(|cx| {
                mesh_settings::MeshSettings::new(retained.read(cx).core_device().mesh_admin(), cx)
            });
            let helper_node = node.id.clone();
            cx.subscribe(&agents, move |view, _, event: &ui::OpenAgent, cx| {
                view.navigate_device(
                    navigation::Navigate {
                        node: Some(helper_node.clone()),
                        destination: navigation::Destination::Leader(event.id.clone()),
                    },
                    cx,
                );
            })
            .detach();
            let helper_node = node.id.clone();
            cx.subscribe(&profiles, move |view, _, event: &ui::OpenAgent, cx| {
                view.navigate_device(
                    navigation::Navigate {
                        node: Some(helper_node.clone()),
                        destination: navigation::Destination::Leader(event.id.clone()),
                    },
                    cx,
                );
            })
            .detach();

            self.management_views.insert(
                node.id.clone(),
                (binding, agents.clone(), profiles.clone(), mesh.clone()),
            );
            self.agents = Some(agents);
            self.profiles = Some(profiles);
            self.mesh_settings = Some(mesh);
        }
        if !self
            .service_views
            .get(&node.id)
            .is_some_and(|(version, _)| *version == binding)
        {
            let resources = self.source.resources();
            let locale = self.client_settings.locale;
            let services = cx.new(|cx| {
                resources::ResourcesView::services(resources, node.id.clone(), locale, cx)
            });
            self.service_views
                .insert(node.id.clone(), (binding, services));
        }
        let source = self.source.clone();
        let info_node = node.clone();
        cx.spawn(async move |_, _| {
            let _ = source.refresh_info(&info_node).await;
        })
        .detach();
        for saved in self.nodes.clone() {
            self.ensure_node_view(&saved, cx);
        }
        let active = retained;
        let (selection, locale) = active.read(cx).navigation_selection();
        self.navigation.update(cx, |nav, cx| {
            nav.update_nodes(&self.nodes, cx);
            nav.activate(&node.id, cx);
            nav.set_selection(&node.id, selection, locale, cx);
        });
        if let Some(previous) = &self.active {
            previous.update(cx, |view, cx| view.hide_browser(cx));
        }
        if let Some(profiles) = &self.profiles {
            profiles.update(cx, |view, cx| view.set_locale(locale, cx));
        }
        self.active = Some(active);
        self.managing = false;
        cx.notify();
    }
    fn start_pairing(&mut self, node: Option<SavedNode>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.pairing = true;
        let source = self.source.clone();
        let work = cx
            .background_executor()
            .spawn(async move { source.pair(node) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(Some(node)) => {
                        view.open_node(node, cx);
                        if let Some(action) = view.pending_navigation.take() {
                            view.apply_navigation(action.destination, cx);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        view.pending_navigation = None;
                        view.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn connect_remote(&mut self, cx: &mut Context<Self>) {
        match Directory::remote_input(
            self.remote_origin.read(cx).value().into(),
            self.remote_name.read(cx).value().into(),
            self.remote_addr.read(cx).value().into(),
        ) {
            Ok(node) => self.start_pairing(Some(node), cx),
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }
    fn start_node(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let source = self.source.clone();
        let work = cx
            .background_executor()
            .spawn(async move { source.start_local() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(node) => view.open_node(node, cx),
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn stop_node(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        if let Some(previous) = &self.active {
            previous.update(cx, |view, cx| view.hide_browser(cx));
        }
        self.active = None;
        self.profiles = None;
        self.agents = None;
        self.mesh_settings = None;
        self.active_node_name = None;
        self.active_node_id = None;
        self.management_tab = 3;
        let source = self.source.clone();
        let work = cx
            .background_executor()
            .spawn(async move { source.stop_local() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                view.error = result.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_node_background(&mut self, enabled: bool, at_login: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let local = self.local.clone();
        let work = cx
            .background_executor()
            .spawn(async move { local.set_background(enabled, at_login) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |v, cx| {
                v.busy = false;
                v.error = result.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl DesktopRoot {
    fn apply_device_name(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        self.navigation
            .update(cx, |nav, cx| nav.update_nodes(&self.nodes, cx));
        if let Some((_, view)) = self.node_views.get(id) {
            view.update(cx, |v, cx| {
                v.attach_navigation(self.navigation.clone(), name.into());
                cx.notify();
            });
        }
        if let Some((_, agents, profiles, _)) = self.management_views.get(id) {
            agents.update(cx, |v, cx| {
                v.set_device_name(name.into());
                cx.notify();
            });
            profiles.update(cx, |v, cx| {
                v.set_device_name(name.into());
                cx.notify();
            });
        }
        if self.active_node_id.as_deref() == Some(id) {
            self.active_node_name = Some(name.into());
        }
    }
    fn open_device_rename(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self
            .nodes
            .iter()
            .find(|n| Some(&n.id) == self.active_node_id.as_ref())
        else {
            return;
        };
        self.rename_input
            .update(cx, |v, cx| v.set_value(node.name.clone(), cx));
        self.rename_node_id = Some(node.id.clone());
        self.rename_error = None;
        cx.notify();
    }
    fn save_device_name(&mut self, cx: &mut Context<Self>) {
        if self.rename_busy {
            return;
        }
        let Some(node) = self
            .nodes
            .iter()
            .find(|n| Some(&n.id) == self.rename_node_id.as_ref())
            .cloned()
        else {
            return;
        };
        let name = self.rename_input.read(cx).value().to_owned();
        self.rename_busy = true;
        self.rename_error = None;
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = source.rename(&node, name).await;
            let _ = this.update(cx, |view, cx| {
                view.rename_busy = false;
                match result {
                    Ok(()) => view.rename_node_id = None,
                    Err(e) => view.rename_error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn refresh_device_info(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(node) = self
            .nodes
            .iter()
            .find(|n| Some(&n.id) == self.active_node_id.as_ref())
            .cloned()
        else {
            return;
        };
        self.busy = true;
        self.error = None;
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = source.refresh_info(&node).await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                view.error = result.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn update_device_release(&mut self, install: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(node) = self
            .nodes
            .iter()
            .find(|n| Some(&n.id) == self.active_node_id.as_ref())
            .cloned()
        else {
            return;
        };
        self.busy = true;
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = source.update_release(&node, install).await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                view.error = result.err().map(|e| e.to_string());
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn render_device(&self, cx: &mut Context<Self>) -> Div {
        use zork_ui::settings::{DeviceAction, DeviceData};
        let Some(node) = self
            .nodes
            .iter()
            .find(|n| Some(&n.id) == self.active_node_id.as_ref())
        else {
            return self.render_nodes(cx);
        };
        let info = self.device_info.get(&node.id);
        let data = DeviceData {
            name: node.name.clone(),
            version: info
                .and_then(|i| {
                    i["gateway"]["release_version"]
                        .as_str()
                        .or(i["gateway"]["version"].as_str())
                })
                .unwrap_or("尚未获取")
                .into(),
            update_supported: info.is_some_and(|i| i["update"]["supported"] == true),
            update_reason: info
                .and_then(|i| i["update"]["reason"].as_str())
                .map(|reason| {
                    if node.local && !self.local.background() {
                        "开启后台运行后可升级".into()
                    } else {
                        reason.into()
                    }
                }),
            latest_version: info
                .and_then(|i| i["latest_version"].as_str())
                .map(str::to_owned),
            online: self
                .active
                .as_ref()
                .and_then(|v| v.read(cx).core_device().snapshot().online),
            local: node.local,
            running: self.local_enabled,
            background: self.local.background(),
            start_at_login: self.local.start_at_login(),
            busy: self.busy,
            notice: info
                .and_then(|i| {
                    i["error"]
                        .as_str()
                        .or(i["update"]["status"]["message"].as_str())
                })
                .map(str::to_owned),
        };
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(zork_ui::settings::device(
                data,
                &self.device_switch_focus,
                cx,
                |v, action, cx| match action {
                    DeviceAction::Rename => v.open_device_rename(cx),
                    DeviceAction::Refresh => v.refresh_device_info(cx),
                    DeviceAction::CheckUpdate => v.update_device_release(false, cx),
                    DeviceAction::Upgrade => v.update_device_release(true, cx),
                    DeviceAction::ToggleRunning => {
                        if v.local_enabled {
                            v.stop_node(cx)
                        } else {
                            v.start_node(cx)
                        }
                    }
                    DeviceAction::Background(on) => v.set_node_background(
                        on,
                        if on { v.local.start_at_login() } else { false },
                        cx,
                    ),
                    DeviceAction::StartAtLogin(on) => v.set_node_background(true, on, cx),
                },
            ))
            .when_some(
                self.service_views
                    .get(&node.id)
                    .map(|(_, view)| view.clone()),
                |body, view| body.child(view),
            )
    }
    fn render_nodes(&self, cx: &mut Context<Self>) -> Div {
        let p = CUE_UI.palette;
        let running = self.local.running();
        let enabled = self.local_enabled;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(ui::heading(
                "设备",
                if self.nodes.is_empty() {
                    "开启本机设备，或连接一台已有设备。"
                } else {
                    "选择运行小伙伴的设备。"
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .py_4()
                    .child(
                        div()
                            .size(px(44.))
                            .rounded(px(12.))
                            .bg(rgb(p.selected))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon("icons/node.svg", 22.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().font_weight(FontWeight::MEDIUM).child("本机设备"))
                            .child(div().text_size(px(12.)).text_color(rgb(p.muted)).child(
                                if self.busy {
                                    "处理中…"
                                } else if running {
                                    "运行中"
                                } else if enabled {
                                    "已开启，设备未运行"
                                } else {
                                    "已关闭"
                                },
                            )),
                    )
                    .child(
                        ui::button(
                            "local-node-toggle",
                            if enabled {
                                "关闭设备"
                            } else {
                                "开启本机设备"
                            },
                            !enabled,
                            !self.busy,
                        )
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if enabled {
                                v.stop_node(cx)
                            } else {
                                v.start_node(cx)
                            }
                        }))
                        .automation_enabled(
                            !self.busy,
                            AutomationRole::Button,
                            if enabled {
                                "关闭设备"
                            } else {
                                "开启本机设备"
                            },
                        ),
                    ),
            )
            .when(enabled && !running && !self.busy, |v| {
                v.child(
                    ui::button("local-node-retry", "重试启动", false, true)
                        .on_click(cx.listener(|v, _, _, cx| v.start_node(cx)))
                        .automation(AutomationRole::Button, "重试启动本机设备"),
                )
            })
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(p.muted))
                    .pb_4()
                    .child(if self.local.background(){"Gateway 由后台服务管理，退出客户端后继续运行。"}else{"由客户端启动的 Gateway 随客户端退出。连接已有的独立 Gateway 不改变它的运行方式。"}),
            )
            .when(enabled||running,|v|{
                let background=self.local.background();let at_login=self.local.start_at_login();
                v.child(ui::section()
                    .child(div().flex().items_center().justify_between().gap_4()
                        .child(div().flex_1().child("退出客户端后保持 Gateway 运行"))
                        .child(ui::button("local-node-background",if background{"已开启"}else{"开启"},false,!self.busy)
                            .on_click(cx.listener(move|v,_,_,cx|v.set_node_background(!background,at_login,cx)))
                            .automation_enabled(!self.busy,AutomationRole::Button,if background{"关闭后台运行"}else{"开启后台运行"})))
                    .when(background,|v|v.child(div().flex().items_center().justify_between().gap_4().pt_3()
                        .child("登录系统后自动启动")
                        .child(ui::button("local-node-login",if at_login{"已开启"}else{"开启"},false,!self.busy)
                            .on_click(cx.listener(move|v,_,_,cx|v.set_node_background(true,!at_login,cx)))
                            .automation_enabled(!self.busy,AutomationRole::Button,"登录系统后自动启动"))))
                    .child(div().pt_3().text_size(px(12.)).text_color(rgb(p.muted)).child("切换后台运行不会重启任务。关闭设备会停止此设备的 Gateway，其他设备将暂时无法访问它。")))
            })
            .child(
                ui::section()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(ui::label("其他设备"))
                            .child(
                                ui::button("connect-existing-node", "连接设备", false, !self.busy)
                                    .on_click(cx.listener(|v, _, _, cx| v.start_pairing(None, cx)))
                                    .automation_enabled(
                                        !self.busy,
                                        AutomationRole::Button,
                                        "连接已有设备",
                                    ),
                            ),
                    )
                    .children(
                        self.nodes
                            .iter()
                            .filter(|n| n.mesh.is_some())
                            .cloned()
                            .map(|node| {
                                let open = node.clone();
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .py_2()
                                    .child(ui::icon("icons/node.svg", 18.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .child(node.name.clone()),
                                    )
                                    .child(
                                        ui::button(
                                            format!("connect-node-{}", node.id),
                                            "连接",
                                            false,
                                            !self.busy,
                                        )
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.start_pairing(Some(open.clone()), cx)
                                        }))
                                        .automation_enabled(
                                            !self.busy,
                                            AutomationRole::Button,
                                            "连接设备",
                                        ),
                                    )
                            }),
                    )
                    .when(
                        self.nodes.iter().all(|n| n.mesh.is_none()) && !self.pairing,
                        |v| {
                            v.child(
                                div()
                                    .py_3()
                                    .text_size(px(12.))
                                    .text_color(rgb(p.muted))
                                    .child("还没有连接其他设备。"),
                            )
                        },
                    ),
            )
            .when(self.pairing, |v| {
                v.child(
                    ui::section()
                        .child(ui::label("连接已有设备"))
                        .when_some(self.mesh_identity.clone(), |v, identity| {
                            v.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(12.))
                                            .line_height(px(20.))
                                            .text_color(rgb(p.muted))
                                            .child("在目标设备添加此设备，并开启客户端权限。"),
                                    )
                                    .child(
                                        ui::button(
                                            "copy-mesh-identity",
                                            "复制我的身份",
                                            false,
                                            true,
                                        )
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                identity.clone(),
                                            ))
                                        })
                                        .automation(AutomationRole::Button, "复制设备身份"),
                                    ),
                            )
                        })
                        .child(ui::field("remote-name", "设备名称", &self.remote_name, cx))
                        .child(ui::field(
                            "remote-origin",
                            "目标设备身份",
                            &self.remote_origin, cx))
                        .child(ui::field(
                            "remote-addr",
                            "局域网地址 · 可选",
                            &self.remote_addr, cx))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    ui::button("cancel-pairing", "取消", false, !self.busy)
                                        .on_click(cx.listener(|v, _, _, cx| {
                                            v.pairing = false;
                                            cx.notify();
                                        }))
                                        .automation(AutomationRole::Button, "取消连接"),
                                )
                                .child(
                                    ui::button(
                                        "connect-remote",
                                        "保存并连接",
                                        true,
                                        !self.busy && self.mesh_identity.is_some(),
                                    )
                                    .on_click(cx.listener(|v, _, _, cx| v.connect_remote(cx)))
                                    .automation_enabled(
                                        !self.busy && self.mesh_identity.is_some(),
                                        AutomationRole::Button,
                                        "保存并连接",
                                    ),
                                ),
                        ),
                )
            })
            .when(!running && !self.nodes.is_empty(), |v| {
                v.child(ui::section().child(ui::label("本地记录")).children(
                    self.nodes.clone().into_iter().map(|node| {
                        ui::button(
                            format!("browse-node-{}", node.id),
                            format!("浏览 {}", node.name),
                            false,
                            true,
                        )
                        .on_click(cx.listener(move |v, _, _, cx| v.open_node(node.clone(), cx)))
                        .automation(AutomationRole::Button, "浏览本地记录")
                    }),
                ))
            })
    }
    fn render_account(&self, cx: &mut Context<Self>) -> Div {
        use zork_ui::settings::{AccountAction, AccountData};
        zork_ui::settings::account(
            AccountData {
                name: self.identity.as_ref().map(|i| i.name.clone()),
                email: self.identity.as_ref().and_then(|i| i.email.clone()),
                identity: None,
                busy: self.account_busy,
                notice: None,
            },
            cx,
            |v, action, cx| match action {
                AccountAction::Login => v.login_account(cx),
                AccountAction::Cancel => {
                    v.source.cancel_account();
                    cx.notify();
                }
                AccountAction::Logout => {
                    if let Err(error) = v.source.logout() {
                        v.error = Some(error.to_string());
                    }
                    cx.notify();
                }
                AccountAction::CopyIdentity => {
                    if let Some(identity) = &v.mesh_identity {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(identity.clone()));
                        v.error = Some("客户端身份已复制。".into());
                        cx.notify();
                    }
                }
            },
        )
    }
}
impl Render for DesktopRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.busy {
            if let Some(tag) = self.pending_notification.take() {
                let view = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = view.update(cx, |view, cx| view.open_notification(tag, cx));
                });
            }
        }
        let p = CUE_UI.palette;
        if !self.activation_observed {
            cx.observe_window_activation(window, |_, _, cx| cx.notify())
                .detach();
            self.activation_observed = true;
        }
        self.navigation.update(cx, |nav, cx| {
            nav.set_viewing(
                !self.managing && !self.add_device_open && window.is_window_active(),
                cx,
            )
        });
        self.rename_modal.sync(
            self.rename_node_id.as_ref().map(|_| "device-rename-dialog"),
            window,
            cx,
        );
        self.add_device_modal.sync(
            self.add_device_open.then_some("add-device-dialog"),
            window,
            cx,
        );
        let width = self
            .navigation
            .read(cx)
            .width(window.viewport_size().width.as_f32());
        for (id, (_, _, profiles, _)) in &self.management_views {
            let visible = self.managing
                && self.management_tab == 0
                && self.active.is_some()
                && self.active_node_id.as_ref() == Some(id);
            profiles.update(cx, |view, cx| view.set_visible(visible, cx));
        }
        let tab = if self.active.is_none() && !matches!(self.management_tab, 4 | 5) {
            3
        } else {
            self.management_tab
        };
        let shell = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.dialog_focus)
            .bg(rgb(p.window))
            .on_key_down(cx.listener(|v, e: &gpui::KeyDownEvent, _, cx| {
                if v.add_device_open && e.keystroke.key == "escape" {
                    v.close_add_device(cx);
                    cx.stop_propagation();
                }
            }))
            .text_color(rgb(p.text))
            .font_family("Inter Variable")
            .text_size(px(13.))
            .on_mouse_move(cx.listener(|v, e: &gpui::MouseMoveEvent, w, cx| {
                v.drag_message_preview(e.position.y.as_f32(), cx);
                v.navigation.update(cx, |n, cx| {
                    n.resize(e.position.x.as_f32(), w.viewport_size().width.as_f32(), cx)
                });
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|v, _, _, cx| {
                    v.finish_message_preview_drag(cx);
                    v.navigation.update(cx, |n, cx| n.finish_resize(cx));
                }),
            );
        let shell = shell.on_mouse_up_out(
            gpui::MouseButton::Left,
            cx.listener(|v, _, _, cx| v.finish_message_preview_drag(cx)),
        );
        let content = if !self.managing {
            div()
                .size_full()
                .when_some(self.active.clone(), |v, a| v.child(a))
        } else {
            div()
                .size_full()
                .flex()
                .child(
                    self.settings_tabs.surface(
                        self.settings_tabs
                            .column()
                            .w(px(width))
                            .relative()
                            .flex_shrink_0()
                            .h_full()
                            .px_2()
                            .child(
                                div()
                                    .h(px(48.))
                                    .pl(px(64.))
                                    .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                                        window.start_window_move()
                                    })
                                    .child(window.use_keyed_state(
                                        "management-header-brand",
                                        cx,
                                        |_, _| {
                                            crate::components::brand::Brand::new(
                                                crate::components::brand::BrandMotion::Header,
                                                p.sidebar,
                                            )
                                        },
                                    )),
                            )
                            .child(
                                self.settings_tabs
                                    .tab("desktop-return".into(), false)
                                    .child(ui::icon("icons/arrow-left.svg", 16.))
                                    .child("返回对话")
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        if v.active.is_some() {
                                            v.managing = false;
                                            cx.notify();
                                        }
                                    }))
                                    .automation(AutomationRole::Button, "返回对话"),
                            )
                            .child(
                                self.settings_tabs
                                    .tab("settings-tool-connections".into(), tab == 5)
                                    .child(ui::icon("icons/mesh.svg", 20.))
                                    .child(self.client_settings.locale.text("tool_connections"))
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.apply_navigation(navigation::Destination::Manage(5), cx)
                                    }))
                                    .automation(
                                        AutomationRole::Button,
                                        self.client_settings.locale.text("tool_connections"),
                                    ),
                            )
                            .child(
                                self.settings_tabs
                                    .column()
                                    .id("settings-navigation-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .child(self.render_client_settings_navigation(tab == 4, cx))
                                    .child(
                                        self.settings_tabs
                                            .section(
                                                "device-settings-heading",
                                                self.client_settings
                                                    .locale
                                                    .text("device_settings_title"),
                                            )
                                            .children(self.nodes.clone().into_iter().map(|node| {
                                                let selected =
                                                    self.active_node_id.as_ref() == Some(&node.id);
                                                let open = node.clone();
                                                self.settings_tabs
                                                    .column()
                                                    .child(
                                                        self.settings_tabs
                                                            .tab(
                                                                format!(
                                                                    "settings-device-{}",
                                                                    node.id
                                                                ),
                                                                selected && tab == 3,
                                                            )
                                                            .child(ui::icon("icons/node.svg", 20.))
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .child(node.name.clone()),
                                                            )
                                                            .on_click(cx.listener(
                                                                move |v, _, _, cx| {
                                                                    if !v.busy {
                                                                        v.open_node(
                                                                            open.clone(),
                                                                            cx,
                                                                        );
                                                                        v.managing = true;
                                                                        v.management_tab = 3;
                                                                        cx.notify();
                                                                    }
                                                                },
                                                            ))
                                                            .automation(
                                                                AutomationRole::Button,
                                                                format!("{} 设备设置", node.name),
                                                            ),
                                                    )
                                                    .children(
                                                        [(1, "队员"), (0, "大模型")]
                                                            .into_iter()
                                                            .map(|(index, label)| {
                                                                let open = node.clone();
                                                                self.settings_tabs.tab(
                                                    format!("settings-{}-{index}", node.id),
                                                    selected && tab == index,
                                                )
                                                .pl(px(36.))
                                                .child(label)
                                                .on_click(cx.listener(move |v, _, _, cx| {
                                                    if !v.busy {
                                                        if v.active_node_id.as_ref()
                                                            != Some(&open.id)
                                                        {
                                                            v.open_node(open.clone(), cx);
                                                        }
                                                        v.managing = true;
                                                        v.management_tab = index;
                                                        if index == 1 {
                                                            if let Some(agents) = &v.agents {
                                                                agents.update(cx, |a, cx| {
                                                                    a.refresh(cx)
                                                                });
                                                            }
                                                        } else if index == 0 {
                                                            if let Some(profiles) = &v.profiles {
                                                                profiles.update(cx, |p, cx| {
                                                                    p.refresh(cx)
                                                                });
                                                            }
                                                        }
                                                        cx.notify();
                                                    }
                                                }))
                                                .automation(
                                                    AutomationRole::Button,
                                                    format!("{} {label}", node.name),
                                                )
                                                            }),
                                                    )
                                            }))
                                            .child(
                                                self.settings_tabs
                                                    .tab("settings-add-device".into(), false)
                                                    .child(ui::icon("icons/plus.svg", 20.))
                                                    .child("连接设备")
                                                    .on_click(cx.listener(|v, _, _, cx| {
                                                        v.navigate_device(
                                                            navigation::Navigate {
                                                                node: None,
                                                                destination:
                                                                    navigation::Destination::Manage(
                                                                        3,
                                                                    ),
                                                            },
                                                            cx,
                                                        )
                                                    }))
                                                    .automation(AutomationRole::Button, "连接设备"),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .w(px(5.))
                                    .h_full()
                                    .id("settings-sidebar-resize")
                                    .cursor_col_resize()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|v, _, _, cx| {
                                            v.navigation.update(cx, |n, _| n.resizing = true);
                                            cx.stop_propagation();
                                        }),
                                    ),
                            ),
                    ),
                )
                .child(
                    div().flex_1().min_w_0().h_full().bg(rgb(p.canvas)).child(
                        div()
                            .id("desktop-settings-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .child(ui::settings_content(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .when(tab == 3, |v| v.child(self.render_device(cx)))
                                    .when(tab == 4, |v| v.child(self.render_client_settings(cx)))
                                    .when(tab == 5, |v| {
                                        v.when_some(self.resources.clone(), |body, view| {
                                            body.child(view)
                                        })
                                    })
                                    .when(tab == 0, |v| {
                                        v.when_some(self.profiles.clone(), |v, e| v.child(e))
                                    })
                                    .when(tab == 1, |v| {
                                        v.when_some(self.agents.clone(), |v, e| v.child(e))
                                    })
                                    .when(tab == 2, |v| {
                                        v.when_some(self.mesh_settings.clone(), |v, e| v.child(e))
                                    })
                                    .when_some(self.error.clone(), |v, e| v.child(ui::feedback(e))),
                            )),
                    ),
                )
        };
        shell
            .child(content)
            .when_some(self.resource_inspector.clone(), |shell, view| {
                shell.child(view)
            })
            .when(self.rename_node_id.is_some(), |shell| {
                shell.child(ui::modal(
                    "device-rename-dialog",
                    "修改设备名称",
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(ui::field(
                            "device-name-input",
                            "名称",
                            &self.rename_input,
                            cx,
                        ))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(p.muted))
                                .child("连接此设备的小伙伴都会看到新名称。"),
                        ),
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            ui::button("device-name-cancel", "取消", false, !self.rename_busy)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    if !v.rename_busy {
                                        v.rename_node_id = None;
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            ui::busy_button(
                                "device-name-save",
                                if self.rename_busy {
                                    "保存中…"
                                } else {
                                    "保存"
                                },
                                true,
                                !self.rename_busy,
                                self.rename_busy,
                            )
                            .on_click(cx.listener(|v, _, _, cx| v.save_device_name(cx)))
                            .automation_enabled(
                                !self.rename_busy,
                                AutomationRole::Button,
                                "保存设备名称",
                            ),
                        ),
                    self.rename_error.clone(),
                    &self.rename_modal.focus,
                    window,
                    cx,
                    !self.rename_busy,
                    |v, _, cx| {
                        v.rename_node_id = None;
                        cx.notify();
                    },
                ))
            })
            .when(self.add_device_open, |shell| {
                shell.child(ui::detail_modal(
                    "add-device-dialog",
                    "连接设备",
                    div()
                        .flex()
                        .flex_col()
                        .when_some(self.mesh_settings.clone(), |v, e| v.child(e)),
                    None,
                    &self.add_device_modal.focus,
                    window,
                    cx,
                    true,
                    |v, _, cx| v.close_add_device(cx),
                ))
            })
    }
}

#[cfg(feature = "headless-bench")]
pub use ui::settings_content as headless_settings_content;

#[cfg(feature = "headless-bench")]
pub mod stories;
