//! Draft membership changes retain departing previews until they are below the
//! rim. A new member waits for the aperture before emerging, including 0 → 1.
use super::fan::FanState;
use std::time::Instant;
use zork_client_core::files::FileRef;
use zork_ui::components::attachment_fan::{self as geometry, phase, Pose, Shape};

#[derive(Clone)]
pub(in crate::views) struct VisualFile {
    pub file: FileRef,
    pub pose: Pose,
    pub visible: bool,
    pub departing: bool,
}
#[derive(Clone)]
pub(in crate::views) struct Frame {
    pub files: Vec<VisualFile>,
    pub shape: Shape,
    pub expanded: f32,
    pub width: f32,
    pub height: f32,
    pub changing_count: bool,
}
impl Frame {
    pub fn settled(files: &[FileRef], expanded: f32, available: f32) -> Self {
        let (width, height) = geometry::dimensions(files.len(), available);
        Self {
            files: files
                .iter()
                .zip(geometry::poses(files.len(), expanded, available))
                .map(|(file, pose)| VisualFile {
                    file: file.clone(),
                    pose,
                    visible: true,
                    departing: false,
                })
                .collect(),
            shape: Shape::for_count(files.len()),
            expanded,
            width,
            height,
            changing_count: false,
        }
    }
}
struct CountMotion {
    from: Frame,
    began: Instant,
    adding: bool,
    duration: f32,
}
#[derive(Default)]
pub(in crate::views) struct DraftFiles {
    current: Vec<FileRef>,
    motion: Option<CountMotion>,
    frame: Option<Frame>,
}
impl DraftFiles {
    pub fn reset(&mut self, files: &[FileRef]) {
        self.current = files.to_vec();
        self.motion = None;
        self.frame = None;
    }
    pub fn changing(&self) -> bool {
        self.motion.is_some()
    }
    pub fn frame(&self, expanded: f32, available: f32) -> Frame {
        self.frame
            .clone()
            .unwrap_or_else(|| Frame::settled(&self.current, expanded, available))
    }
    pub fn advance(
        &mut self,
        desired: &[FileRef],
        fan: &mut FanState,
        now: Instant,
        reduced: bool,
        available: f32,
    ) -> bool {
        if reduced {
            if self.motion.is_some() || self.current != desired {
                self.current = desired.to_vec();
                if self.current.is_empty() {
                    *fan = FanState::default();
                }
            }
            self.motion = None;
            self.frame = Some(Frame::settled(&self.current, fan.progress, available));
            return false;
        }
        // A newer core snapshot retargets from the last displayed frame.
        // Never finish an obsolete membership sequence before responding.
        if self.current != desired {
            let from = self.frame(fan.progress, available);
            let adding = desired.len() > self.current.len();
            self.current = desired.to_vec();
            let target = Frame::settled(&self.current, if fan.open() { 1. } else { 0. }, available);
            let duration = count_duration(&from, &target, adding);
            self.motion = Some(CountMotion {
                from,
                began: now,
                adding,
                duration,
            });
        }
        let Some(motion) = &self.motion else {
            self.frame = Some(Frame::settled(&self.current, fan.progress, available));
            return false;
        };
        let t = (now.duration_since(motion.began).as_secs_f32() / motion.duration).min(1.);
        let shape_t = if motion.adding {
            phase(0.18, 0.45, t)
        } else {
            phase(0.7, 1., t)
        };
        // Membership and hover are independent inputs. Deleting an item
        // must not reset hover/pinning or force an expanded fan closed.
        let expanded = fan.progress;
        let mut frame = Frame::settled(&self.current, expanded, available);
        frame.shape = motion.from.shape.interpolate(frame.shape, shape_t);
        // The fan is right-anchored. Reserving the target width immediately
        // shifts every existing paper before the animation has started.
        // Grow with the aperture before entry, and shrink only after retreat.
        frame.width = geometry::mix(motion.from.width, frame.width, shape_t);
        frame.height = geometry::mix(motion.from.height, frame.height, shape_t);
        frame.changing_count = true;
        for visual in &mut frame.files {
            if let Some(old) = motion
                .from
                .files
                .iter()
                .find(|v| v.file.id == visual.file.id)
            {
                visual.pose = old.pose.interpolate(visual.pose, phase(0.18, 1., t));
                visual.visible = old.visible || t >= 0.45;
            } else {
                visual.pose = visual
                    .pose
                    .hidden()
                    .interpolate(visual.pose, phase(0.45, 1., t));
                visual.visible = t >= 0.45;
            }
        }
        for old in &motion.from.files {
            if self.current.iter().any(|file| file.id == old.file.id) {
                continue;
            }
            frame.files.push(VisualFile {
                file: old.file.clone(),
                pose: old.pose.interpolate(old.pose.hidden(), phase(0.18, 0.7, t)),
                visible: old.visible && t < 0.7,
                departing: true,
            });
        }
        // Departing papers retain their old stacking order until hidden.
        frame.files.sort_by_key(|visual| {
            motion
                .from
                .files
                .iter()
                .position(|old| old.file.id == visual.file.id)
                .unwrap_or(motion.from.files.len())
        });
        self.frame = Some(frame);
        if t < 1. {
            return true;
        }
        self.motion = None;
        if self.current.is_empty() {
            *fan = FanState::default();
        }
        self.frame = Some(Frame::settled(&self.current, fan.progress, available));
        self.current != desired
    }
}

