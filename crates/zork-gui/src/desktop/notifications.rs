//! Platform delivery only. Classification, suppression and receipts belong to core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    #[default]
    Unknown,
    NotDetermined,
    Allowed,
    Denied,
    Unavailable,
}
impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "notification_permission_unknown",
            Self::NotDetermined => "notification_permission_not_requested",
            Self::Allowed => "notification_permission_allowed",
            Self::Denied => "notification_permission_denied",
            Self::Unavailable => "notification_permission_unavailable",
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{accepted, permission, post};
#[cfg(not(target_os = "macos"))]
pub async fn permission(_: bool) -> anyhow::Result<Permission> {
    Ok(Permission::Allowed)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Permission;
    use anyhow::{Context, Result};
    use block2::RcBlock;
    use futures_channel::oneshot;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationRequest, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter,
    };
    use std::{ptr::NonNull, sync::Mutex};

    pub async fn permission(request: bool) -> Result<Permission> {
        // UserNotifications raises an Objective-C exception outside an app bundle.
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            return Ok(Permission::Unavailable);
        }
        let (send, receive) = oneshot::channel();
        let send = Mutex::new(Some(send));
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&RcBlock::new(
                move |settings: NonNull<UNNotificationSettings>| {
                    // The framework owns this object for the callback's duration.
                    let status = unsafe { settings.as_ref() }.authorizationStatus();
                    let permission = match status {
                        UNAuthorizationStatus::NotDetermined => Permission::NotDetermined,
                        UNAuthorizationStatus::Denied => Permission::Denied,
                        UNAuthorizationStatus::Authorized
                        | UNAuthorizationStatus::Provisional
                        | UNAuthorizationStatus::Ephemeral => Permission::Allowed,
                        _ => Permission::Unknown,
                    };
                    if let Some(send) = send.lock().unwrap().take() {
                        let _ = send.send(permission);
                    }
                },
            ));
        let status = receive
            .await
            .context("notification settings callback closed")?;
        if !request || status != Permission::NotDetermined {
            return Ok(status);
        }
        let (send, receive) = oneshot::channel();
        let send = Mutex::new(Some(send));
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &RcBlock::new(move |granted: Bool, error: *mut NSError| {
                    // Non-null NSError is borrowed only within the completion block.
                    let result = if let Some(error) = unsafe { error.as_ref() } {
                        Err(anyhow::anyhow!("{}", error.localizedDescription()))
                    } else {
                        Ok(if granted.as_bool() {
                            Permission::Allowed
                        } else {
                            Permission::Denied
                        })
                    };
                    if let Some(send) = send.lock().unwrap().take() {
                        let _ = send.send(result);
                    }
                }),
            );
        receive
            .await
            .context("notification authorization callback closed")?
    }

    /// Success means the OS accepted the request, not that the user saw it.
    /// The GPUI-owned delegate continues to route clicks on this same center.
    pub async fn post(
        notification: gpui::SystemNotification,
        sound: bool,
        event: &str,
    ) -> Result<()> {
        anyhow::ensure!(
            NSBundle::mainBundle().bundleIdentifier().is_some(),
            "notifications require an application bundle"
        );
        let (send, receive) = oneshot::channel();
        {
            let content = UNMutableNotificationContent::new();
            let event_key = NSString::from_str("zork-event");
            let event_value = NSString::from_str(event);
            let info = NSDictionary::from_slices(&[&*event_key], &[&*event_value]);
            // This property-list dictionary contains only NSString keys and values.
            unsafe {
                content.setUserInfo(info.cast_unchecked());
            }
            content.setThreadIdentifier(&NSString::from_str(&notification.tag));
            content.setTitle(&NSString::from_str(&notification.title));
            content.setBody(&NSString::from_str(&notification.body));
            if sound {
                content.setSound(Some(&UNNotificationSound::defaultSound()));
            }
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&notification.tag),
                &content,
                None,
            );
            let send = Mutex::new(Some(send));
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(
                    &request,
                    Some(&RcBlock::new(move |error: *mut NSError| {
                        let result = if let Some(error) = unsafe { error.as_ref() } {
                            Err(anyhow::anyhow!("{}", error.localizedDescription()))
                        } else {
                            Ok(())
                        };
                        if let Some(send) = send.lock().unwrap().take() {
                            let _ = send.send(result);
                        }
                    })),
                );
        }
        receive
            .await
            .context("notification delivery callback closed")?
    }
    fn matches(request: &UNNotificationRequest, tag: &str, event: &str) -> bool {
        if request.identifier().to_string() != tag {
            return false;
        }
        request
            .content()
            .userInfo()
            .objectForKey(&NSString::from_str("zork-event"))
            .and_then(|value| value.downcast::<NSString>().ok())
            .is_some_and(|value| value.to_string() == event)
    }
    #[test]
    fn recovery_uses_event_identity_instead_of_repeated_visible_text() {
        let content = UNMutableNotificationContent::new();
        content.setBody(&NSString::from_str("New reply"));
        let key = NSString::from_str("zork-event");
        let value = NSString::from_str("event-1");
        let info = NSDictionary::from_slices(&[&*key], &[&*value]);
        unsafe {
            content.setUserInfo(info.cast_unchecked());
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str("tag"),
            &content,
            None,
        );
        assert!(matches(&request, "tag", "event-1"));
        assert!(!matches(&request, "tag", "event-2"));
        assert!(!matches(&request, "another-node", "event-1"));
    }
    pub async fn accepted(tag: &str, event: &str) -> Result<bool> {
        let (send, receive) = oneshot::channel();
        let send = Mutex::new(Some(send));
        let delivered_tag = tag.to_owned();
        let delivered_event = event.to_owned();
        UNUserNotificationCenter::currentNotificationCenter()
            .getDeliveredNotificationsWithCompletionHandler(&RcBlock::new(
                move |values: NonNull<NSArray<UNNotification>>| {
                    let found = unsafe { values.as_ref() }.iter().any(|notification| {
                        matches(&notification.request(), &delivered_tag, &delivered_event)
                    });
                    if let Some(send) = send.lock().unwrap().take() {
                        let _ = send.send(found);
                    }
                },
            ));
        if receive
            .await
            .context("notification recovery callback closed")?
        {
            return Ok(true);
        }
        let (send, receive) = oneshot::channel();
        let send = Mutex::new(Some(send));
        let tag = tag.to_owned();
        let event = event.to_owned();
        UNUserNotificationCenter::currentNotificationCenter()
            .getPendingNotificationRequestsWithCompletionHandler(&RcBlock::new(
                move |values: NonNull<NSArray<UNNotificationRequest>>| {
                    let found = unsafe { values.as_ref() }
                        .iter()
                        .any(|request| matches(&request, &tag, &event));
                    if let Some(send) = send.lock().unwrap().take() {
                        let _ = send.send(found);
                    }
                },
            ));
        receive
            .await
            .context("pending notification recovery callback closed")
    }
}

