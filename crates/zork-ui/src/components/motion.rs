//! Short, interruptible motion shared by native and Web controls.
use gpui::{
    prelude::*, px, rgb, Animation, AnimationExt, ElementId, SpringAnimation, SpringConfig,
};
use std::time::Duration;

const SLIDE_MIN_SECONDS: f32 = 0.04;
const SLIDE_MAX_SECONDS: f32 = 0.24;
const SLIDE_DISTANCE_SCALE: f32 = 240.;
const POSITION_EPSILON: f32 = 0.1;
const VELOCITY_EPSILON: f32 = 8.;

/// Distance-adaptive, critically damped motion shared by tabs and floating cards.
/// Only the goal changes on input; the last painted position and velocity survive.
pub(crate) struct Slide<const N: usize> {
    position: [f32; N],
    velocity: [f32; N],
    target: [f32; N],
    frequency: f32,
}

impl<const N: usize> Slide<N> {
    pub(crate) fn new(position: [f32; N]) -> Self {
        Self {
            position,
            target: position,
            velocity: [0.; N],
            frequency: 0.,
        }
    }

    pub(crate) fn position(&self) -> [f32; N] {
        self.position
    }

    pub(crate) fn retarget(&mut self, target: [f32; N]) -> bool {
        if self.target == target {
            return false;
        }
        let delta = std::array::from_fn::<_, N, _>(|i| target[i] - self.position[i]);
        let distance = length(delta);
        // Account for the current momentum even when the new goal is directly
        // under the surface. It still needs a short, continuous braking path.
        let travel = distance.max(length(self.velocity) * SLIDE_MIN_SECONDS);
        self.frequency = slide_frequency(travel);
        if distance > POSITION_EPSILON {
            // When a nearer goal lies ahead, increase braking enough to avoid
            // crossing it. Preserve velocity instead of snapping it to zero.
            let closing_rate = self
                .velocity
                .iter()
                .zip(delta)
                .map(|(v, d)| v * d)
                .sum::<f32>()
                / (distance * distance);
            self.frequency = self.frequency.max(closing_rate);
        }
        self.target = target;
        true
    }

    pub(crate) fn is_moving(&self) -> bool {
        length(std::array::from_fn::<_, N, _>(|i| {
            self.target[i] - self.position[i]
        })) > POSITION_EPSILON
            || length(self.velocity) > VELOCITY_EPSILON
    }

    pub(crate) fn advance(&mut self, dt: f32, reduced: bool) -> bool {
        if reduced || !self.is_moving() {
            self.position = self.target;
            self.velocity = [0.; N];
            return false;
        }
        // Closed-form critical damping is stable across refresh rates and
        // dropped frames; no per-frame integration loop or cached curve table.
        let dt = dt.max(0.);
        let decay = (-self.frequency * dt).exp();
        for i in 0..N {
            let error = self.position[i] - self.target[i];
            let slope = self.velocity[i] + self.frequency * error;
            self.position[i] = self.target[i] + (error + slope * dt) * decay;
            self.velocity[i] = (self.velocity[i] - self.frequency * slope * dt) * decay;
        }
        if self.is_moving() {
            true
        } else {
            self.position = self.target;
            self.velocity = [0.; N];
            false
        }
    }
}

fn length<const N: usize>(values: [f32; N]) -> f32 {
    values.iter().map(|v| v * v).sum::<f32>().sqrt()
}

fn slide_frequency(distance: f32) -> f32 {
    // About 70 / 140 / 225 ms at 34 / 160 / 600 px. Longer jumps
    // gain speed, with a soft 240 ms ceiling instead of a distance threshold.
    let seconds = SLIDE_MIN_SECONDS
        + (SLIDE_MAX_SECONDS - SLIDE_MIN_SECONDS) * -(-distance / SLIDE_DISTANCE_SCALE).exp_m1();
    // Solve (1+z)e^-z = epsilon/distance: equal absolute settling precision
    // keeps large moves from acquiring a long, visibly creeping tail.
    let log_ratio = (distance / POSITION_EPSILON).max(std::f32::consts::E).ln();
    let mut z = log_ratio + (log_ratio + 1.).ln();
    for _ in 0..4 {
        z += ((1. + z).ln() - z + log_ratio) * (1. + z) / z;
    }
    z / seconds
}

