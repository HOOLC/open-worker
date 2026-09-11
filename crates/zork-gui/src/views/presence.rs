//! Composer-local presence. Timers exist only for a transition or idle grace period.
use super::*;
use crate::api::ParticipantStatus;
use std::time::Instant;

mod preview;

const MOVE: Duration = Duration::from_millis(500);
const IDLE_GRACE: Duration = Duration::from_millis(1200);
// Logical pixels per second, shared with Android's dp-based animation.
const WIDTH_SPEED: f32 = 1440.;
use zork_ui::components::liquid_composer::{
    DOCK_GAP, EDGE, IMMERSION, RADIUS, ROW_SPACING, SPACING,
};
const AVATAR: f32 = RADIUS * 1.4;
const ROW: f32 = ROW_SPACING;
const FIRST_LIFT: f32 = RADIUS + DOCK_GAP;
const PORTRAIT_INSET: f32 = RADIUS - AVATAR / 2.;
const LABEL_GAP: f32 = 8.;
const LABEL_RIGHT_PADDING: f32 = 12.;

fn reveal(progress: f32) -> f32 {
    ((progress - 0.45) / 0.55).clamp(0., 1.)
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Position {
    x: f32,
    y: f32,
    label: f32,
}

struct WidthMotion {
    from: f32,
    to: f32,
    began: Instant,
}
impl WidthMotion {
    fn sample(&self, now: Instant) -> f32 {
        let distance = self.to - self.from;
        let travelled = WIDTH_SPEED * now.duration_since(self.began).as_secs_f32();
        self.from + distance.signum() * travelled.min(distance.abs())
    }
    fn retarget(&mut self, target: f32, now: Instant, snap: bool) {
        if snap {
            self.from = target;
            self.to = target;
            self.began = now;
        } else if self.to != target {
            self.from = self.sample(now);
            self.to = target;
            self.began = now;
        }
    }
    fn moving(&self, now: Instant) -> bool {
        WIDTH_SPEED * now.duration_since(self.began).as_secs_f32() < (self.to - self.from).abs()
    }
}

struct Member {
    info: ParticipantStatus,
    label: String,
    measured_label: String,
    text_width: f32,
    width: WidthMotion,
    failed: bool,
    idle_since: Option<Instant>,
    expanded: bool,
    from: Position,
    velocity: Position,
    to: Position,
    began: Instant,
}

impl Member {
    fn sample(&self, now: Instant) -> (Position, Position) {
        let elapsed = now.duration_since(self.began);
        if elapsed >= MOVE {
            return (self.to, Position::default());
        }
        // Critically damped spring: zero initial speed, continuous position and
        // velocity on retarget, no canned easing restart or decorative bounce.
        let t = elapsed.as_secs_f32();
        let omega = 10.08 / MOVE.as_secs_f32();
        let decay = (-omega * t).exp();
        let axis = |from: f32, to: f32, velocity: f32| {
            let displacement = from - to;
            let coefficient = velocity + omega * displacement;
            (
                to + (displacement + coefficient * t) * decay,
                (velocity - omega * coefficient * t) * decay,
            )
        };
        let (x, vx) = axis(self.from.x, self.to.x, self.velocity.x);
        let (y, vy) = axis(self.from.y, self.to.y, self.velocity.y);
        let (label, vl) = axis(self.from.label, self.to.label, self.velocity.label);
        (
            Position { x, y, label },
            Position {
                x: vx,
                y: vy,
                label: vl,
            },
        )
    }
    fn position(&self, now: Instant) -> Position {
        self.sample(now).0
    }
    fn update_width(&mut self, target: f32, now: Instant, reduced: bool) {
        let position = self.position(now);
        let lifting = self.expanded
            && !reduced
            && ((position.x - self.to.x).abs() > 0.001 || (position.y - self.to.y).abs() > 0.001);
        let target = if lifting {
            self.width.sample(now)
        } else if self.expanded {
            target
        } else {
            0.
        };
        self.width.retarget(target, now, reduced);
    }
    fn moving(&self, now: Instant) -> bool {
        self.width.moving(now)
            || ((self.from != self.to || self.velocity != Position::default())
                && now.duration_since(self.began) < MOVE)
    }
}

#[derive(Default)]
pub(super) struct Presence {
    #[cfg(feature = "headless-bench")]
    previews: HashMap<String, Arc<zork_client_core::state::HistoryData>>,
    members: Vec<Member>,
    preview: Option<Entity<preview::Overlay>>,
    max_label_width: Option<f32>,
    wake: Option<Task<()>>,
    frame_time: Option<Instant>,
    pub(super) extent: f32,
    surface: zork_ui::components::liquid_composer::SurfaceCache,
}

impl Presence {
    fn reconcile(
        &mut self,
        members: Vec<ParticipantStatus>,
        now: Instant,
        reduced: bool,
        locale: Locale,
    ) {
        self.members
            .retain(|old| members.iter().any(|m| m.id == old.info.id));
        for info in members {
            let index = self
                .members
                .iter()
                .position(|m| m.info.id == info.id)
                .unwrap_or_else(|| {
                    let position = Position {
                        x: self.members.len() as f32 * SPACING,
                        y: -IMMERSION,
                        ..Position::default()
                    };
                    self.members.push(Member {
                        info: info.clone(),
                        label: String::new(),
                        measured_label: String::new(),
                        text_width: 0.,
                        width: WidthMotion {
                            from: 0.,
                            to: 0.,
                            began: now,
                        },
                        failed: false,
                        idle_since: None,
                        expanded: false,
                        from: position,
                        velocity: Position::default(),
                        to: position,
                        began: now,
                    });
                    self.members.len() - 1
                });
            let member = &mut self.members[index];
            let status = info
                .activity
                .as_ref()
                .filter(|status| should_render_live_activity(status, None));
            if let Some(status) = status {
                member.label = agent_status_label(status, locale);
                member.failed = matches!(status, AgentStatus::Failed { .. });
                member.idle_since = None;
                member.expanded = true;
            } else if member.expanded {
                let since = *member.idle_since.get_or_insert(now);
                member.label = locale.text("presence_idle").into();
                member.failed = false;
                if now.duration_since(since) >= IDLE_GRACE {
                    member.expanded = false;
                    member.idle_since = None;
                }
            }
            member.info = info;
        }
        // Existing members keep their order even when events reorder the snapshot.
        let mut idle = 0;
        let mut active = 0;
        for member in &mut self.members {
            let target = if member.expanded {
                active += 1;
                Position {
                    x: zork_ui::components::liquid_composer::ACTIVE_EDGE - EDGE,
                    y: FIRST_LIFT + (active - 1) as f32 * ROW,
                    label: 1.,
                }
            } else {
                let x = idle as f32 * SPACING;
                idle += 1;
                Position {
                    x,
                    y: -IMMERSION,
                    label: 0.,
                }
            };
            if target != member.to {
                (member.from, member.velocity) = member.sample(now);
                member.to = target;
                member.began = now;
            }
            if reduced {
                member.from = member.to;
                member.velocity = Position::default();
            }
        }
    }

    fn next_wake(&self, now: Instant) -> Option<Duration> {
        if self.members.iter().any(|m| m.moving(now)) {
            return Some(Duration::from_nanos(8_333_333));
        }
        self.members
            .iter()
            .filter_map(|m| {
                m.idle_since
                    .map(|since| IDLE_GRACE.saturating_sub(now.duration_since(since)))
            })
            .min()
    }
}

impl RootView {
    #[cfg(feature = "headless-bench")]
    pub fn benchmark_presence_history(
        &mut self,
        member: &str,
        data: zork_client_core::state::HistoryData,
        cx: &mut Context<Self>,
    ) {
        self.presence.previews.insert(member.into(), Arc::new(data));
        zork_ui::components::region::invalidate(cx, &["composer"]);
    }
    pub(super) fn conversation_members(&self) -> Vec<ParticipantStatus> {
        self.participants.clone()
    }

    pub(super) fn prepare_presence_frame(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.advance_file_fans(window, cx);
        let now = cx.background_executor().now();
        let file_width = self.file_fan_dimensions().0;
        self.presence.max_label_width = Some(
            (self.composer_surface_width
                - file_width
                - EDGE
                - PORTRAIT_INSET * 2.
                - AVATAR
                - if file_width > 0. { 16. } else { 0. })
            .max(0.),
        );
        self.presence.reconcile(
            self.conversation_members(),
            now,
            cx.reduce_motion(),
            self.locale,
        );
        for member in &mut self.presence.members {
            let label = format!("{} · {}", member.info.name, member.label);
            if label != member.measured_label {
                let run = gpui::TextRun {
                    len: label.len(),
                    font: gpui::font("Inter Variable"),
                    color: rgb(TEXT).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                member.text_width = window
                    .text_system()
                    .shape_line(label.clone().into(), px(12.), &[run], None)
                    .width
                    .as_f32();
                member.measured_label = label;
            }
            let width = (LABEL_GAP + member.text_width + LABEL_RIGHT_PADDING - PORTRAIT_INSET)
                .min(self.presence.max_label_width.unwrap_or(f32::MAX));
            member.update_width(width, now, cx.reduce_motion());
        }
        self.presence.frame_time = Some(now);
        let extent = self
            .presence
            .members
            .iter()
            .map(|m| m.position(now).y + RADIUS)
            .fold(0., f32::max);
        if (extent - self.presence.extent).abs() > 0.001 {
            self.presence.extent = extent;
            // The list and bubbles consume the same sample in this frame.
            // Tail mode follows padding; a historical scroll anchor stays put.
            zork_ui::components::region::invalidate(cx, &["transcript"]);
        }
    }

    pub(super) fn presence_surface(
        &self,
        cx: &Context<Self>,
        opening: Option<zork_ui::components::attachment_fan::Opening>,
    ) -> gpui::AnyElement {
        use zork_ui::components::liquid_composer::Bubble;
        let now = self
            .presence
            .frame_time
            .unwrap_or_else(|| cx.background_executor().now());
        let bubbles = self
            .presence
            .members
            .iter()
            .map(|member| {
                let p = member.position(now);
                Bubble {
                    x: EDGE + p.x,
                    lift: p.y,
                    width: (AVATAR + member.width.sample(now))
                        .min(self.composer_surface_width - EDGE - PORTRAIT_INSET * 2.)
                        + PORTRAIT_INSET * 2.,
                }
            })
            .collect();
        let height =
            self.composer_editor_height + zork_ui::components::liquid_composer::COMPOSER_CHROME;
        if let Some(opening) = opening {
            self.presence
                .surface
                .element_with_attachments(height, bubbles, opening)
                .into_any_element()
        } else {
            self.presence
                .surface
                .element(height, bubbles)
                .into_any_element()
        }
    }

    pub(super) fn render_presence(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let now = self
            .presence
            .frame_time
            .unwrap_or_else(|| cx.background_executor().now());
        self.presence.wake = None;
        if self
            .presence
            .members
            .iter()
            .any(|member| member.moving(now))
        {
            let root = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = root.update(cx, |_, cx| {
                    zork_ui::components::region::invalidate(cx, &["composer"]);
                });
            });
        } else if let Some(delay) = self.presence.next_wake(now) {
            self.presence.wake = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |_, cx| {
                    zork_ui::components::region::invalidate(cx, &["composer"]);
                });
            }));
        }
        let height = self.presence.extent;
        let count = self.presence.members.len();
        let preview = self
            .presence
            .preview
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    zork_ui::components::region::forget_on_release(cx);
                    preview::Overlay::default()
                })
            })
            .clone();
        preview.update(cx, |v, cx| {
            v.retain(self.presence.members.iter().map(|m| &m.info.id), cx)
        });
        div()
            .relative()
            .w_full()
            .max_w(px(self.composer_surface_width))
            .h(px(if count == 0 {
                zork_ui::components::liquid_composer::TOP_EXTENSION
            } else {
                height + zork_ui::components::liquid_composer::TOP_EXTENSION
            }))
            .children(self.presence.members.iter().map(|member| {
                let position = member.position(now);
                let info = member.info.clone();
                let name = info.name.clone();
                let label = format!("{} · {}", name, member.label);
                let preview_member = info.clone();
                let device = self.core_device.clone();
                let conversation = self.core_conversation.clone();
                let locale = self.locale;
                #[cfg(feature = "headless-bench")]
                let offline = self.benchmark_offline;
                #[cfg(not(feature = "headless-bench"))]
                let offline = false;
                let is_selected = self.selected_session.as_deref() == Some(&info.session_id);
                #[cfg(feature = "headless-bench")]
                let fixture = self.presence.previews.get(&info.id).cloned();
                let anchor = window.use_keyed_state(format!("member-preview-anchor-{}", info.id), cx, |_, _| gpui::Bounds::default());
                let measured_anchor = anchor.clone();
                let hover_preview = preview.clone();
                let layout_preview = preview.clone();
                let anchor_id = info.id.clone();
                div()
                    .absolute()
                    // Only the member's actual row blocks input. Its otherwise
                    // transparent full-width rail overlaps the attachment fan.
                    .occlude()
                    .left(px(EDGE + PORTRAIT_INSET + position.x))
                    .w(px((AVATAR + member.width.sample(now)).min(
                        self.composer_surface_width - EDGE - PORTRAIT_INSET * 2.,
                    )))
                    .bottom(px(position.y
                        + zork_ui::components::liquid_composer::TOP_EXTENSION
                        - AVATAR / 2.))
                    .h(px(AVATAR))
                    .flex()
                    .items_center()
                    .gap(px(LABEL_GAP))
                    .child(
                        div()
                            .id(format!("composer-member-{}", info.id))
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .relative()
                            .size(px(AVATAR))
                            .flex_shrink_0()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .child(zork_ui::components::motion::HoverFill {
                                id: format!("composer-member-hover-{}", info.id).into(),
                                color: zork_ui::design::INTERACTION.neutral_hover,
                                radius: 8.,
                                pressed: None,
                            })
                            .child(zork_ui::controls::agent_portrait(
                                info.avatar.as_deref(),
                                AVATAR,
                            ))
                            .on_hover(move |hovered, _, cx| {
                                let bounds = *anchor.read(cx);
                                hover_preview.update(cx, |v, cx| {
                                    if !hovered {
                                        v.leave(&preview_member.id, cx);
                                        return;
                                    }
                                    v.show(&preview_member.id, bounds, |cx| {
                                        #[cfg(feature = "headless-bench")]
                                        if let Some(state) = &fixture {
                                            return cx.new(|_| preview::Preview::fixture(preview_member.clone(), locale, state.clone()));
                                        }
                                        let source = (!offline && !preview_member.session_id.is_empty())
                                            .then(|| device.conversation(&preview_member.session_id));
                                        cx.new(|cx| preview::Preview::new(
                                            preview_member.clone(), locale, source,
                                            conversation.clone(), is_selected, cx,
                                        ))
                                    }, cx);
                                });
                            })
                            .child(gpui::canvas(move |bounds, _, cx| {
                                measured_anchor.update(cx, |v, _| *v = bounds);
                                layout_preview.update(cx, |v, cx| v.anchor(&anchor_id, bounds, cx));
                            }, |_, _, _, _| {}).absolute().inset_0())
                            .on_click(cx.listener(move |v, _, _, cx| {
                                v.toggle_history(&info.session_id, cx);
                            }))
                            .automation(
                                AutomationRole::Button,
                                format!("{} · {}", name, self.locale.text("history_title")),
                            ),
                    )
                    .when(reveal(position.label) > 0.01 && member.width.sample(now) > 0.01, |row| {
                        row.child(
                            div()
                                .id(format!("composer-activity-{}", member.info.id))
                                .flex_1()
                                .min_w_0()
                                .pr(px(LABEL_RIGHT_PADDING - PORTRAIT_INSET))
                                .truncate()
                                .opacity(reveal(position.label))
                                .text_size(px(12.))
                                .text_color(rgb(if member.failed {
                                    CUE_UI.palette.danger
                                } else {
                                    zork_ui::components::liquid_composer::TEXT_COLOR
                                }))
                                .child(label.clone())
                                .automation(AutomationRole::Status, label),
                        )
                    })
            }))
            .child(zork_ui::components::region::tracked_view(preview))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_moves_at_fixed_speed_and_retargets_from_its_current_position() {
        let now = Instant::now();
        let mut width = WidthMotion {
            from: 0.,
            to: 960.,
            began: now,
        };
        assert_eq!(width.sample(now + Duration::from_millis(100)), 144.);
        assert_eq!(width.sample(now + Duration::from_millis(200)), 288.);
        assert!(width.moving(now + MOVE)); // Large changes take longer than the lift animation.
        let turn = now + Duration::from_millis(200);
        width.retarget(0., turn, false);
        assert_eq!(width.sample(turn), 288.);
        assert_eq!(width.sample(turn + Duration::from_millis(100)), 144.);
        assert_eq!(width.sample(turn + Duration::from_millis(200)), 0.);
        assert!(!width.moving(turn + Duration::from_millis(200)));
        width.retarget(240., turn, true);
        assert_eq!(width.sample(turn), 240.);
        assert!(!width.moving(turn));
    }
    fn member(id: &str, activity: Option<AgentStatus>) -> ParticipantStatus {
        ParticipantStatus {
            subscribed: false,
            id: id.into(),
            name: id.into(),
            avatar: None,
            session_id: "s".into(),
            activity,
        }
    }

    #[test]
    fn active_member_lifts_before_it_extends() {
        let now = Instant::now();
        let mut dock = Presence::default();
        dock.reconcile(vec![member("a", None)], now, false, Locale::default());
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Thinking))],
            now,
            false,
            Locale::default(),
        );
        let member = &mut dock.members[0];
        member.update_width(240., now, false);
        member.update_width(240., now + MOVE / 2, false);
        assert_eq!(member.width.sample(now + MOVE / 2), 0.);
        assert!(member.position(now + MOVE / 2).y < member.to.y);
        member.update_width(240., now + MOVE, false);
        assert_eq!(member.position(now + MOVE).y, member.to.y);
        assert_eq!(member.width.sample(now + MOVE), 0.);
        assert!(member.width.sample(now + MOVE + Duration::from_millis(50)) > 0.);
    }
    #[test]
    fn idle_grace_reentry_and_quiescence() {
        let mut dock = Presence::default();
        let now = Instant::now();
        let locale = Locale::default();
        dock.reconcile(vec![member("a", None)], now, false, locale);
        assert!(dock.next_wake(now).is_none());
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Thinking))],
            now,
            false,
            locale,
        );
        let middle = now + MOVE / 2;
        assert!(dock.members[0].position(middle).y > 0.);
        assert!(dock.members[0].position(middle).y < ROW);
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Finished))],
            now + MOVE,
            false,
            locale,
        );
        assert!(dock.members[0].expanded);
        assert_eq!(dock.next_wake(now + MOVE), Some(IDLE_GRACE));
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Thinking))],
            now + MOVE + IDLE_GRACE / 2,
            false,
            locale,
        );
        assert!(dock.members[0].idle_since.is_none());
        let idle = now + MOVE + IDLE_GRACE;
        dock.reconcile(vec![member("a", None)], idle, false, locale);
        dock.reconcile(vec![member("a", None)], idle + IDLE_GRACE, false, locale);
        assert!(!dock.members[0].expanded);
        assert!(dock.next_wake(idle + IDLE_GRACE + MOVE).is_none());
        assert_eq!(
            dock.members[0].position(idle + IDLE_GRACE + MOVE).y,
            -IMMERSION
        );
    }

    #[test]
    fn resumed_work_preserves_motion_velocity() {
        let mut dock = Presence::default();
        let now = Instant::now();
        let locale = Locale::default();
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Thinking))],
            now,
            false,
            locale,
        );
        dock.reconcile(vec![member("a", None)], now + MOVE, false, locale);
        let returning = now + MOVE + IDLE_GRACE;
        dock.reconcile(vec![member("a", None)], returning, false, locale);
        let interrupted = returning + Duration::from_millis(40);
        let (before, velocity) = dock.members[0].sample(interrupted);
        assert!(velocity.y < 0.);
        dock.reconcile(
            vec![member("a", Some(AgentStatus::Thinking))],
            interrupted,
            false,
            locale,
        );
        let (after, resumed_velocity) = dock.members[0].sample(interrupted);
        assert!((before.y - after.y).abs() < 0.0001);
        assert!((velocity.y - resumed_velocity.y).abs() < 0.0001);
        assert_eq!(dock.members[0].position(interrupted + MOVE).y, FIRST_LIFT);
    }

    #[test]
    fn stable_members_waiting_failure_and_reduced_motion() {
        let mut dock = Presence::default();
        let now = Instant::now();
        let a = member(
            "a",
            Some(AgentStatus::Waiting {
                reason: "approval".into(),
                deadline_ms: 0,
            }),
        );
        let b = member(
            "b",
            Some(AgentStatus::Failed {
                reason: "error".into(),
            }),
        );
        dock.reconcile(vec![a.clone(), b.clone()], now, true, Locale::default());
        dock.reconcile(vec![b, a], now + IDLE_GRACE * 2, true, Locale::default());
        assert_eq!(dock.members[0].info.id, "a");
        assert!(dock.members.iter().all(|m| m.expanded));
        assert!(dock.members[1].failed);
        assert_eq!(dock.members[1].position(now).y, FIRST_LIFT + ROW);
        assert!(dock.next_wake(now + IDLE_GRACE * 2).is_none());
        dock.reconcile(vec![], now + IDLE_GRACE * 3, false, Locale::default());
        assert!(dock.members.is_empty());
    }
}
