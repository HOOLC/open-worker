//! Metal evaluates the field; GPUI retains compositing and all interactive UI.
use super::*;
use std::{ffi::c_void, sync::Arc};
#[repr(C)]
struct Parameters {
    width: f32,
    height: f32,
    scale: f32,
    plate_y: f32,
    plate_height: f32,
    radius: f32,
    member_fusion: f32,
    dock_fusion: f32,
    surface: [f32; 4],
    lower: [f32; 4],
    count: u32,
    pixel_width: u32,
    pixel_height: u32,
    surface_radius: f32,
}
unsafe extern "C" {
    fn zork_liquid_prepare(bytes: *const u8, length: usize) -> i32;
    fn zork_liquid_create() -> *mut c_void;
    fn zork_liquid_render(
        handle: *mut c_void,
        parameters: *const Parameters,
        bubbles: *const [f32; 4],
        gpu_ms: *mut f64,
    ) -> *const u8;
    fn zork_liquid_destroy(handle: *mut c_void);
}
pub(super) fn prepare() {
    if std::env::var_os("ZORK_LIQUID_GPU").is_none() {
        return;
    }
    let library = include_bytes!(concat!(env!("OUT_DIR"), "/liquid-field.metallib"));
    unsafe {
        zork_liquid_prepare(library.as_ptr(), library.len());
    }
}
#[derive(Default)]
pub(super) struct State {
    handle: Option<*mut c_void>,
    key: Option<(Bounds<Pixels>, f32, f32, Vec<Bubble>)>,
    image: Option<Arc<RenderImage>>,
}
impl State {
    pub(super) fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        height: f32,
        bubbles: &[Bubble],
        window: &mut Window,
    ) -> bool {
        if std::env::var_os("ZORK_LIQUID_GPU").is_none()
            || std::env::var_os("ZORK_LIQUID_CPU").is_some()
        {
            return false;
        }
        let handle = *self.handle.get_or_insert_with(|| {
            prepare();
            unsafe { zork_liquid_create() }
        });
        if handle.is_null() {
            return false;
        }
        let scale = window.scale_factor();
        let image_bounds = Bounds::new(
            bounds.origin - point(px(2.), px(2.)),
            bounds.size + size(px(4.), px(4.)),
        );
        let key = (bounds, height, scale, bubbles.to_vec());
        if self.key.as_ref() != Some(&key) {
            let pixel_width = (image_bounds.size.width.as_f32() * scale).ceil() as u32;
            let pixel_height = (image_bounds.size.height.as_f32() * scale).ceil() as u32;
            if pixel_width == 0 || pixel_height == 0 {
                return false;
            }
            let color = |value: u32| {
                [
                    ((value >> 16) & 255) as f32 / 255.,
                    ((value >> 8) & 255) as f32 / 255.,
                    (value & 255) as f32 / 255.,
                    1.,
                ]
            };
            let parameters = Parameters {
                width: bounds.size.width.as_f32(),
                height: bounds.size.height.as_f32(),
                scale,
                plate_y: bounds.size.height.as_f32() - height,
                plate_height: height,
                radius: RADIUS,
                member_fusion: MEMBER_FUSION,
                dock_fusion: DOCK_FUSION,
                surface: color(SURFACE_COLOR),
                lower: color(LOWER_COLOR),
                count: bubbles.len() as u32,
                pixel_width,
                pixel_height,
                surface_radius: SURFACE_RADIUS,
            };
            let members: Vec<_> = bubbles.iter().map(|b| [b.x, b.lift, b.width, 0.]).collect();
            let mut gpu_ms = 0.;
            let pixels =
                unsafe { zork_liquid_render(handle, &parameters, members.as_ptr(), &mut gpu_ms) };
            if pixels.is_null() {
                return false;
            }
            let data = unsafe {
                std::slice::from_raw_parts(pixels, pixel_width as usize * pixel_height as usize * 4)
            }
            .to_vec();
            let Some(buffer) = image::RgbaImage::from_raw(pixel_width, pixel_height, data) else {
                return false;
            };
            if let Some(previous) = self.image.take() {
                let _ = window.drop_image(previous);
            }
            self.image = Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])));
            self.key = Some(key);
            #[cfg(feature = "headless-bench")]
            STATS.with(|stats| {
                let mut stats = stats.borrow_mut();
                stats.0 += 1;
                stats.1 = stats.1.max(gpu_ms);
            });
        }
        self.image.as_ref().is_some_and(|image| {
            window
                .paint_image(
                    image_bounds,
                    image_bounds,
                    Corners::default(),
                    image.clone(),
                    0,
                    false,
                )
                .is_ok()
        })
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            unsafe {
                zork_liquid_destroy(handle);
            }
        }
    }
}
#[cfg(feature = "headless-bench")]
thread_local! {static STATS: RefCell<(u64,f64)>=const {RefCell::new((0,0.))};}
#[cfg(feature = "headless-bench")]
pub(super) fn stats() -> (u64, f64) {
    STATS.with(|stats| *stats.borrow())
}
