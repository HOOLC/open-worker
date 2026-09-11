//! Device connection and enrollment surfaces shared with the desktop controllers.
use crate::{
    automation::{AutomationElementExt, AutomationRole},
    controls as ui,
    design::CUE_UI,
    settings::row,
};
use gpui::{div, prelude::*, px, rgb, Context, Div, FocusHandle, FontWeight};
use std::rc::Rc;
#[derive(Clone, Default)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub status: String,
    pub permission: String,
}
#[derive(Clone, Default)]
pub struct NetworkData {
    pub enabled: bool,
    pub available: bool,
    pub peers: Vec<Peer>,
    pub identity: Option<String>,
    pub busy: bool,
    pub notice: Option<String>,
}
#[derive(Clone, Debug)]
pub enum NetworkAction {
    Toggle(bool),
    Refresh,
    CopyIdentity,
    Add,
    Remove(String),
}
pub fn network<V: 'static>(
    data: NetworkData,
    focus: &FocusHandle,
    cx: &Context<V>,
    action: impl Fn(&mut V, NetworkAction, &mut Context<V>) + 'static,
) -> Div {
    let action = Rc::new(action);
    let add = action.clone();
    let toggle = action.clone();
    let refresh = action.clone();
    let copy = action.clone();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .pb(px(18.))
                .child(ui::page_title("设备连接"))
                .child(
                    ui::page_action("mesh-new-peer", "手动连接")
                        .on_click(cx.listener(move |v, _, _, cx| add(v, NetworkAction::Add, cx)))
                        .automation(AutomationRole::Button, "手动连接设备"),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(CUE_UI.palette.muted))
                .child("管理已配对设备及其访问权限。"),
        )
        .child(row(
            "允许设备连接",
            if data.enabled {
                "已启用"
            } else {
                "已关闭"
            },
            ui::switch(
                "node-mesh-toggle",
                "允许设备连接",
                data.enabled,
                !data.busy && data.available,
                focus,
                cx,
                move |v, on, cx| toggle(v, NetworkAction::Toggle(on), cx),
            ),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .py_4()
                .child(
                    ui::button(
                        "copy-node-origin",
                        "复制节点身份",
                        false,
                        data.identity.is_some(),
                    )
                    .on_click(
                        cx.listener(move |v, _, _, cx| copy(v, NetworkAction::CopyIdentity, cx)),
                    )
                    .automation_enabled(
                        data.identity.is_some(),
                        AutomationRole::Button,
                        "复制节点身份",
                    ),
                )
                .child(
                    ui::button(
                        "mesh-settings-refresh",
                        if data.busy {
                            "正在刷新…"
                        } else {
                            "刷新"
                        },
                        false,
                        !data.busy,
                    )
                    .on_click(
                        cx.listener(move |v, _, _, cx| refresh(v, NetworkAction::Refresh, cx)),
                    )
                    .automation_enabled(
                        !data.busy,
                        AutomationRole::Button,
                        "刷新连接信息",
                    ),
                ),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .pb_3()
                .child("已配对设备"),
        )
        .children(data.peers.iter().map(|peer| {
            let action = action.clone();
            let id = peer.id.clone();
            row(
                peer.name.clone(),
                format!("{} · {}", peer.status, peer.permission),
                ui::button(format!("revoke-peer-{id}"), "移除", false, !data.busy)
                    .on_click(cx.listener(move |v, _, _, cx| {
                        action(v, NetworkAction::Remove(id.clone()), cx)
                    }))
                    .automation_enabled(!data.busy, AutomationRole::Button, "撤销配对"),
            )
        }))
        .when(data.peers.is_empty(), |v| {
            v.child(
                div()
                    .py_5()
                    .text_size(px(12.))
                    .text_color(rgb(CUE_UI.palette.muted))
                    .child("还没有配对设备。通过加入命令或手动连接连接设备。"),
            )
        })
        .when_some(data.notice, |v, text| {
            v.child(div().mt_4().child(ui::feedback(text)))
        })
}
#[derive(Clone, Default)]
pub struct EnrollmentData {
    pub client: bool,
    pub ticket: String,
    pub available: bool,
    pub busy: bool,
    pub status: String,
    pub command: String,
    pub status_label: String,
    pub notice: Option<String>,
}
#[derive(Clone, Debug)]
pub enum EnrollmentAction {
    Select(bool),
    CreateClient,
    Approve,
    Create,
    Copy,
    Revoke,
}
pub fn enrollment<V: 'static>(
    data: EnrollmentData,
    cx: &Context<V>,
    action: impl Fn(&mut V, EnrollmentAction, &mut Context<V>) + 'static,
) -> Div {
    let action = Rc::new(action);
    let create = action.clone();
    let copy = action.clone();
    let revoke = action.clone();
    let select_client = action.clone();
    let select_gateway = action.clone();
    let approve = action.clone();
    let keyboard = action.clone();
    let active = matches!(
        data.status.as_str(),
        "waiting" | "connecting" | "awaiting_approval"
    ) && (!data.command.is_empty() || !data.ticket.is_empty());
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            ui::choice_group("mesh-connect-mode", if data.client { 0 } else { 1 }, 2)
                .on_key_down(cx.listener(move |v, event: &gpui::KeyDownEvent, _, cx| {
                    let phone = match event.keystroke.key.as_str() {
                        "left" | "home" => Some(true),
                        "right" | "end" => Some(false),
                        _ => None,
                    };
                    if let Some(phone) = phone.filter(|_| !data.busy) {
                        keyboard(v, EnrollmentAction::Select(phone), cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    ui::segment(
                        "mesh-connect-phone-tab",
                        "连接手机",
                        data.client,
                        !data.busy,
                    )
                    .flex_1()
                    .justify_center()
                    .on_click(cx.listener(move |v, _, _, cx| {
                        select_client(v, EnrollmentAction::Select(true), cx)
                    }))
                    .automation_enabled(
                        !data.busy,
                        AutomationRole::Button,
                        if data.client {
                            "连接手机 · 已选中"
                        } else {
                            "连接手机"
                        },
                    ),
                )
                .child(
                    ui::segment(
                        "mesh-connect-device-tab",
                        "连接其它设备",
                        !data.client,
                        !data.busy,
                    )
                    .flex_1()
                    .justify_center()
                    .on_click(cx.listener(move |v, _, _, cx| {
                        select_gateway(v, EnrollmentAction::Select(false), cx)
                    }))
                    .automation_enabled(
                        !data.busy,
                        AutomationRole::Button,
                        if !data.client {
                            "连接其它设备 · 已选中"
                        } else {
                            "连接其它设备"
                        },
                    ),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(CUE_UI.palette.muted))
                .child(if data.client {
                    "在手机 Zork 中打开扫一扫。扫码后，请在这里确认允许连接。"
                } else {
                    "复制加入命令，在其它设备的终端执行。"
                }),
        )
        .when(!data.status_label.is_empty(), |v| {
            v.child(
                div()
                    .id("mesh-invite-status")
                    .text_size(px(12.))
                    .child(data.status_label.clone())
                    .automation(AutomationRole::Status, data.status_label.clone()),
            )
        })
        .when(!active, |v| {
            let label = if data.busy {
                "正在生成…"
            } else if data.client {
                "生成连接二维码"
            } else {
                "生成加入命令"
            };
            v.child(
                ui::button(
                    if data.client {
                        "mesh-client-invite-create"
                    } else {
                        "mesh-invite-create"
                    },
                    label,
                    true,
                    !data.busy && data.available,
                )
                .on_click(cx.listener(move |v, _, _, cx| {
                    create(
                        v,
                        if data.client {
                            EnrollmentAction::CreateClient
                        } else {
                            EnrollmentAction::Create
                        },
                        cx,
                    )
                }))
                .automation_enabled(
                    !data.busy && data.available,
                    AutomationRole::Button,
                    label,
                ),
            )
        })
        .when(active, |v| {
            v.when(data.client, |v| v.child(invitation_qr(&data.ticket)))
                .when(!data.client, |v| {
                    v.child(command_block("mesh-invite-command", data.command.clone()))
                })
                .when(data.status == "awaiting_approval", |v| {
                    v.child(
                        ui::button("mesh-client-invite-approve", "允许连接", true, !data.busy)
                            .on_click(cx.listener(move |v, _, _, cx| {
                                approve(v, EnrollmentAction::Approve, cx)
                            }))
                            .automation_enabled(!data.busy, AutomationRole::Button, "允许连接手机"),
                    )
                })
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            ui::button(
                                "mesh-invite-revoke",
                                if data.client {
                                    "取消邀请"
                                } else {
                                    "撤销命令"
                                },
                                false,
                                !data.busy,
                            )
                            .on_click(cx.listener(move |v, _, _, cx| {
                                revoke(v, EnrollmentAction::Revoke, cx)
                            }))
                            .automation_enabled(
                                !data.busy,
                                AutomationRole::Button,
                                if data.client {
                                    "取消手机邀请"
                                } else {
                                    "撤销加入命令"
                                },
                            ),
                        )
                        .child(
                            ui::button(
                                "mesh-invite-copy",
                                if data.client {
                                    "复制邀请"
                                } else {
                                    "复制命令"
                                },
                                true,
                                true,
                            )
                            .on_click(
                                cx.listener(move |v, _, _, cx| copy(v, EnrollmentAction::Copy, cx)),
                            )
                            .automation(
                                AutomationRole::Button,
                                if data.client {
                                    "复制手机邀请"
                                } else {
                                    "复制加入命令"
                                },
                            ),
                        ),
                )
        })
        .when_some(data.notice, |v, text| v.child(ui::feedback(text)))
}
fn invitation_qr(ticket: &str) -> gpui::AnyElement {
    let Ok(code) = qrcode::QrCode::new(ticket.as_bytes()) else {
        return ui::feedback("邀请较长，请使用复制邀请连接。".into()).into_any_element();
    };
    let width = code.width();
    let modules = code.to_colors();
    // Four modules of quiet zone; integer physical pixels keep camera edges sharp.
    div()
        .id("mesh-client-invite-qr")
        .flex()
        .justify_center()
        .child(
            gpui::canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let scale = window.scale_factor();
                    let unit = ((f32::from(bounds.size.width) * scale) / (width + 8) as f32)
                        .floor()
                        .max(1.0)
                        / scale;
                    let left = bounds.origin.x
                        + px((f32::from(bounds.size.width) - unit * (width + 8) as f32) / 2.0);
                    window.paint_quad(gpui::fill(bounds, gpui::rgb(0xffffff)));
                    for y in 0..width {
                        for x in 0..width {
                            if modules[y * width + x] == qrcode::Color::Dark {
                                window.paint_quad(gpui::fill(
                                    gpui::Bounds::new(
                                        gpui::point(
                                            left + px((x + 4) as f32 * unit),
                                            bounds.origin.y + px((y + 4) as f32 * unit),
                                        ),
                                        gpui::size(px(unit), px(unit)),
                                    ),
                                    gpui::rgb(0x000000),
                                ));
                            }
                        }
                    }
                },
            )
            .w(px(300.))
            .h(px(300.)),
        )
        .into_any_element()
}
pub fn command_block(id: impl Into<gpui::ElementId>, command: String) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .min_h(px(84.))
        .p_3()
        .bg(rgb(CUE_UI.palette.sidebar))
        .border_1()
        .border_color(rgb(CUE_UI.palette.border))
        .rounded(px(ui::FIELD_RADIUS))
        .overflow_x_scroll()
        .font_family("Menlo")
        .text_size(px(11.))
        .line_height(px(18.))
        .child(command.clone())
        .automation(AutomationRole::Status, command)
}