fn hover_spring_config() -> SpringConfig {
    SpringConfig::new(2500., 100., 1.)
}

pub fn spring(target: f32) -> SpringAnimation<f32> {
    // Critically damped: fast response without overshoot or a rubbery finish.
    SpringAnimation::new(hover_spring_config())
        .to(target)
        .with_epsilon(0.001)
}
pub fn enter<E: IntoElement + gpui::Styled + 'static>(
    element: E,
    id: impl Into<ElementId>,
    distance: f32,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(Duration::from_millis(180))
            .with_easing(|t| 1. - (1. - t).powi(3))
            .with_max_fps(60.),
        move |v, t| v.relative().top(px(distance * (1. - t))).opacity(t),
    )
}
#[derive(gpui::IntoElement)]
pub struct HoverFill {
    pub id: ElementId,
    pub color: u32,
    pub radius: f32,
    pub pressed: Option<(gpui::SharedString, u32)>,
}
impl gpui::RenderOnce for HoverFill {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let hovered = *state.read(cx);
        gpui::div()
            .id(self.id)
            .absolute()
            .inset_0()
            .rounded(px(self.radius))
            .bg(rgb(self.color))
            .when_some(self.pressed, |v, (group, color)| {
                v.group_active(group, move |v| v.bg(rgb(color)).opacity(1.))
            })
            .on_hover(move |hovered, _, cx| {
                state.update(cx, |v, cx| {
                    if *v != *hovered {
                        *v = *hovered;
                        cx.notify();
                    }
                })
            })
            .with_spring(
                "hover-fade",
                spring(if hovered { 1. } else { 0. }),
                |v, t| v.opacity(t),
            )
    }
}

pub fn enter_instrumented<E: gpui::Element + gpui::Styled + 'static>(
    element: crate::automation::element::AutomationElement<E>,
    id: impl Into<ElementId>,
    distance: f32,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(Duration::from_millis(180))
            .with_easing(|t| 1. - (1. - t).powi(3))
            .with_max_fps(60.),
        move |v, t| v.map_inner(|v| v.relative().top(px(distance * (1. - t))).opacity(t)),
    )
}

pub fn mix_rgb(from: u32, to: u32, progress: f32) -> gpui::Rgba {
    let t = progress.clamp(0., 1.);
    let value = [16, 8, 0].into_iter().fold(0, |value, shift| {
        let a = ((from >> shift) & 255) as f32;
        let b = ((to >> shift) & 255) as f32;
        value | (((a + (b - a) * t).round() as u32) << shift)
    });
    rgb(value)
}

/// A group's moving surface, independent of row lifetimes and clipping.
#[derive(Default)]
pub(crate) struct SlidingSurface {
    active: Option<ElementId>,
    target: [f32; 4],
    position: Option<Slide<4>>,
    opacity: gpui::SpringState,
    last_frame: Option<MotionInstant>,
    target_clip: gpui::Bounds<gpui::Pixels>,
    paint_clip: gpui::Bounds<gpui::Pixels>,
}
#[cfg(not(target_family = "wasm"))]
use std::time::Instant as MotionInstant;
#[cfg(target_family = "wasm")]
use web_time::Instant as MotionInstant;

impl SlidingSurface {
    pub(crate) fn is_target(&self, id: &ElementId) -> bool {
        self.active.as_ref() == Some(id)
    }

