use crate::{gpu, metal};
pub enum Engine {
    Wgpu(gpu::Gpu),
    Metal(metal::Gpu),
}
impl Engine {
    pub fn new(force_f32: bool) -> Self {
        let backend = std::env::var("LEXER_BACKEND").unwrap_or_else(|_| "wgpu".into());
        if backend == "tint" || backend == "optimized" {
            Self::Metal(metal::Gpu::new(force_f32, backend == "optimized"))
        } else if backend == "wgpu" {
            Self::Wgpu(gpu::Gpu::new(force_f32))
        } else {
            panic!("unknown GPU backend: {backend}")
        }
    }
    pub fn adapter(&self) -> &str {
        match self {
            Self::Wgpu(g) => &g.adapter,
            Self::Metal(g) => &g.adapter,
        }
    }
    pub fn f16(&self) -> bool {
        match self {
            Self::Wgpu(g) => g.f16,
            Self::Metal(g) => g.f16,
        }
    }
    pub fn set_profile(&mut self, value: bool) {
        match self {
            Self::Wgpu(g) => g.profile = value,
            Self::Metal(g) => g.profile = value,
        }
    }
    pub fn labels(&mut self, features: &[u32]) -> Vec<u8> {
        match self {
            Self::Wgpu(g) => g.labels(features),
            Self::Metal(g) => g.labels(features),
        }
    }
    pub fn highlight(&mut self, code: &str) -> Vec<(usize, usize, u32)> {
        match self {
            Self::Wgpu(g) => g.highlight(code),
            Self::Metal(g) => g.highlight(code),
        }
    }
}