// Keep a consistent spatial pace. Smoothstep shapes acceleration within each
// phase; the total time grows with the path instead of forcing every count
// change into a fixed duration.
fn count_duration(from: &Frame, to: &Frame, adding: bool) -> f32 {
    const SPEED: f32 = 200.; // logical pixels per second
    let shape_span = if adding { 0.27 } else { 0.3 };
    let mut distance = ((to.width - from.width).abs() * 0.5 / shape_span)
        .max((to.height - from.height).abs() * 0.5 / shape_span);
    let travel = |a: Pose, b: Pose| {
        (a.center.x - b.center.x).hypot(a.center.y - b.center.y)
            + (a.width - b.width).abs() * 0.5
            + (a.height - b.height).abs() * 0.5
    };
    for next in &to.files {
        if let Some(old) = from.files.iter().find(|old| old.file.id == next.file.id) {
            distance = distance.max(travel(old.pose, next.pose) / 0.82);
        } else {
            distance = distance.max(travel(next.pose.hidden(), next.pose) / 0.55);
        }
    }
    for old in &from.files {
        if !to.files.iter().any(|next| next.file.id == old.file.id) {
            distance = distance.max(travel(old.pose, old.pose.hidden()) / 0.52);
        }
    }
    (distance / SPEED).max(1. / 60.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn file(id: &str) -> FileRef {
        FileRef {
            id: id.into(),
            name: id.into(),
            byte_len: 0,
            content_root: String::new(),
        }
    }
    #[test]
    fn first_file_waits_for_opening_and_last_file_returns_before_it_closes() {
        let mut state = DraftFiles::default();
        let mut fan = FanState::default();
        let now = Instant::now();
        let files = [file("one")];
        assert!(state.advance(&files, &mut fan, now, false, 300.));
        assert_eq!(state.frame(0., 300.).shape.presence, 0.);
        let adding = state.motion.as_ref().unwrap().duration;
        state.advance(
            &files,
            &mut fan,
            now + Duration::from_secs_f32(adding * 0.3),
            false,
            300.,
        );
        let frame = state.frame(0., 300.);
        assert!(frame.shape.presence > 0. && frame.shape.presence < 1.);
        assert!(!frame.files[0].visible);
        state.advance(
            &files,
            &mut fan,
            now + Duration::from_secs_f32(adding * 0.5),
            false,
            300.,
        );
        let frame = state.frame(0., 300.);
        assert_eq!(frame.shape.presence, 1.);
        assert!(frame.files[0].visible);
        assert!(!state.advance(
            &files,
            &mut fan,
            now + Duration::from_secs_f32(adding + 0.001),
            false,
            300.
        ));
        let end = now + Duration::from_secs_f32(adding + 0.001);
        state.advance(&[], &mut fan, end, false, 300.);
        let removing = state.motion.as_ref().unwrap().duration;
        state.advance(
            &[],
            &mut fan,
            end + Duration::from_secs_f32(removing * 0.5),
            false,
            300.,
        );
        let frame = state.frame(0., 300.);
        assert_eq!(frame.shape.presence, 1.);
        assert!(frame.files[0].departing);
        state.advance(
            &[],
            &mut fan,
            end + Duration::from_secs_f32(removing * 0.8),
            false,
            300.,
        );
        let frame = state.frame(0., 300.);
        assert!(!frame.files[0].visible && frame.shape.presence < 1.);
        assert!(!state.advance(
            &[],
            &mut fan,
            end + Duration::from_secs_f32(removing + 0.001),
            false,
            300.
        ));
        assert!(state.frame(0., 300.).files.is_empty());
    }
    #[test]
    fn removing_a_file_preserves_hover_and_expansion() {
        let now = Instant::now();
        let mut state = DraftFiles::default();
        let mut fan = FanState::default();
        fan.hovered = true;
        fan.progress = 1.;
        let before = [file("a"), file("b")];
        state.reset(&before);
        state.advance(&before, &mut fan, now, false, 300.);
        state.advance(&before[..1], &mut fan, now, false, 300.);
        assert!(fan.hovered && fan.open());
        assert_eq!(state.frame(1., 300.).expanded, 1.);
        let duration = state.motion.as_ref().unwrap().duration;
        state.advance(
            &before[..1],
            &mut fan,
            now + Duration::from_secs_f32(duration + 0.001),
            false,
            300.,
        );
        assert!(fan.hovered && fan.open());
        assert_eq!(fan.progress, 1.);
        assert_eq!(state.frame(1., 300.).files.len(), 1);
    }
    #[test]
    fn twice_the_travel_uses_twice_the_time() {
        let from = Frame::settled(&[file("a")], 0., 300.);
        let mut near = from.clone();
        near.files[0].pose.center.x += 20.;
        let mut far = from.clone();
        far.files[0].pose.center.x += 40.;
        assert!(
            (count_duration(&from, &far, false) - 2. * count_duration(&from, &near, false)).abs()
                < 0.0001
        );
    }
    #[test]
    fn membership_starts_at_the_previous_geometry_and_layer_order() {
        let now = Instant::now();
        for (before, after) in [
            (vec![file("a")], vec![file("a"), file("b")]),
            (
                vec![file("a"), file("b"), file("c")],
                vec![file("a"), file("c")],
            ),
        ] {
            let mut state = DraftFiles::default();
            let mut fan = FanState::default();
            state.reset(&before);
            state.advance(&before, &mut fan, now, false, 300.);
            let old = state.frame(0., 300.);
            state.advance(&after, &mut fan, now, false, 300.);
            let first = state.frame(0., 300.);
            assert_eq!(
                (first.width, first.height, first.shape),
                (old.width, old.height, old.shape)
            );
            let visible: Vec<_> = first.files.iter().filter(|v| v.visible).collect();
            assert_eq!(visible.len(), old.files.len());
            for (a, b) in visible.iter().zip(&old.files) {
                assert_eq!(a.file.id, b.file.id);
                assert_eq!(a.pose, b.pose);
            }
        }
    }
    #[test]
    fn rapid_changes_converge_to_latest_draft_and_reduced_motion_settles() {
        let now = Instant::now();
        let mut state = DraftFiles::default();
        let mut fan = FanState::default();
        state.advance(&[file("one")], &mut fan, now, false, 300.);
        let target = [file("two"), file("three")];
        state.advance(
            &target,
            &mut fan,
            now + Duration::from_millis(400),
            false,
            300.,
        );
        assert_eq!(state.current, target);
        assert_eq!(
            state.motion.as_ref().unwrap().began,
            now + Duration::from_millis(400)
        );
        assert!(state.advance(
            &target,
            &mut fan,
            now + Duration::from_millis(800),
            false,
            300.
        ));
        state.advance(
            &target,
            &mut fan,
            now + Duration::from_millis(810),
            false,
            300.,
        );
        assert!(!state.advance(
            &target,
            &mut fan,
            now + Duration::from_millis(1610),
            false,
            300.
        ));
        assert_eq!(
            state
                .frame(0., 300.)
                .files
                .iter()
                .map(|v| v.file.id.clone())
                .collect::<Vec<_>>(),
            vec!["two", "three"]
        );
        assert!(!state.advance(&[], &mut fan, now + Duration::from_millis(1700), true, 300.));
        assert_eq!(state.frame(0., 300.).shape.presence, 0.);
    }
}
