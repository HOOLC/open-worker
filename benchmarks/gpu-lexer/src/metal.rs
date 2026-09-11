//! Rust driver for the exact MSL emitted by the original Chrome/Dawn backend.
//! All buffers use coherent shared memory; completion is event-driven.
use crate::tokenizer;
use objc2::{
    rc::{Retained, autoreleasepool},
    runtime::ProtocolObject,
};
use objc2_foundation::{NSRange, NSString};
use objc2_metal::*;
use std::{
    ptr::NonNull,
    time::{Duration, Instant},
};

type Object<T> = Retained<ProtocolObject<T>>;
const SOURCES: [&str; 7] = [
    include_str!("../assets/dawn/a.metal"),
    include_str!("../assets/dawn/b.metal"),
    include_str!("../assets/dawn/c.metal"),
    include_str!("../assets/dawn/d.metal"),
    include_str!("../assets/dawn/e.metal"),
    include_str!("../assets/dawn/f.metal"),
    include_str!("../assets/dawn/g.metal"),
];
const BINDINGS: [&[usize]; 7] = [
    &[0, 3, 5, 9],
    &[3, 4, 9],
    &[0, 3, 4, 99, 5, 10, 8, 9],
    &[3, 4, 8],
    &[3, 4, 99, 6, 7, 5, 10, 8],
    &[3, 4, 99, 7, 8],
    &[0, 1, 3, 4, 99, 6, 5, 8],
];
const THREADS: [usize; 7] = [64, 64, 32, 256, 32, 256, 64];
const SHARED_BYTES: [usize; 7] = [0, 0, 16384, 16384, 16384, 0, 11168];

