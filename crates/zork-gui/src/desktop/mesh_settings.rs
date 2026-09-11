use super::ui;
use crate::{
    api::{MeshAction, MeshAdmin, MeshAdminData},
    automation::{AutomationElementExt, AutomationRole},
    components::text_input::ComposerInput,
    design::CUE_UI,
};
use gpui::{div, prelude::*, rgb, Context, Entity, Window};
use std::sync::Arc;
pub struct MeshSettings {
    pub enrollment_only: bool,
    source: Arc<MeshAdmin>,
    updates: Option<gpui::Task<()>>,
    saved: u64,
    config: Option<crate::api::MeshConfig>,
    origin: Option<String>,
    name: Entity<ComposerInput>,
    peer: Entity<ComposerInput>,
    addr: Entity<ComposerInput>,
    client_grant: bool,
    form_open: bool,
    busy: bool,
    message: Option<String>,
    invitation: Option<serde_json::Value>,
    phone: bool,
    peers: Vec<crate::api::MeshPeer>,
    modal: ui::ModalState,
    switch_focus: [gpui::FocusHandle; 2],
}
impl MeshSettings {
    pub fn new(source: Arc<MeshAdmin>, cx: &mut Context<Self>) -> Self {
        let mut field = |label| {
            let e = cx.new(|cx| ComposerInput::new(label, cx));
            cx.observe(&e, |_, _, cx| cx.notify()).detach();
            e
        };
        let name = field("设备名称");
        let peer = field("key: 设备身份");
        let addr = field("局域网地址（可选）");
        let mut v = Self {
            enrollment_only: false,
            source,
            updates: None,
            saved: 0,
            config: None,
            origin: None,
            name,
            peer,
            addr,
            client_grant: false,
            form_open: false,
            busy: false,
            message: None,
            invitation: None,
            phone: true,
            peers: vec![],
            modal: ui::ModalState::new(cx),
            switch_focus: [cx.focus_handle(), cx.focus_handle()],
        };
        v.watch(cx);
        v.refresh(cx);
        v
    }
    pub fn refresh(&mut self, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::Refresh);
    }
    fn watch(&mut self, cx: &mut Context<Self>) {
        let mut updates = self.source.subscribe();
        self.accept(updates.snapshot(), cx);
        self.updates = Some(cx.spawn(async move |this, cx| {
            while let Some(state) = updates.changed().await {
                if this.update(cx, |view, cx| view.accept(state, cx)).is_err() {
                    return;
                }
            }
        }));
    }
    fn accept(&mut self, state: Arc<MeshAdminData>, cx: &mut Context<Self>) {
        self.config = state.config.clone();
        self.origin = state.origin.clone();
        self.peers = state.peers.clone();
        self.invitation = state.invitations[usize::from(self.phone)].clone();
        self.busy = state.busy;
        self.message = state.message.clone();
        if self.form_open && self.saved != state.saved {
            self.form_open = false;
            self.name.update(cx, |v, cx| v.clear(cx));
            self.peer.update(cx, |v, cx| v.clear(cx));
            self.addr.update(cx, |v, cx| v.clear(cx));
            self.client_grant = false;
        }
        self.saved = state.saved;
        cx.notify();
    }

    fn add_peer(&mut self, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::AddPeer {
            origin: self.peer.read(_cx).value().into(),
            name: self.name.read(_cx).value().into(),
            address: self.addr.read(_cx).value().into(),
            client: self.client_grant,
        });
    }

    fn create_invite(&mut self, phone: bool, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::CreateInvite(phone));
    }

    fn approve_phone(&mut self, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::ApproveInvite(self.phone));
    }
    fn revoke_invite(&mut self, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::RevokeInvite(self.phone));
    }

    fn remove_peer(&mut self, origin: String, _cx: &mut Context<Self>) {
        self.source.dispatch(MeshAction::RemovePeer(origin));
    }
    fn field(
        &self,
        id: &'static str,
        label: &'static str,
        input: &Entity<ComposerInput>,
        cx: &gpui::App,
    ) -> gpui::Div {
        ui::field(id, label, input, cx)
    }
}
impl MeshSettings {
    fn enrollment_data(&self) -> zork_ui::network::EnrollmentData {
        let invitation = self.invitation.as_ref();
        let status = invitation.and_then(|i| i["status"].as_str()).unwrap_or("");
        let remaining = self.source.remaining(self.phone).unwrap_or_default();
        let phone = self.phone;
        let label = match status {
            "awaiting_approval" => format!(
                "{} 请求连接，请确认是你的手机。",
                invitation
                    .and_then(|i| i["device"]["name"].as_str())
                    .unwrap_or("手机")
            ),
            "joined" if phone => "手机已连接，可以继续工作。".into(),
            "waiting" if phone => format!(
                "等待手机扫码 · {} 分 {} 秒后过期",
                remaining / 60,
                remaining % 60
            ),
            "joined" => format!(
                "{} 已加入，任务协作已开启",
                invitation
                    .and_then(|i| i["device"]["name"].as_str())
                    .unwrap_or("设备")
            ),
            "expired" => if phone {
                "手机邀请已过期，请重新生成。"
            } else {
                "加入命令已过期，请重新生成。"
            }
            .into(),
            "revoked" => if phone {
                "手机邀请已取消。"
            } else {
                "加入命令已撤销。"
            }
            .into(),
            "connecting" => "正在确认设备身份…".into(),
            "waiting" => format!(
                "等待目标设备执行 · {} 分 {} 秒后过期",
                remaining / 60,
                remaining % 60
            ),
            _ => String::new(),
        };
        zork_ui::network::EnrollmentData {
            client: phone,
            ticket: invitation
                .and_then(|i| i["invitation"].as_str())
                .unwrap_or_default()
                .into(),
            available: self.config.is_some(),
            busy: self.busy,
            status: status.into(),
            command: invitation
                .and_then(|i| i["command"].as_str())
                .unwrap_or_default()
                .into(),
            status_label: label,
            notice: self.message.clone(),
        }
    }
    fn select_invitation(&mut self, phone: bool, cx: &mut Context<Self>) {
        self.phone = phone;
        self.accept(self.source.snapshot(), cx);
    }
    fn enrollment_action(
        &mut self,
        action: zork_ui::network::EnrollmentAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            zork_ui::network::EnrollmentAction::Select(phone) => self.select_invitation(phone, cx),
            zork_ui::network::EnrollmentAction::Create => self.create_invite(false, cx),
            zork_ui::network::EnrollmentAction::CreateClient => self.create_invite(true, cx),
            zork_ui::network::EnrollmentAction::Approve => self.approve_phone(cx),
            zork_ui::network::EnrollmentAction::Revoke => self.revoke_invite(cx),
            zork_ui::network::EnrollmentAction::Copy => {
                if let Some(command) = self.invitation.as_ref().and_then(|i| {
                    i[if i["scope"] == "client" {
                        "invitation"
                    } else {
                        "command"
                    }]
                    .as_str()
                }) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(command.into()));
                    self.message = Some("连接邀请已复制。".into());
                    cx.notify();
                }
            }
        }
    }
}
impl Render for MeshSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.modal.sync(
            (self.form_open && !self.enrollment_only).then_some("mesh-peer-dialog"),
            window,
            cx,
        );
        if self.enrollment_only {
            return zork_ui::network::enrollment(self.enrollment_data(), cx, |v, event, cx| {
                v.enrollment_action(event, cx)
            })
            .into_any_element();
        }
        let data = zork_ui::network::NetworkData {
            enabled: self.config.as_ref().is_some_and(|c| c.enabled),
            available: self.config.is_some(),
            busy: self.busy,
            identity: self.origin.clone(),
            notice: if self.form_open {
                None
            } else {
                self.message.clone()
            },
            peers: self
                .config
                .as_ref()
                .map(|c| {
                    c.peers
                        .iter()
                        .map(|peer| zork_ui::network::Peer {
                            id: peer.origin.clone(),
                            name: peer.name.clone(),
                            status: self
                                .peers
                                .iter()
                                .find(|p| p.origin == peer.origin)
                                .map(|p| {
                                    if p.online {
                                        "已连接"
                                    } else {
                                        "暂时无法连接"
                                    }
                                })
                                .unwrap_or("等待连接")
                                .into(),
                            permission: if peer.collaborate {
                                "同一 mesh · 任务协作已开启"
                            } else if peer.client {
                                "客户端 · 可管理此设备"
                            } else {
                                "设备 · 按队员授权协作"
                            }
                            .into(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        div()
            .flex()
            .flex_col()
            .child(zork_ui::network::network(
                data,
                &self.switch_focus[0],
                cx,
                |v, event, cx| {
                    use zork_ui::network::NetworkAction;
                    match event {
                        NetworkAction::Toggle(on) => {
                            v.source.dispatch(MeshAction::Enable(on));
                        }
                        NetworkAction::Refresh => v.refresh(cx),
                        NetworkAction::CopyIdentity => {
                            if let Some(origin) = &v.origin {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    origin.clone(),
                                ));
                                v.message = Some("设备身份已复制。".into());
                                cx.notify();
                            }
                        }
                        NetworkAction::Add => {
                            v.form_open = true;
                            v.message = None;
                            cx.notify();
                        }
                        NetworkAction::Remove(id) => v.remove_peer(id, cx),
                    }
                },
            ))
            .child(
                div()
                    .mt_6()
                    .pt_5()
                    .border_t_1()
                    .border_color(rgb(CUE_UI.palette.border))
                    .child(zork_ui::network::enrollment(
                        self.enrollment_data(),
                        cx,
                        |v, event, cx| v.enrollment_action(event, cx),
                    )),
            )
            .when(self.form_open, |v| {
                v.child(ui::modal(
                    "mesh-peer-dialog",
                    "手动连接设备",
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(self.field("mesh-peer-name", "设备名称", &self.name, cx))
                        .child(self.field("mesh-peer-origin", "设备身份", &self.peer, cx))
                        .child(self.field("mesh-peer-addr", "局域网地址 · 可选", &self.addr, cx))
                        .child(zork_ui::settings::row(
                            "允许作为客户端管理",
                            "可管理此设备的模型连接、小伙伴和任务。",
                            ui::switch(
                                "mesh-client-grant",
                                "客户端权限",
                                self.client_grant,
                                !self.busy,
                                &self.switch_focus[1],
                                cx,
                                |v, on, cx| {
                                    v.client_grant = on;
                                    cx.notify();
                                },
                            ),
                        )),
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            ui::button("mesh-cancel-peer", "取消", false, !self.busy)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    if !v.busy {
                                        v.form_open = false;
                                        v.message = None;
                                        cx.notify();
                                    }
                                }))
                                .automation_enabled(!self.busy, AutomationRole::Button, "取消添加"),
                        )
                        .child(
                            ui::busy_button(
                                "mesh-add-peer",
                                if self.busy {
                                    "正在保存…"
                                } else {
                                    "保存配对"
                                },
                                true,
                                !self.busy,
                                self.busy,
                            )
                            .on_click(cx.listener(|v, _, _, cx| v.add_peer(cx)))
                            .automation_enabled(
                                !self.busy,
                                AutomationRole::Button,
                                "保存配对",
                            ),
                        ),
                    self.message.clone(),
                    &self.modal.focus,
                    window,
                    cx,
                    !self.busy,
                    |v, _, cx| {
                        v.form_open = false;
                        v.message = None;
                        cx.notify();
                    },
                ))
            })
            .into_any_element()
    }
}