use super::{ui, DesktopRoot};
use crate::automation::{AutomationElementExt, AutomationRole};
use gpui::{div, prelude::*, px, rgb, Context, Div};

impl DesktopRoot {
    pub(super) fn open_notification(&mut self, tag: String, cx: &mut Context<Self>) {
        cx.activate(true);
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| window.activate_window());
        }
        if tag == "zork-notification-test" {
            cx.dismiss_system_notification(&tag);
            return;
        }
        if self.busy {
            self.pending_notification = Some(tag);
            cx.notify();
            return;
        }
        match zork_client_core::notifications::resolve(&self.store, &tag) {
            Ok(Some(notice)) => {
                let destination = destination(&notice);
                self.navigate_device(
                    super::navigation::Navigate {
                        node: Some(notice.node.clone()),
                        destination,
                    },
                    cx,
                );
                if notice.session.is_none() {
                    self.error = Some(format!(
                        "{} · {}",
                        self.client_settings.locale.text("notification_no_session"),
                        notice.title
                    ));
                }
                cx.dismiss_system_notification(&tag);
            }
            Ok(None) => {
                self.error = Some(
                    self.client_settings
                        .locale
                        .text("notification_unavailable")
                        .into(),
                )
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn refresh_notification_permission(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = permission(false).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(permission) => view.client_settings.notification_permission = permission,
                    Err(error) => view.client_settings.notification_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn save_notification_preference(
        &mut self,
        edit: impl FnOnce(&mut zork_client_core::notifications::Preferences),
        cx: &mut Context<Self>,
    ) {
        let result = (|| -> anyhow::Result<()> {
            let mut prefs = zork_client_core::notifications::preferences(&self.store)?;
            edit(&mut prefs);
            zork_client_core::notifications::save_preferences(&self.store, &prefs)
        })();
        self.client_settings.notification_error = result.err().map(|error| error.to_string());
        cx.notify();
    }
    fn test_notification(&mut self, cx: &mut Context<Self>) {
        if self.client_settings.notification_busy {
            return;
        }
        match zork_client_core::notifications::preferences(&self.store) {
            Ok(prefs) if !prefs.enabled => return,
            Ok(_) => {}
            Err(error) => {
                self.client_settings.notification_error = Some(error.to_string());
                cx.notify();
                return;
            }
        }
        self.client_settings.notification_busy = true;
        self.client_settings.notification_error = None;
        let store = self.store.clone();
        let body = self.client_settings.locale.text("notification_test_body");
        cx.spawn(async move |this, cx| {
            let result = async {
                let status = permission(true).await?;
                if status != Permission::Allowed {
                    return Ok::<_, anyhow::Error>(status);
                }
                let prefs = zork_client_core::notifications::preferences(&store)?;
                if !prefs.enabled {
                    return Ok(status);
                }
                let notification = gpui::SystemNotification {
                    tag: "zork-notification-test".into(),
                    title: "Zork".into(),
                    body: body.into(),
                    actions: vec![],
                };
                #[cfg(target_os = "macos")]
                post(notification, prefs.sound, &ulid::Ulid::new().to_string()).await?;
                #[cfg(not(target_os = "macos"))]
                this.update(cx, |_, cx| cx.show_system_notification(notification))?;
                Ok(status)
            }
            .await;
            let _ = this.update(cx, |view, cx| {
                view.client_settings.notification_busy = false;
                match result {
                    Ok(permission) => view.client_settings.notification_permission = permission,
                    Err(error) => view.client_settings.notification_error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn render_notification_settings(&self, cx: &mut Context<Self>) -> Div {
        let locale = self.client_settings.locale;
        let t = |key| locale.text(key);
        let p = crate::design::CUE_UI.palette;
        let prefs = match zork_client_core::notifications::preferences(&self.store) {
            Ok(value) => value,
            Err(error) => return div().child(error.to_string()),
        };
        let mut content = div().flex().flex_col().gap_3();
        for (index, (id, title, detail, selected)) in [
            (
                "notifications-toggle",
                "notification_enabled",
                "notification_enabled_detail",
                prefs.enabled,
            ),
            (
                "notifications-preview",
                "notification_preview",
                "notification_preview_detail",
                prefs.preview,
            ),
            (
                "notifications-sound",
                "notification_sound",
                "notification_sound_detail",
                prefs.sound,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            content = content.child(zork_ui::settings::row(
                t(title),
                t(detail),
                ui::switch(
                    id,
                    t(title),
                    selected,
                    true,
                    &self.notification_switch_focus[index],
                    cx,
                    move |view, on, cx| {
                        view.save_notification_preference(
                            |prefs| match id {
                                "notifications-toggle" => prefs.enabled = on,
                                "notifications-preview" => prefs.preview = on,
                                _ => prefs.sound = on,
                            },
                            cx,
                        )
                    },
                ),
            ));
        }
        if let Some((node, session)) = self.active_node_id.as_ref().and_then(|node| {
            self.active.as_ref().and_then(|root| {
                root.read(cx)
                    .navigation_selection()
                    .0
                    .selected_session
                    .map(|session| (node.clone(), session))
            })
        }) {
            let muted = prefs.muted.contains(&(node.clone(), session.clone()));
            content = content.child(zork_ui::settings::row(
                t("notification_mute"),
                t("notification_mute_detail"),
                ui::switch(
                    "notification-mute-current",
                    t("notification_mute"),
                    muted,
                    true,
                    &self.notification_switch_focus[3],
                    cx,
                    move |view, on, cx| {
                        view.save_notification_preference(
                            |prefs| {
                                if on {
                                    prefs.muted.insert((node.clone(), session.clone()));
                                } else {
                                    prefs.muted.remove(&(node.clone(), session.clone()));
                                }
                            },
                            cx,
                        )
                    },
                ),
            ));
        }
        content = content
            .child(
                zork_ui::settings::row(
                    t("notification_system_permission"),
                    t(self.client_settings.notification_permission.label()),
                    ui::button(
                        "notifications-permission-refresh",
                        t("notification_check_permission"),
                        false,
                        true,
                    )
                    .on_click(
                        cx.listener(|view, _, _, cx| view.refresh_notification_permission(cx)),
                    )
                    .automation(AutomationRole::Button, t("notification_check_permission")),
                )
                .id("notifications-permission-status")
                .automation(
                    AutomationRole::Status,
                    t(self.client_settings.notification_permission.label()),
                ),
            )
            .child(
                ui::button(
                    "notifications-test",
                    t("notification_test"),
                    false,
                    prefs.enabled && !self.client_settings.notification_busy,
                )
                .on_click(cx.listener(|view, _, _, cx| view.test_notification(cx)))
                .automation_enabled(
                    prefs.enabled && !self.client_settings.notification_busy,
                    AutomationRole::Button,
                    t("notification_test"),
                ),
            )
            .when(cfg!(target_os = "macos"), |content| {
                content.child(
                    ui::button(
                        "notifications-system-settings",
                        t("notification_system_settings"),
                        false,
                        true,
                    )
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.open_url(
                            "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
                        )
                    }))
                    .automation(AutomationRole::Button, t("notification_system_settings")),
                )
            })
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(p.muted))
                    .child(t("notification_permission_detail")),
            );
        if let Some(error) = &self.client_settings.notification_error {
            content = content.child(div().text_color(rgb(p.danger)).child(error.clone()));
        }
        content
    }
}

fn destination(notice: &zork_client_core::notifications::Notice) -> super::navigation::Destination {
    if let Some(session) = &notice.session {
        super::navigation::Destination::Conversation {
            session: session.clone(),
            leader: notice.leader.clone(),
        }
    } else if let Some(leader) = &notice.leader {
        super::navigation::Destination::Leader(leader.clone())
    } else {
        super::navigation::Destination::Home
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_routes_use_session_identity_and_never_treat_task_ids_as_sessions() {
        let mut notice = zork_client_core::notifications::Notice {
            id: "event".into(),
            node: "node".into(),
            session: Some("session".into()),
            task: Some("task".into()),
            leader: Some("leader".into()),
            title: "Title".into(),
            kind: zork_client_core::notifications::Kind::Review,
            created_at_ms: 0,
        };
        assert!(
            matches!(destination(&notice), super::super::navigation::Destination::Conversation { session, .. } if session == "session")
        );
        notice.session = None;
        assert!(
            matches!(destination(&notice), super::super::navigation::Destination::Leader(id) if id == "leader")
        );
        notice.leader = None;
        assert!(matches!(
            destination(&notice),
            super::super::navigation::Destination::Home
        ));
    }
}