#[cfg(feature = "stories")]
pub struct NetworkStory {
    family: String,
    data: NetworkData,
    enrollment: EnrollmentData,
    name: gpui::Entity<crate::components::text_input::ComposerInput>,
    origin: gpui::Entity<crate::components::text_input::ComposerInput>,
    addr: gpui::Entity<crate::components::text_input::ComposerInput>,
    grant: bool,
    focus: [FocusHandle; 2],
    modal: crate::modal::ModalState,
    open: bool,
}
#[cfg(feature = "stories")]
impl NetworkStory {
    pub fn new(family: String, state: String, cx: &mut Context<Self>) -> Self {
        let f = crate::stories::page_fixture();
        let mut field = |placeholder| {
            let input =
                cx.new(|cx| crate::components::text_input::ComposerInput::new(placeholder, cx));
            cx.observe(&input, |_, _, cx| cx.notify()).detach();
            input
        };
        let name = field("设备名称");
        let origin = field("key: 设备身份");
        let addr = field("局域网地址（可选）");
        let active = state == "command";
        let status = if active {
            "waiting"
        } else if state == "expired" {
            "expired"
        } else {
            ""
        };
        Self {
            open: family == "enrollment" || state == "manual",
            family,
            data: NetworkData {
                enabled: true,
                available: true,
                peers: if state == "empty" {
                    vec![]
                } else {
                    vec![Peer {
                        id: "mini2".into(),
                        name: "mini2".into(),
                        status: "已连接".into(),
                        permission: "客户端 · 可管理此节点".into(),
                    }]
                },
                identity: Some("device-demo-01".into()),
                busy: false,
                notice: None,
            },
            enrollment: EnrollmentData {
                client: false,
                ticket: String::new(),
                available: true,
                busy: state == "loading",
                status: status.into(),
                command: if active {
                    f["mesh"]["invitation"].as_str().unwrap().into()
                } else {
                    String::new()
                },
                status_label: if active {
                    "等待目标设备执行 · 10 分 0 秒后过期".into()
                } else if state == "expired" {
                    "加入命令已过期，请重新生成。".into()
                } else {
                    String::new()
                },
                notice: (state == "error").then(|| "获取加入命令失败，请重试。".into()),
            },
            name,
            origin,
            addr,
            grant: false,
            focus: [cx.focus_handle(), cx.focus_handle()],
            modal: crate::modal::ModalState::new(cx),
        }
    }
    fn enrollment_action(&mut self, action: EnrollmentAction, cx: &mut Context<Self>) {
        match action {
            EnrollmentAction::Select(phone) => {
                self.enrollment.client = phone;
                self.enrollment.status.clear();
                self.enrollment.status_label.clear();
                self.enrollment.command.clear();
                self.enrollment.ticket.clear();
                self.enrollment.notice = None;
            }
            EnrollmentAction::CreateClient | EnrollmentAction::Create => {
                self.enrollment.busy = false;
                self.enrollment.status = "waiting".into();
                self.enrollment.status_label = "等待目标设备执行 · 10 分 0 秒后过期".into();
                self.enrollment.command = crate::stories::page_fixture()["mesh"]["invitation"]
                    .as_str()
                    .unwrap()
                    .into();
                self.enrollment.notice = None;
            }
            EnrollmentAction::Revoke => {
                self.enrollment.status = "revoked".into();
                self.enrollment.status_label = "加入命令已撤销。".into();
                self.enrollment.command.clear();
            }
            EnrollmentAction::Approve => {
                self.enrollment.status = "connecting".into();
            }
            EnrollmentAction::Copy => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                    self.enrollment.command.clone(),
                ));
                self.enrollment.notice = Some("加入命令已复制。".into());
            }
        }
        cx.notify();
    }
}
#[cfg(feature = "stories")]
impl gpui::Render for NetworkStory {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enrollment_only = self.family == "enrollment";
        self.modal.sync(
            self.open.then_some(if enrollment_only {
                "add-device-dialog"
            } else {
                "mesh-peer-dialog"
            }),
            window,
            cx,
        );
        let invite = || {
            enrollment(self.enrollment.clone(), cx, |v, event, cx| {
                v.enrollment_action(event, cx)
            })
        };
        if enrollment_only {
            return div()
                .child(
                    ui::page_action("storybook-add-device", "连接设备").on_click(cx.listener(
                        |v, _, _, cx| {
                            v.open = true;
                            cx.notify();
                        },
                    )),
                )
                .when(self.open, |v| {
                    v.child(ui::detail_modal(
                        "add-device-dialog",
                        "连接设备",
                        invite(),
                        None,
                        &self.modal.focus,
                        window,
                        cx,
                        true,
                        |v, _, cx| {
                            v.open = false;
                            cx.notify();
                        },
                    ))
                })
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .child(network(
                self.data.clone(),
                &self.focus[0],
                cx,
                |v, event, cx| {
                    match event {
                        NetworkAction::Toggle(on) => v.data.enabled = on,
                        NetworkAction::Refresh => {
                            v.data.busy = false;
                            v.data.notice = Some("连接信息已刷新。".into())
                        }
                        NetworkAction::CopyIdentity => {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                v.data.identity.clone().unwrap_or_default(),
                            ));
                            v.data.notice = Some("节点身份已复制。".into());
                        }
                        NetworkAction::Add => {
                            v.open = true;
                            v.data.notice = None
                        }
                        NetworkAction::Remove(id) => v.data.peers.retain(|p| p.id != id),
                    }
                    cx.notify();
                },
            ))
            .child(
                div()
                    .mt_6()
                    .pt_5()
                    .border_t_1()
                    .border_color(rgb(CUE_UI.palette.border))
                    .child(invite()),
            )
            .when(self.open, |v| {
                v.child(ui::modal(
                    "mesh-peer-dialog",
                    "手动连接设备",
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(ui::field("mesh-peer-name", "设备名称", &self.name, cx))
                        .child(ui::field("mesh-peer-origin", "设备身份", &self.origin, cx))
                        .child(ui::field(
                            "mesh-peer-addr",
                            "局域网地址 · 可选",
                            &self.addr,
                            cx,
                        ))
                        .child(row(
                            "允许作为客户端管理",
                            "可管理此设备的模型连接、Agent 和任务。",
                            ui::switch(
                                "mesh-client-grant",
                                "客户端权限",
                                self.grant,
                                true,
                                &self.focus[1],
                                cx,
                                |v, on, cx| {
                                    v.grant = on;
                                    cx.notify();
                                },
                            ),
                        )),
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            ui::button("mesh-cancel-peer", "取消", false, true).on_click(
                                cx.listener(|v, _, _, cx| {
                                    v.open = false;
                                    v.data.notice = None;
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            ui::button("mesh-add-peer", "保存配对", true, true).on_click(
                                cx.listener(|v, _, _, cx| {
                                    let name = v.name.read(cx).value().trim().to_owned();
                                    let origin = v.origin.read(cx).value().trim().to_owned();
                                    if name.is_empty() {
                                        v.data.notice = Some("请填写设备名称".into())
                                    } else if origin.is_empty() {
                                        v.data.notice = Some("请填写设备身份".into())
                                    } else {
                                        v.data.peers.push(Peer {
                                            id: origin,
                                            name,
                                            status: "已连接".into(),
                                            permission: if v.grant {
                                                "客户端 · 可管理此节点"
                                            } else {
                                                "节点 · 按 Worker 授权协作"
                                            }
                                            .into(),
                                        });
                                        v.open = false;
                                        v.data.notice = None;
                                    }
                                    cx.notify();
                                }),
                            ),
                        ),
                    self.data.notice.clone(),
                    &self.modal.focus,
                    window,
                    cx,
                    true,
                    |v, _, cx| {
                        v.open = false;
                        v.data.notice = None;
                        cx.notify();
                    },
                ))
            })
            .into_any_element()
    }
}
