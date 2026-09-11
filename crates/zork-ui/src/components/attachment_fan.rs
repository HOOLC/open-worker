//! Geometry shared by the conversation composer and delivered file previews.
//! Coordinates are logical pixels relative to the opening's horizontal centre
//! and the composer's top edge. The approved editor used a quarter-scale group.
use gpui::{point, Point};

pub type Curve = [Point<f32>; 4];
pub const SCALE: f32 = 0.25;
pub const MAX_FILES: usize = 16;
pub const REMOVE_SIZE: f32 = 28.;
pub const REMOVE_GLYPH: f32 = 8.;
const HOLE_WIDTH: f32 = 1.07;
const HOLE_HEIGHT: f32 = 0.82;
const HOLE_Y: f32 = 49.;
const DEPTH: f32 = 32.;
const GAP: f32 = 16.;

pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn phase(start: f32, end: f32, value: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn lerp(a: Point<f32>, b: Point<f32>, t: f32) -> Point<f32> {
    point(mix(a.x, b.x, t), mix(a.y, b.y, t))
}
pub fn sample(c: Curve, t: f32) -> Point<f32> {
    let u = 1. - t;
    c[0] * (u * u * u) + c[1] * (3. * u * u * t) + c[2] * (3. * u * t * t) + c[3] * (t * t * t)
}
fn split(c: Curve, t: f32) -> [Curve; 2] {
    let a = lerp(c[0], c[1], t);
    let b = lerp(c[1], c[2], t);
    let d = lerp(c[2], c[3], t);
    let e = lerp(a, b, t);
    let f = lerp(b, d, t);
    let m = lerp(e, f, t);
    [[c[0], a, e, m], [m, f, d, c[3]]]
}
fn line(a: Point<f32>, b: Point<f32>) -> Curve {
    [a, lerp(a, b, 1. / 3.), lerp(a, b, 2. / 3.), b]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub width: f32,
    pub bend: f32,
    pub straight: f32,
    pub presence: f32,
}
impl Shape {
    pub fn for_count(count: usize) -> Self {
        let width = match count {
            0 => 0.5,
            1 => (160. + 24.) / 353. * 1.18,
            2 => {
                let angle = 7.2_f32.to_radians();
                (96. + 160. * angle.cos() + 225. * angle.sin() + 24.) / 353. * 0.86
            }
            _ => (1. + count.saturating_sub(3) as f32 * 0.1).min(1.3),
        };
        Self {
            width,
            bend: if count <= 1 { 0. } else { width.min(1.4) },
            straight: if count <= 1 { 1. } else { 0. },
            presence: if count == 0 { 0. } else { 1. },
        }
    }
    pub fn interpolate(self, to: Self, t: f32) -> Self {
        Self {
            width: mix(self.width, to.width, t),
            bend: mix(self.bend, to.bend, t),
            straight: mix(self.straight, to.straight, t),
            presence: mix(self.presence, to.presence, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    pub outer: [Curve; 2],
    /// Clockwise, starting at the upper middle. Separate straight segments and
    /// circular end caps prevent long, pointed ends in the flattened state.
    pub hole: [Curve; 8],
}
impl Opening {
    pub fn new(shape: Shape, expanded: f32) -> Self {
        let top = [
            point(320., 193.738),
            point(406.525, 193.738),
            point(511.579, 220.679),
            point(494.5, 242.6),
        ];
        let bottom = [
            point(494.5, 242.6),
            point(484.965, 254.839),
            point(414.05, 216.7),
            point(320., 216.7),
        ];
        let mirror = |c: Curve| std::array::from_fn(|i| point(640. - c[3 - i].x, c[3 - i].y));
        let source = [top, bottom, mirror(bottom), mirror(top)];
        let mut max_x = 320_f32;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for c in [top, bottom] {
            for i in 0..=64 {
                let p = sample(c, i as f32 / 64.);
                max_x = max_x.max(p.x);
                min_y = min_y.min(p.y);
                max_y = max_y.max(p.y);
            }
        }
        let width = 2. * (max_x - 320.) * HOLE_WIDTH * shape.width;
        // The final height is independent of file count and curvature. A single
        // file starts flat, but must not have its height flattened a second time.
        let radius = ((max_y - min_y) * HOLE_HEIGHT * 0.25).max(4.) * 0.5;
        let half = width * 0.425;
        let cy = 220. + (242.6 - 220.) * HOLE_HEIGHT + HOLE_Y;
        let left = 320. - half;
        let right = 320. + half;
        let top_y = cy - radius;
        let bottom_y = cy + radius;
        let k = 0.5522848;
        let capsule = [
            line(point(320., top_y), point(right - radius, top_y)),
            [
                point(right - radius, top_y),
                point(right - radius + k * radius, top_y),
                point(right, cy - k * radius),
                point(right, cy),
            ],
            [
                point(right, cy),
                point(right, cy + k * radius),
                point(right - radius + k * radius, bottom_y),
                point(right - radius, bottom_y),
            ],
            line(point(right - radius, bottom_y), point(320., bottom_y)),
            line(point(320., bottom_y), point(left + radius, bottom_y)),
            [
                point(left + radius, bottom_y),
                point(left + radius - k * radius, bottom_y),
                point(left, cy + k * radius),
                point(left, cy),
            ],
            [
                point(left, cy),
                point(left, cy - k * radius),
                point(left + radius - k * radius, top_y),
                point(left + radius, top_y),
            ],
            line(point(left + radius, top_y), point(320., top_y)),
        ];
        let bend_lift = (242.6 - (193.738 + 216.7) * 0.5) * (1. - shape.bend);
        let sag = ((242.6_f32 - 193.738) * 0.2).clamp(0., 12.) * phase(0., 0.35, expanded);
        let segments: [Curve; 8] = std::array::from_fn(|segment| {
            let side = segment / 2;
            let curve = std::array::from_fn(|i| {
                let p = source[side][i];
                let middle = if side % 2 == 0 { i < 2 } else { i > 1 };
                let weight = (1. - (p.x - 320.).abs() / 174.5).max(0.);
                point(
                    320. + (p.x - 320.) * HOLE_WIDTH * shape.width,
                    220. + (p.y + if middle { bend_lift } else { 0. } + sag * weight - 220.)
                        * HOLE_HEIGHT
                        + HOLE_Y,
                )
            });
            split(curve, if side % 2 == 0 { 0.75 } else { 0.25 })[segment % 2]
        });
        let t = mix(phase(0.15, 1., expanded), 1., shape.straight);
        let hole = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                let p = lerp(segments[i][j], capsule[i][j], t);
                point(
                    (p.x - 320.) * SCALE * shape.presence,
                    (mix(cy, p.y, shape.presence) - 242.) * SCALE,
                )
            })
        });
        let remaining = (1. - phase(0.3, 1., expanded)) * (1. - shape.straight) * shape.presence;
        let outer_right = [
            point(320., 170.),
            point(520.5, 170.),
            point(495., 242.),
            point(608., 242.),
        ];
        let outer = [mirror(outer_right), outer_right].map(|c| {
            c.map(|p| {
                point(
                    (p.x - 320.) * shape.width * SCALE,
                    (p.y - 242.) * 0.18 * shape.bend * remaining * SCALE,
                )
            })
        });
        Self { outer, hole }
    }
    pub fn translated(self, offset: Point<f32>) -> Self {
        Self {
            outer: self.outer.map(|c| c.map(|p| p + offset)),
            hole: self.hole.map(|c| c.map(|p| p + offset)),
        }
    }
    pub fn lower_edge(&self) -> f32 {
        self.hole
            .iter()
            .flatten()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max)
    }
}