    pub(crate) fn retarget(
        &mut self,
        id: &ElementId,
        bounds: gpui::Bounds<gpui::Pixels>,
        clip: gpui::Bounds<gpui::Pixels>,
    ) -> bool {
        let target = [
            bounds.origin.x.as_f32(),
            bounds.origin.y.as_f32(),
            bounds.size.width.as_f32(),
            bounds.size.height.as_f32(),
        ];
        if self.is_target(id) && self.target == target && self.target_clip == clip {
            return false;
        }
        if !self.position.as_ref().is_some_and(Slide::is_moving) {
            // Discard time spent at rest. Continuous retargeting while moving
            // keeps its frame clock, so scrubbing cannot freeze the animation.
            self.last_frame = Some(MotionInstant::now());
        }
        self.paint_clip = if self.position.is_some() {
            self.paint_clip.union(&clip)
        } else {
            clip
        };
        self.target_clip = clip;
        self.active = Some(id.clone());
        self.target = target;
        self.position.get_or_insert_with(|| Slide::new(target));
        true
    }

    pub(crate) fn clear(&mut self, id: &ElementId) -> bool {
        if !self.is_target(id) {
            return false;
        }
        self.active = None;
        if !self.position.as_ref().is_some_and(Slide::is_moving) {
            self.last_frame = Some(MotionInstant::now());
        }
        true
    }

    fn advance(&mut self, reduced: bool) -> bool {
        let now = MotionInstant::now();
        let dt = self
            .last_frame
            .replace(now)
            .map_or(0., |last| now.duration_since(last).as_secs_f32());
        self.advance_by(dt, reduced)
    }

    fn advance_by(&mut self, dt: f32, reduced: bool) -> bool {
        let target_opacity = if self.active.is_some() { 1. } else { 0. };
        let config = hover_spring_config();
        self.opacity = config.step(self.opacity, target_opacity, dt);
        if reduced || config.is_settled(self.opacity, target_opacity, 0.001) {
            self.opacity = gpui::SpringState {
                position: target_opacity,
                velocity: 0.,
            };
        }
        let mut moving = false;
        if let Some(position) = &mut self.position {
            position.retarget(self.target);
            moving = position.advance(dt, reduced);
        }
        // Travel across regions using their combined clip. At rest, respect
        // the target's scroll viewport so offscreen selections cannot bleed.
        if !moving {
            self.paint_clip = self.target_clip;
        }
        if self.active.is_none() && self.opacity.position == 0. {
            self.position = None;
            return false;
        }
        moving || self.opacity.position != target_opacity || self.opacity.velocity != 0.
    }

    pub(crate) fn paint(
        &mut self,
        color: u32,
        radius: f32,
        group_bounds: gpui::Bounds<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &gpui::App,
    ) -> bool {
        let moving = self.advance(cx.reduce_motion());
        if let Some([x, y, w, h]) = self.position.as_ref().map(Slide::position) {
            let rect = gpui::Bounds::new(gpui::point(px(x), px(y)), gpui::size(px(w), px(h)));
            window.with_content_mask(
                Some(gpui::ContentMask {
                    bounds: self.paint_clip.intersect(&group_bounds),
                }),
                |window| {
                    window.paint_quad(
                        gpui::fill(rect, rgb(color).opacity(self.opacity.position))
                            .corner_radii(px(radius)),
                    );
                },
            );
        }
        moving
    }
}

#[cfg(test)]
mod sliding_hover_tests {
    use super::*;

    fn visible() -> SlidingSurface {
        SlidingSurface {
            active: Some("first".into()),
            target: [0., 0., 240., 32.],
            position: Some(Slide::new([0., 0., 240., 32.])),
            opacity: gpui::SpringState {
                position: 1.,
                velocity: 0.,
            },
            last_frame: None,
            target_clip: Default::default(),
            paint_clip: Default::default(),
        }
    }

