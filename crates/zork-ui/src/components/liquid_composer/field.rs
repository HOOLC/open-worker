use super::{smooth_union, Bubble, DOCK_FUSION, MEMBER_FUSION, RADIUS, SURFACE_RADIUS};

pub(super) fn scalar(x: f32, y: f32, width: f32, bubbles: &[Bubble]) -> f32 {
    let corner_x = x.clamp(SURFACE_RADIUS, (width - SURFACE_RADIUS).max(SURFACE_RADIUS));
    let plate = if x >= SURFACE_RADIUS && x <= width - SURFACE_RADIUS {
        -y.min(SURFACE_RADIUS)
    } else {
        ((x - corner_x).powi(2) + (y - y.max(SURFACE_RADIUS)).powi(2)).sqrt() - SURFACE_RADIUS
    };
    let mut actors = f32::INFINITY;
    for b in bubbles {
        let start = b.x + RADIUS;
        let center = x.clamp(start, (b.x + b.width - RADIUS).max(start));
        let d = ((x - center).powi(2) + (y + b.lift).powi(2)).sqrt() - RADIUS;
        actors = smooth_union(actors, d, MEMBER_FUSION);
    }
    smooth_union(plate, actors, DOCK_FUSION)
}

/// Evaluate four neighbouring sample points together on Apple Silicon/ARM64.
/// No approximate reciprocal/square-root or fused operations change the field.
#[cfg(target_arch = "aarch64")]
pub(super) fn four(xs: [f32; 4], ys: [f32; 4], width: f32, bubbles: &[Bubble]) -> [f32; 4] {
    // Advanced SIMD is part of the ARM64 targets supported by the application.
    unsafe { neon(xs, ys, width, bubbles) }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn neon(xs: [f32; 4], ys: [f32; 4], width: f32, bubbles: &[Bubble]) -> [f32; 4] {
    use std::arch::aarch64::*;
    let x = vld1q_f32(xs.as_ptr());
    let y = vld1q_f32(ys.as_ptr());
    let zero = vdupq_n_f32(0.);
    let r = vdupq_n_f32(SURFACE_RADIUS);
    let cx = vmaxq_f32(
        r,
        vminq_f32(x, vdupq_n_f32((width - SURFACE_RADIUS).max(SURFACE_RADIUS))),
    );
    let dx = vsubq_f32(x, cx);
    let dy = vsubq_f32(y, vmaxq_f32(y, r));
    let corner = vsubq_f32(
        vsqrtq_f32(vaddq_f32(vmulq_f32(dx, dx), vmulq_f32(dy, dy))),
        r,
    );
    let flat = vnegq_f32(vminq_f32(y, r));
    let inside = vandq_u32(
        vcgeq_f32(x, r),
        vcleq_f32(x, vdupq_n_f32(width - SURFACE_RADIUS)),
    );
    let plate = vbslq_f32(inside, flat, corner);
    let union = |a, b, k: f32| {
        let k4 = vdupq_n_f32(k);
        let h = vmaxq_f32(
            zero,
            vdivq_f32(vsubq_f32(k4, vabsq_f32(vsubq_f32(a, b))), k4),
        );
        vsubq_f32(
            vminq_f32(a, b),
            vmulq_f32(vmulq_f32(vmulq_f32(h, h), k4), vdupq_n_f32(0.25)),
        )
    };
    let mut actors = vdupq_n_f32(f32::INFINITY);
    for b in bubbles {
        let start = b.x + RADIUS;
        let center = vmaxq_f32(
            vdupq_n_f32(start),
            vminq_f32(x, vdupq_n_f32((b.x + b.width - RADIUS).max(start))),
        );
        let dx = vsubq_f32(x, center);
        let dy = vaddq_f32(y, vdupq_n_f32(b.lift));
        let d = vsubq_f32(
            vsqrtq_f32(vaddq_f32(vmulq_f32(dx, dx), vmulq_f32(dy, dy))),
            vdupq_n_f32(RADIUS),
        );
        actors = union(actors, d, MEMBER_FUSION);
    }
    let result = union(plate, actors, DOCK_FUSION);
    let mut out = [0.; 4];
    vst1q_f32(out.as_mut_ptr(), result);
    out
}

#[cfg(all(test, target_arch = "aarch64"))]
mod tests {
    use super::*;
    #[test]
    fn simd_matches_scalar_at_contacts_and_separation() {
        for frame in 0..81 {
            let t = frame as f32 / 80.;
            let bubbles = [
                Bubble {
                    x: 16.,
                    lift: -8. + 29. * t,
                    width: 36. + 140. * t,
                },
                Bubble {
                    x: 32. - 16. * t,
                    lift: -8. + 68. * t,
                    width: 36. + 100. * t,
                },
                Bubble {
                    x: 48. - 32. * t,
                    lift: -8. + 107. * t,
                    width: 36. + 180. * t,
                },
            ];
            for y in -130..40 {
                for x in (0..250).step_by(4) {
                    let xs = [x as f32, x as f32 + 1., x as f32, x as f32 + 1.];
                    let ys = [y as f32, y as f32, y as f32 + 1., y as f32 + 1.];
                    let values = four(xs, ys, 600., &bubbles);
                    for lane in 0..4 {
                        let expected = scalar(xs[lane], ys[lane], 600., &bubbles);
                        assert!(
                            (values[lane] - expected).abs() < 0.0001,
                            "frame {frame}, ({},{}): {} vs {expected}",
                            xs[lane],
                            ys[lane],
                            values[lane]
                        );
                    }
                }
            }
        }
    }
}