pub fn clip_below(opening: &Opening, _expanded: f32) -> f32 {
    opening.lower_edge() + 0.5
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub center: Point<f32>,
    pub width: f32,
    pub height: f32,
    pub angle: f32,
}
impl Pose {
    pub fn interpolate(self, to: Self, t: f32) -> Self {
        Self {
            center: lerp(self.center, to.center, t),
            width: mix(self.width, to.width, t),
            height: mix(self.height, to.height, t),
            angle: mix(self.angle, to.angle, t),
        }
    }
    pub fn hidden(self) -> Self {
        Self {
            center: point(
                0.,
                (220. + (242.6 - 220.) * HOLE_HEIGHT + HOLE_Y - 242.) * SCALE
                    + self.height * 0.5
                    + 8.,
            ),
            angle: 0.,
            ..self
        }
    }
    pub fn half_extent(self) -> Point<f32> {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        point(
            (self.width * cos.abs() + self.height * sin.abs()) * 0.5,
            (self.height * cos.abs() + self.width * sin.abs()) * 0.5,
        )
    }
}

/// One pose per file, preserving file identity when the three-page stack puts
/// the first file in its centre. Expanded pages land at the flattened aperture,
/// using the same insertion depth as the approved interactive reference.
pub fn poses(count: usize, expanded: f32, available: f32) -> Vec<Pose> {
    let count = count.min(MAX_FILES);
    if count == 0 {
        return vec![];
    }
    let columns = count.min(4);
    let rows = count.div_ceil(columns);
    let mut height: f32 = if rows > 1 { 170. } else { 220. };
    let max_width = (available / SCALE - 40.).clamp(180., 600.);
    height = height
        .min((max_width - GAP * columns.saturating_sub(1) as f32) / (columns as f32 * 160. / 225.));
    let lift = phase(0., 0.65, expanded);
    let spread = phase(0.1, 1., expanded);
    let width_factor = Shape::for_count(count).width;
    (0..count)
        .map(|index| {
            let slot = index;
            let t = if count == 1 {
                0.
            } else {
                slot as f32 / (count - 1) as f32 * 2. - 1.
            };
            let side = t.abs();
            let (width, closed_height) = if count > 2 && index != count / 2 {
                (136., 190.)
            } else {
                (160., 225.)
            };
            let angle = if count <= 2 { t * 7.2 } else { t * 12. };
            let closed_x = if count <= 2 {
                t * 48.
            } else {
                t * 110. * width_factor
            };
            let closed_y = if count <= 2 {
                let radians = angle.to_radians();
                263. + DEPTH - (closed_height * radians.cos() + width * radians.sin().abs()) * 0.5
            } else {
                150.5 + 9.5 * side + DEPTH
            };
            let row = slot / columns;
            let row_count = columns.min(count - row * columns);
            let open_width = height * 160. / 225.;
            let total_width =
                row_count as f32 * open_width + row_count.saturating_sub(1) as f32 * GAP;
            let open_x = -total_width * 0.5
                + open_width * 0.5
                + (slot % columns) as f32 * (open_width + GAP);
            let open_y = 150.5 + DEPTH - 36. - (rows - 1 - row) as f32 * (height + GAP);
            Pose {
                center: point(
                    mix(closed_x, open_x, spread) * SCALE,
                    (mix(closed_y, open_y, lift) + (HOLE_Y - 25.) * (1. - lift) - 242.) * SCALE,
                ),
                width: mix(width, open_width, spread) * SCALE,
                height: mix(closed_height, height, spread) * SCALE,
                angle: angle * (1. - spread),
            }
        })
        .collect()
}

