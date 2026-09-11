use crate::tokenizer;
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue};
use std::{
    num::NonZeroU64,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;
struct Queries {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Vec<wgpu::ComputePipeline>,
    weights: wgpu::Buffer,
    buffers: Vec<wgpu::Buffer>,
    groups: Vec<wgpu::BindGroup>,
    count: usize,
    chunks: u32,
    pub adapter: String,
    pub f16: bool,
    pub profile: bool,
    queries: Option<Queries>,
    tiny_pipeline: Option<wgpu::ComputePipeline>,
    tiny_group: Option<wgpu::BindGroup>,
}
impl Gpu {
    pub fn new(force_f32: bool) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("Metal adapter");
        let f16 = !force_f32 && adapter.features().contains(wgpu::Features::SHADER_F16);
        let info = format!("{:?}", adapter.get_info());
        let timestamps = std::env::var_os("LEXER_GPU_TIMESTAMPS").is_some();
        let timing_features = if timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        assert!(
            adapter.features().contains(timing_features),
            "GPU timestamp features unavailable"
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: timing_features
                | if f16 {
                    wgpu::Features::SHADER_F16
                } else {
                    wgpu::Features::empty()
                },
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .unwrap();
        let no_bounds_checks = std::env::var_os("LEXER_NO_BOUNDS_CHECKS").is_some();
        let use_tiny = std::env::var_os("LEXER_TINY").is_some();
        let transpose = std::env::var_os("LEXER_TRANSPOSE").is_some();
        let source = if std::env::var_os("LEXER_CANONICAL").is_some() {
            include_str!("../assets/canonical.wgsl")
        } else if transpose {
            include_str!("../assets/transposed.wgsl")
        } else if use_tiny {
            include_str!("../assets/tiny.wgsl")
        } else if no_bounds_checks {
            include_str!("../assets/validated.wgsl")
        } else if std::env::var_os("LEXER_SOA").is_some() {
            include_str!("../assets/soa.wgsl")
        } else {
            include_str!("../assets/model.wgsl")
        };
        let source = if f16 {
            source.to_owned()
        } else {
            source
                .strip_prefix("enable f16;")
                .unwrap()
                .replace("f16", "f32")
        };
        let descriptor = wgpu::ShaderModuleDescriptor {
            label: Some("gpu-lexer 0.0.2"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        };
        let module = if no_bounds_checks || std::env::var_os("LEXER_NO_LOOP_BOUNDING").is_some() {
            // SAFETY: only the pinned shader is compiled. Its loops are fixed-size,
            // bounded by validated token/chunk counts, or halve/double powers of
            // two up to the validated buffer limit. No token feature controls a
            // loop limit. Runtime array bounds checks remain enabled.
            unsafe {
                device.create_shader_module_trusted(
                    descriptor,
                    wgpu::ShaderRuntimeChecks {
                        force_loop_bounding: false,
                        bounds_checks: !no_bounds_checks,
                        ..wgpu::ShaderRuntimeChecks::checked()
                    },
                )
            }
        } else {
            device.create_shader_module(descriptor)
        };
        let pipelines = ["a", "b", "c", "d", "e", "f", "g"]
            .into_iter()
            .map(|entry| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        zero_initialize_workgroup_memory: std::env::var_os(
                            "LEXER_NO_WORKGROUP_INIT",
                        )
                        .is_none(),
                        ..Default::default()
                    },
                    cache: None,
                })
            })
            .collect();
        let tiny_pipeline = use_tiny.then(|| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("single-chunk exact fusion"),
                layout: None,
                module: &module,
                entry_point: Some("tiny"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    zero_initialize_workgroup_memory: std::env::var_os("LEXER_NO_WORKGROUP_INIT")
                        .is_none(),
                    ..Default::default()
                },
                cache: None,
            })
        });
        let weights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("weights"),
            contents: if transpose {
                include_bytes!("../assets/weights-transposed.f32")
            } else {
                include_bytes!("../assets/weights.f32")
            },
            usage: wgpu::BufferUsages::STORAGE,
        });
        let queries = timestamps.then(|| Queries {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("stage timings"),
                ty: wgpu::QueryType::Timestamp,
                count: 14,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 14 * 8,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 14 * 8,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        });
        Self {
            tiny_pipeline,
            tiny_group: None,
            queries,
            device,
            queue,
            pipelines,
            weights,
            buffers: Vec::new(),
            groups: Vec::new(),
            count: 0,
            chunks: 0,
            adapter: info,
            f16,
            profile: std::env::var_os("LEXER_PROFILE").is_some(),
        }
    }
    fn prepare(&mut self, count: usize) {
        if count == self.count {
            return;
        }
        self.count = count;
        assert!(
            count <= self.device.limits().max_storage_buffer_binding_size as usize / 128,
            "token count exceeds the largest scratch buffer binding"
        );
        let chunks = (count as u32).div_ceil(32);
        self.chunks = chunks;
        let leaves = chunks.next_power_of_two();
        let nodes = leaves * 2 - 1;
        let output = (count as u64).div_ceil(4) * 4;
        let scalar = if self.f16 { 2 } else { 4 };
        use wgpu::BufferUsages as U;
        let storage = U::STORAGE | U::COPY_DST;
        let descriptors = [
            (count as u64 * 8, storage),
            (output, storage | U::COPY_SRC),
            (output, U::MAP_READ | U::COPY_DST),
            (16, U::UNIFORM | U::COPY_DST),
            (24, storage),
            (chunks as u64 * 16, storage),
            (count as u64 * 32 * scalar, U::STORAGE),
            (nodes as u64 * 32 * scalar, U::STORAGE),
            (
                (chunks as u64 * 32 * 16).max(nodes as u64 * 32 * 4),
                U::STORAGE,
            ),
            (chunks as u64 * 8, U::STORAGE),
            (count as u64 * 32 * 4, U::STORAGE),
        ];
        self.groups.clear();
        self.tiny_group = None;
        self.buffers = descriptors
            .into_iter()
            .map(|(size, usage)| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage,
                    mapped_at_creation: false,
                })
            })
            .collect();
        self.queue.write_buffer(
            &self.buffers[3],
            0,
            bytemuck::cast_slice(&[1u32, count as u32, chunks, nodes]),
        );
        self.queue.write_buffer(
            &self.buffers[4],
            0,
            bytemuck::cast_slice(&[0u32, count as u32, 0, chunks, 0, leaves]),
        );
        let chunk_data: Vec<u32> = (0..chunks)
            .flat_map(|c| [c * 32, (count as u32 - c * 32).min(32), 0, c])
            .collect();
        self.queue
            .write_buffer(&self.buffers[5], 0, bytemuck::cast_slice(&chunk_data));
        if let Some(pipeline) = &self.tiny_pipeline {
            let entries: Vec<_> = [(0, 0), (1, 1), (2, 3), (4, 99)]
                .into_iter()
                .map(|(binding, index)| {
                    let buffer = if index == 99 {
                        &self.weights
                    } else {
                        &self.buffers[index]
                    };
                    wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    }
                })
                .collect();
            self.tiny_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            }));
        }
        let bindings: [&[(u32, usize)]; 7] = [
            &[(0, 0), (2, 3), (9, 5), (12, 9)],
            &[(2, 3), (3, 4), (12, 9)],
            &[
                (0, 0),
                (2, 3),
                (3, 4),
                (4, 99),
                (9, 5),
                (10, 10),
                (11, 8),
                (12, 9),
            ],
            &[(2, 3), (3, 4), (11, 8)],
            &[
                (2, 3),
                (3, 4),
                (4, 99),
                (5, 6),
                (6, 7),
                (9, 5),
                (10, 10),
                (11, 8),
            ],
            &[(2, 3), (3, 4), (4, 99), (6, 7), (11, 8)],
            &[
                (0, 0),
                (1, 1),
                (2, 3),
                (4, 99),
                (3, 4),
                (5, 6),
                (9, 5),
                (11, 8),
            ],
        ];
        self.groups = bindings
            .iter()
            .enumerate()
            .map(|(p, bindings)| {
                let entries: Vec<_> = bindings
                    .iter()
                    .map(|&(binding, index)| {
                        let buffer = if index == 99 {
                            &self.weights
                        } else {
                            &self.buffers[index]
                        };
                        wgpu::BindGroupEntry {
                            binding,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer,
                                offset: 0,
                                size: NonZeroU64::new(buffer.size()),
                            }),
                        }
                    })
                    .collect();
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.pipelines[p].get_bind_group_layout(0),
                    entries: &entries,
                })
            })
            .collect();
    }
    pub fn labels(&mut self, features: &[u32]) -> Vec<u8> {
        assert_eq!(features.len() % 2, 0, "two feature words per token");
        let count = features.len() / 2;
        if count == 0 {
            return Vec::new();
        }
        let started = Instant::now();
        self.prepare(count);
        let prepared = Instant::now();
        self.queue
            .write_buffer(&self.buffers[0], 0, bytemuck::cast_slice(features));
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.clear_buffer(&self.buffers[1], 0, None);
        let stages = [
            self.chunks.div_ceil(64),
            1,
            self.chunks,
            4,
            self.chunks,
            1,
            self.chunks,
        ];
        let dispatch = |pass: &mut wgpu::ComputePass<'_>, i: usize, n: u32| {
            pass.set_pipeline(&self.pipelines[i]);
            pass.set_bind_group(0, &self.groups[i], &[]);
            let x = n.min(65535);
            pass.dispatch_workgroups(x, n.div_ceil(x), 1);
        };
        if count <= 32 && self.tiny_pipeline.is_some() && self.queries.is_none() {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(self.tiny_pipeline.as_ref().unwrap());
            pass.set_bind_group(0, self.tiny_group.as_ref().unwrap(), &[]);
            pass.dispatch_workgroups(1, 1, 1);
        } else if let Some(q) = &self.queries {
            // Diagnostic only: Metal timestamps are supported at pass boundaries.
            // Normal benchmarking retains the original single compute pass.
            for (i, n) in stages.into_iter().enumerate() {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &q.set,
                        beginning_of_pass_write_index: Some(i as u32 * 2),
                        end_of_pass_write_index: Some(i as u32 * 2 + 1),
                    }),
                });
                dispatch(&mut pass, i, n);
            }
        } else {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            for (i, n) in stages.into_iter().enumerate() {
                dispatch(&mut pass, i, n);
            }
        }
        encoder.copy_buffer_to_buffer(
            &self.buffers[1],
            0,
            &self.buffers[2],
            0,
            self.buffers[1].size(),
        );
        if let Some(q) = &self.queries {
            encoder.resolve_query_set(&q.set, 0..14, &q.resolve, 0);
            encoder.copy_buffer_to_buffer(&q.resolve, 0, &q.readback, 0, 14 * 8);
        }
        let encoded = Instant::now();
        let submission = self.queue.submit([encoder.finish()]);
        let submitted = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffers[2]
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        let query_rx = self.queries.as_ref().map(|q| {
            let (tx, rx) = std::sync::mpsc::channel();
            q.readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
            rx
        });
        if std::env::var("LEXER_WAIT").as_deref() == Ok("metal") {
            // SAFETY: append only an empty completion marker to the same Metal queue.
            // We neither destroy nor mutate any resource owned by wgpu. This driver
            // owns its queue exclusively while highlighting (&mut self).
            let hal = unsafe { self.queue.as_hal::<wgpu::hal::api::Metal>() }.unwrap();
            let marker = hal
                .as_raw()
                .commandBuffer()
                .expect("Metal completion marker");
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let callback = block2::RcBlock::new(move |_| {
                let _ = done_tx.send(());
            });
            // SAFETY: Metal copies the block; the closure owns its sender and does
            // not access resources after their lifetime. It only signals completion.
            unsafe { marker.addCompletedHandler(block2::RcBlock::as_ptr(&callback)) };
            marker.commit();
            done_rx
                .recv_timeout(Duration::from_secs(60))
                .expect("Metal completion timeout");
            assert!(marker.error().is_none(), "Metal completion marker failed");
            drop(hal);
            self.device.poll(wgpu::PollType::Poll).unwrap();
        } else {
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(60)),
                })
                .unwrap();
        }
        let waited = Instant::now();
        rx.recv().unwrap().unwrap();
        let labels = self.buffers[2].slice(..).get_mapped_range()[..count].to_vec();
        self.buffers[2].unmap();
        let mut gpu_stages_us = Vec::new();
        if let Some(q) = &self.queries {
            query_rx.unwrap().recv().unwrap().unwrap();
            {
                let mapped = q.readback.slice(..).get_mapped_range();
                let stamps: &[u64] = bytemuck::cast_slice(&mapped);
                gpu_stages_us = stamps
                    .chunks_exact(2)
                    .map(|p| {
                        (p[1] - p[0]) as f64 * self.queue.get_timestamp_period() as f64 / 1000.
                    })
                    .collect();
            }
            q.readback.unmap();
        }
        if self.profile {
            eprintln!(
                "{}",
                serde_json::json!({"gpu_stages_us":gpu_stages_us,"tokens":count,"prepare_us":(prepared-started).as_secs_f64()*1e6,"encode_us":(encoded-prepared).as_secs_f64()*1e6,"submit_us":(submitted-encoded).as_secs_f64()*1e6,"wait_us":(waited-submitted).as_secs_f64()*1e6,"read_us":waited.elapsed().as_secs_f64()*1e6,"total_us":started.elapsed().as_secs_f64()*1e6})
            );
        }
        labels
    }
    pub fn highlight(&mut self, code: &str) -> Vec<(usize, usize, u32)> {
        let tokens = tokenizer::tokenize(code);
        let labels = self.labels(&tokens.features);
        let mut runs: Vec<(usize, usize, u32)> = Vec::new();
        for ((start, end), label) in tokens.ranges.into_iter().zip(labels) {
            assert!(label < 9);
            assert!(code.is_char_boundary(start) && code.is_char_boundary(end));
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