    #[test]
    fn far_targets_accelerate_but_every_distance_eases_to_rest() {
        let mut peaks = Vec::new();
        for distance in [34., 160., 634., 1200.] {
            let mut motion = Slide::new([0.]);
            motion.retarget([distance]);
            let mut velocities = Vec::new();
            let mut elapsed = 0.;
            loop {
                let moving = motion.advance(1. / 480., false);
                elapsed += 1. / 480.;
                velocities.push(motion.velocity[0]);
                assert!(motion.position[0] >= 0. && motion.position[0] <= distance);
                assert!(
                    elapsed < 0.30,
                    "{distance}px exceeded the long-jump settling bound"
                );
                if !moving {
                    break;
                }
            }
            assert!(velocities[1] > velocities[0], "missing acceleration");
            let peak = velocities.iter().copied().fold(0., f32::max);
            assert!(
                velocities[velocities.len() - 2] < peak / 10.,
                "missing braking"
            );
            assert_eq!(motion.position, [distance]);
            assert_eq!(motion.velocity, [0.]);
            peaks.push(peak);
        }
        assert!(peaks.windows(2).all(|pair| pair[1] > pair[0] * 1.5));
    }

    #[test]
    fn reversals_keep_the_painted_pose_and_velocity_without_queuing_old_goals() {
        let mut motion = Slide::new([0., 0.]);
        motion.retarget([600., 0.]);
        motion.advance(0.025, false);
        let before = (motion.position, motion.velocity);
        motion.retarget([0., 80.]);
        assert_eq!((motion.position, motion.velocity), before);
        motion.advance(0.016, false);
        let before = (motion.position, motion.velocity);
        motion.retarget([200., -20.]);
        assert_eq!((motion.position, motion.velocity), before);
        assert!(!motion.advance(1., false));
        assert_eq!(motion.position, [200., -20.]);
        assert_eq!(motion.velocity, [0., 0.]);
    }

    #[test]
    fn nearer_target_ahead_brakes_without_overshoot() {
        let mut motion = Slide::new([0.]);
        motion.retarget([600.]);
        motion.advance(0.02, false);
        let goal = motion.position[0] + 5.;
        let velocity = motion.velocity;
        motion.retarget([goal]);
        assert_eq!(motion.velocity, velocity);
        for _ in 0..60 {
            motion.advance(1. / 240., false);
            assert!(motion.position[0] <= goal + 0.001);
        }
        assert_eq!(motion.position, [goal]);
    }

    #[test]
    fn analytic_curve_is_independent_of_frame_interval() {
        let make = || {
            let mut m = Slide::new([0., 0., 240., 32.]);
            m.retarget([600., 120., 280., 80.]);
            m
        };
        let mut one = make();
        one.advance(0.1, false);
        let mut many = make();
        for _ in 0..12 {
            many.advance(1. / 120., false);
        }
        for i in 0..4 {
            assert!((one.position[i] - many.position[i]).abs() < 0.002);
            assert!((one.velocity[i] - many.velocity[i]).abs() < 0.02);
        }
    }

    #[test]
    fn continuous_targets_keep_moving_and_then_become_idle() {
        let mut motion = Slide::new([0.]);
        for step in 1..120 {
            motion.retarget([step as f32 * 2.]);
            motion.advance(1. / 120., false);
        }
        assert!(motion.position[0] > 220.);
        assert!(!motion.advance(1., false));
        assert!(!motion.advance(1., false));
        motion.retarget([900.]);
        assert!(!motion.advance(0., true));
        assert_eq!(motion.position, [900.]);
        assert_eq!(motion.velocity, [0.]);
    }

    #[test]
    fn leaving_and_reentering_preserve_fade_and_reduced_motion_settles() {
        let mut state = visible();
        state.active = None;
        assert!(state.advance_by(0.02, false));
        let opacity = state.opacity.position;
        assert!(opacity > 0. && opacity < 1.);
        state.active = Some("second".into());
        state.target[1] = 34.;
        state.advance_by(0., false);
        assert_eq!(state.opacity.position, opacity);
        assert_eq!(state.position.as_ref().unwrap().position()[1], 0.);
        assert!(!state.advance_by(0., true));
        assert_eq!(state.position.as_ref().unwrap().position()[1], 34.);
        assert_eq!(state.opacity.position, 1.);
        state.active = None;
        assert!(!state.advance_by(0., true));
        assert!(state.position.is_none());
    }
}