pub fn dimensions(count: usize, available: f32) -> (f32, f32) {
    if count == 0 {
        return (0., 0.);
    }
    let closed = poses(count, 0., available);
    let open = poses(count, 1., available);
    let half = closed
        .iter()
        .chain(&open)
        .map(|p| p.center.x.abs() + p.half_extent().x)
        .fold(0., f32::max);
    let top = closed
        .iter()
        .chain(&open)
        .map(|p| -p.center.y + p.half_extent().y)
        .fold(0., f32::max);
    (
        (half * 2. + REMOVE_SIZE).min(available),
        top + REMOVE_SIZE * 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_order_is_left_to_right_then_top_to_bottom() {
        for count in 1..=MAX_FILES {
            let closed = poses(count, 0., 300.);
            assert!(closed.windows(2).all(|p| p[0].center.x < p[1].center.x));
            let open = poses(count, 1., 300.);
            assert!(open.windows(2).all(|p| {
                p[0].center.y < p[1].center.y
                    || (p[0].center.y == p[1].center.y && p[0].center.x < p[1].center.x)
            }));
        }
    }
    #[test]
    fn single_is_flat_with_the_same_height_as_other_flattened_openings() {
        let one = Opening::new(Shape::for_count(1), 0.);
        let three = Opening::new(Shape::for_count(3), 1.);
        assert!(one.outer.iter().flatten().all(|p| p.y == 0.));
        assert!(
            (one.hole[3][3].y - one.hole[0][0].y - (three.hole[3][3].y - three.hole[0][0].y)).abs()
                < 0.001
        );
        for opening in [one, three] {
            for i in 0..8 {
                let a = opening.hole[i];
                let b = opening.hole[(i + 1) % 8];
                assert_eq!(a[3], b[0]);
                let u = a[3] - a[2];
                let v = b[1] - b[0];
                assert!((u.x * v.y - u.y * v.x).abs() < 0.001);
            }
        }
    }
    #[test]
    fn count_width_and_bend_follow_occupied_pages() {
        let shapes = [1, 2, 3].map(Shape::for_count);
        assert!(shapes[0].width < shapes[1].width && shapes[1].width < shapes[2].width);
        assert!(shapes[0].bend < shapes[1].bend && shapes[1].bend < shapes[2].bend);
        let two = poses(2, 0., 300.);
        assert_eq!(two[0].center.x, -two[1].center.x);
        assert_eq!(two[0].angle, -two[1].angle);
        assert!(
            (two[0].center.y + two[0].half_extent().y - two[1].center.y - two[1].half_extent().y)
                .abs()
                < 0.001
        );
    }
    #[test]
    fn expanded_pages_are_upright_at_the_slot_and_do_not_overlap() {
        for count in 1..=MAX_FILES {
            let pages = poses(count, 1., 300.);
            let rim = Opening::new(Shape::for_count(count), 1.).lower_edge();
            assert!(pages
                .iter()
                .all(|p| p.angle == 0. && p.center.y + p.height * 0.5 <= rim));
            for (i, a) in pages.iter().enumerate() {
                for b in &pages[i + 1..] {
                    assert!(
                        (a.center.x - b.center.x).abs() >= (a.width + b.width) * 0.5
                            || (a.center.y - b.center.y).abs() >= (a.height + b.height) * 0.5
                    );
                }
            }
        }
    }
}