#[derive(Debug)]
enum WaitMode {
    Callback,
    Direct,
    Event,
}
pub struct Gpu {
    device: Object<dyn MTLDevice>,
    queue: Object<dyn MTLCommandQueue>,
    pipelines: Vec<Object<dyn MTLComputePipelineState>>,
    weights: Object<dyn MTLBuffer>,
    buffers: Vec<Object<dyn MTLBuffer>>,
    count: usize,
    chunks: u32,
    private_scratch: bool,
    binding_sizes: [usize; 11],
    event: Object<dyn MTLSharedEvent>,
    event_value: u64,
    wait_mode: WaitMode,
    pub adapter: String,
    pub f16: bool,
    pub profile: bool,
}
impl Gpu {
    pub fn new(force_f32: bool, optimized: bool) -> Self {
        assert!(!force_f32, "captured upstream Metal shader uses f16");
        autoreleasepool(|_| {
            let device = MTLCreateSystemDefaultDevice().expect("Metal device");
            let queue = device.newCommandQueue().expect("Metal queue");
            let options = MTLCompileOptions::new();
            options.setLanguageVersion(MTLLanguageVersion::Version3_2);
            let pipelines = SOURCES
                .iter()
                .enumerate()
                .map(|(i, source)| {
                    let library = device
                        .newLibraryWithSource_options_error(
                            &NSString::from_str(source),
                            Some(&options),
                        )
                        .expect("upstream MSL compilation");
                    let function = library
                        .newFunctionWithName(&NSString::from_str(&format!(
                            "gpu_lexer_{}",
                            (b'a' + i as u8) as char
                        )))
                        .unwrap();
                    device
                        .newComputePipelineStateWithFunction_error(&function)
                        .expect("Metal compute pipeline")
                })
                .collect();
            let data = include_bytes!("../assets/weights.f32");
            // SAFETY: Metal copies the complete immutable byte slice during this call.
            let weights = unsafe {
                device.newBufferWithBytes_length_options(
                    NonNull::new(data.as_ptr().cast_mut().cast()).unwrap(),
                    data.len(),
                    MTLResourceOptions::StorageModeShared,
                )
            }
            .unwrap();
            let private_mode = std::env::var("LEXER_METAL_PRIVATE")
                .ok()
                .or_else(|| optimized.then(|| "all".to_owned()));
            let private_scratch = matches!(private_mode.as_deref(), Some("all" | "1"));
            let weights = if private_mode.is_some() {
                let destination = device
                    .newBufferWithLength_options(data.len(), MTLResourceOptions::StorageModePrivate)
                    .unwrap();
                let command = queue.commandBuffer().unwrap();
                let blit = command.blitCommandEncoder().unwrap();
                // SAFETY: both retained buffers have exactly data.len() bytes,
                // offsets are zero, and completion precedes dropping the staging buffer.
                unsafe {
                    blit.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                        &weights,
                        0,
                        &destination,
                        0,
                        data.len(),
                    );
                }
                blit.endEncoding();
                command.commit();
                command.waitUntilCompleted();
                assert!(command.error().is_none());
                destination
            } else {
                weights
            };
            let event = device.newSharedEvent().expect("Metal shared event");
            let wait_mode = match std::env::var("LEXER_METAL_WAIT").as_deref() {
                Ok("event") => WaitMode::Event,
                Ok("direct") => WaitMode::Direct,
                Ok("callback") => WaitMode::Callback,
                Err(_) if optimized => WaitMode::Event,
                Err(_) => WaitMode::Callback,
                Ok(other) => panic!("unknown Metal wait mode: {other}"),
            };
            let adapter = format!(
                "{} / Metal, original Dawn-generated MSL, private_scratch={private_scratch}, wait={wait_mode:?}",
                device.name()
            );
            Self {
                device,
                queue,
                pipelines,
                weights,
                buffers: Vec::new(),
                count: 0,
                chunks: 0,
                adapter,
                f16: true,
                profile: false,
                private_scratch,
                binding_sizes: [0; 11],
                event,
                event_value: 0,
                wait_mode,
            }
        })
    }
    fn write(&self, index: usize, data: &[u8]) {
        let buffer = &self.buffers[index];
        assert!(data.len() <= buffer.length());
        // SAFETY: buffers are owned exclusively by this synchronous driver. The
        // prior call completed before this write; the destination fits and is shared.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                buffer.contents().as_ptr().cast(),
                data.len(),
            )
        };
    }
    fn prepare(&mut self, count: usize) {
        if self.count == count {
            return;
        }
        assert!(
            count > 0
                && count <= self.device.maxBufferLength() / 128
                && count <= u32::MAX as usize / 128
        );
        self.count = count;
        self.chunks = (count as u32).div_ceil(32);
        let leaves = self.chunks.next_power_of_two();
        let nodes = leaves * 2 - 1;
        let sizes = [
            count * 8,
            count.div_ceil(4) * 4,
            4,
            16,
            24,
            self.chunks as usize * 16,
            count * 32 * 2,
            nodes as usize * 32 * 2,
            (self.chunks as usize * 128).max(nodes as usize * 32) * 4,
            self.chunks as usize * 8,
            count * 32 * 4,
        ];
        let grow_private = self.private_scratch
            && sizes.iter().enumerate().any(|(index, &size)| {
                index >= 6
                    && self
                        .buffers
                        .get(index)
                        .is_none_or(|buffer| buffer.length() < size)
            });
        let initialization = grow_private.then(|| self.queue.commandBuffer().unwrap());
        let blit = initialization
            .as_ref()
            .map(|c| c.blitCommandEncoder().unwrap());
        for (index, size) in sizes.into_iter().enumerate() {
            // Like upstream: grow geometrically, retain capacity and the largest
            // binding extent when a later source gets smaller. Active counts still
            // come from metadata; stale tail contents never influence valid labels.
            self.binding_sizes[index] = self.binding_sizes[index].max(size);
            if self
                .buffers
                .get(index)
                .is_some_and(|buffer| buffer.length() >= size)
            {
                continue;
            }
            let capacity = size
                .max(256)
                .next_power_of_two()
                .min(self.device.maxBufferLength());
            assert!(capacity >= size);
            let private = self.private_scratch && index >= 6;
            let buffer = self
                .device
                .newBufferWithLength_options(
                    capacity,
                    if private {
                        MTLResourceOptions::StorageModePrivate
                    } else {
                        MTLResourceOptions::StorageModeShared
                    },
                )
                .unwrap();
            if private {
                blit.as_ref().unwrap().fillBuffer_range_value(
                    &buffer,
                    NSRange {
                        location: 0,
                        length: capacity,
                    },
                    0,
                );
            } else {
                // SAFETY: new, owned shared allocation, before any GPU use.
                unsafe {
                    buffer
                        .contents()
                        .as_ptr()
                        .cast::<u8>()
                        .write_bytes(0, capacity)
                };
            }
            if index == self.buffers.len() {
                self.buffers.push(buffer)
            } else {
                self.buffers[index] = buffer
            }
        }
        if let Some(command) = initialization {
            blit.unwrap().endEncoding();
            command.commit();
            // The next compute command is on the same serial queue and retains
            // these buffers. Its completion also covers this initialization.
        }
        self.write(
            3,
            bytemuck::cast_slice(&[1u32, count as u32, self.chunks, nodes]),
        );
        self.write(
            4,
            bytemuck::cast_slice(&[0u32, count as u32, 0, self.chunks, 0, leaves]),
        );
        let chunks: Vec<u32> = (0..self.chunks)
            .flat_map(|c| [c * 32, (count as u32 - c * 32).min(32), 0, c])
            .collect();
        self.write(5, bytemuck::cast_slice(&chunks));
    }
    pub fn labels(&mut self, features: &[u32]) -> Vec<u8> {
        autoreleasepool(|_| self.labels_inner(features))
    }
    fn labels_inner(&mut self, features: &[u32]) -> Vec<u8> {
        assert_eq!(features.len() % 2, 0);
        let count = features.len() / 2;
        if count == 0 {
            return Vec::new();
        }
        let start = Instant::now();
        self.prepare(count);
        let prepared = Instant::now();
        self.write(0, bytemuck::cast_slice(features));
        // SAFETY: previous GPU submission is complete; output is a shared buffer
        // owned by this driver. Match the original clear before atomic label packing.
        unsafe {
            self.buffers[1]
                .contents()
                .as_ptr()
                .cast::<u8>()
                .write_bytes(0, count.div_ceil(4) * 4)
        };
        let command = self.queue.commandBuffer().unwrap();
        // Match Dawn: serial dispatch plus tracked resources supplies inter-dispatch
        // ordering. Explicit full-buffer barriers would redundantly flush caches.
        let encoder = command
            .computeCommandEncoderWithDispatchType(MTLDispatchType::Serial)
            .unwrap();
        let stages = [
            self.chunks.div_ceil(64),
            1,
            self.chunks,
            4,
            self.chunks,
            1,
            self.chunks,
        ];
        for (stage, groups) in stages.into_iter().enumerate() {
            encoder.setComputePipelineState(&self.pipelines[stage]);
            let mut lengths = [0u32; 8];
            for (slot, &index) in BINDINGS[stage].iter().enumerate() {
                let buffer = if index == 99 {
                    &self.weights
                } else {
                    &self.buffers[index]
                };
                lengths[slot] = u32::try_from(if index == 99 {
                    buffer.length()
                } else {
                    self.binding_sizes[index]
                })
                .unwrap();
                // SAFETY: exact captured MSL binding order, offset zero, retained
                // buffers outlive GPU completion. Read/write types match the shader.
                unsafe { encoder.setBuffer_offset_atIndex(Some(buffer), 0, slot) };
            }
            // SAFETY: setBytes copies these 32 bytes. Buffer 30 is the captured
            // shader's storage-size metadata (uint4[2]); all slot sizes are exact.
            unsafe {
                encoder.setBytes_length_atIndex(
                    NonNull::new(lengths.as_mut_ptr().cast()).unwrap(),
                    32,
                    30,
                )
            };
            if SHARED_BYTES[stage] > 0 {
                // SAFETY: exact threadgroup structure size in captured MSL, index 0.
                unsafe { encoder.setThreadgroupMemoryLength_atIndex(SHARED_BYTES[stage], 0) };
            }
            let x = groups.min(65535);
            encoder.dispatchThreadgroups_threadsPerThreadgroup(
                MTLSize {
                    width: x as usize,
                    height: groups.div_ceil(x) as usize,
                    depth: 1,
                },
                MTLSize {
                    width: THREADS[stage],
                    height: 1,
                    depth: 1,
                },
            );
            if stage < 6 && std::env::var_os("LEXER_METAL_FULL_BARRIERS").is_some() {
                encoder.memoryBarrierWithScope(MTLBarrierScope::Buffers);
            }
        }
        encoder.endEncoding();
        let encoded = Instant::now();
        let submitted;
        match self.wait_mode {
            WaitMode::Event => {
                self.event_value = self
                    .event_value
                    .checked_add(1)
                    .expect("Metal event counter overflow");
                // The signal follows all compute work on this serial queue. Shared
                // event completion makes the shared output visible to the CPU;
                // no completion-handler/GCD/channel round trip is required.
                command.encodeSignalEvent_value(self.event.as_ref(), self.event_value);
                command.commit();
                submitted = Instant::now();
                assert!(
                    self.event
                        .waitUntilSignaledValue_timeoutMS(self.event_value, 60_000),
                    "Metal event timeout"
                );
            }
            WaitMode::Direct => {
                command.commit();
                submitted = Instant::now();
                command.waitUntilCompleted();
            }
            WaitMode::Callback => {
                let (tx, rx) = std::sync::mpsc::channel();
                let callback = block2::RcBlock::new(move |_| {
                    let _ = tx.send(());
                });
                // SAFETY: Metal copies the block, which owns its sender.
                unsafe { command.addCompletedHandler(block2::RcBlock::as_ptr(&callback)) };
                command.commit();
                submitted = Instant::now();
                rx.recv_timeout(Duration::from_secs(60))
                    .expect("Metal completion timeout");
            }
        }
        assert!(command.error().is_none(), "Metal execution error");
        let waited = Instant::now();
        // SAFETY: completion establishes GPU->CPU visibility for shared buffers;
        // count is within the output buffer. Copy before the next invocation.
        let labels = unsafe {
            std::slice::from_raw_parts(self.buffers[1].contents().as_ptr().cast::<u8>(), count)
        }
        .to_vec();
        if self.profile {
            let finished = Instant::now();
            command.waitUntilCompleted(); // Diagnostic GPU timestamps need command completion.
            eprintln!(
                "{}",
                serde_json::json!({"tokens":count,"prepare_us":(prepared-start).as_secs_f64()*1e6,"encode_us":(encoded-prepared).as_secs_f64()*1e6,"submit_us":(submitted-encoded).as_secs_f64()*1e6,"wait_us":(waited-submitted).as_secs_f64()*1e6,"read_us":(finished-waited).as_secs_f64()*1e6,"total_us":(finished-start).as_secs_f64()*1e6,"gpu_us":(command.GPUEndTime()-command.GPUStartTime())*1e6})
            );
        }
        labels
    }
    pub fn highlight(&mut self, code: &str) -> Vec<(usize, usize, u32)> {
        let tokens = tokenizer::tokenize(code);
        let labels = self.labels(&tokens.features);
        let mut runs: Vec<(usize, usize, u32)> = Vec::new();
        for ((start, end), label) in tokens.ranges.into_iter().zip(labels) {
            assert!(label < 9 && code.is_char_boundary(start) && code.is_char_boundary(end));
            if let Some(last) = runs.last_mut()
                && last.1 == start
                && last.2 == label as u32
            {
                last.1 = end;
                continue;
            }
            runs.push((start, end, label as u32));
        }
        runs
    }
}
