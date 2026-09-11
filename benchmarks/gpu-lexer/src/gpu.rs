use crate::tokenizer;
use std::{
    num::NonZeroU64,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;
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
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: if f16 {
                wgpu::Features::SHADER_F16
            } else {
                wgpu::Features::empty()
            },
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .unwrap();
        let source = include_str!("../assets/model.wgsl");
        let source = if f16 {
            source.to_owned()
        } else {
            source
                .strip_prefix("enable f16;")
                .unwrap()
                .replace("f16", "f32")
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-lexer 0.0.2"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipelines = ["a", "b", "c", "d", "e", "f", "g"]
            .into_iter()
            .map(|entry| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect();
        let weights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("weights"),
            contents: include_bytes!("../assets/weights.f32"),
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self {
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
            profile: false,
        }
    }
    fn prepare(&mut self, count: usize) {
        if count == self.count {
            return;
        }
        self.count = count;
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
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            for (i, n) in [
                self.chunks.div_ceil(64),
                1,
                self.chunks,
                4,
                self.chunks,
                1,
                self.chunks,
            ]
            .into_iter()
            .enumerate()
            {
                pass.set_pipeline(&self.pipelines[i]);
                pass.set_bind_group(0, &self.groups[i], &[]);
                let x = n.min(65535);
                pass.dispatch_workgroups(x, n.div_ceil(x), 1);
            }
        }
        encoder.copy_buffer_to_buffer(
            &self.buffers[1],
            0,
            &self.buffers[2],
            0,
            self.buffers[1].size(),
        );
        let encoded = Instant::now();
        let submission = self.queue.submit([encoder.finish()]);
        let submitted = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffers[2]
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        let waited = Instant::now();
        rx.recv().unwrap().unwrap();
        let labels = self.buffers[2].slice(..).get_mapped_range()[..count].to_vec();
        self.buffers[2].unmap();
        if self.profile {
            eprintln!(
                "{}",
                serde_json::json!({"tokens":count,"prepare_us":(prepared-started).as_secs_f64()*1e6,"encode_us":(encoded-prepared).as_secs_f64()*1e6,"submit_us":(submitted-encoded).as_secs_f64()*1e6,"wait_us":(waited-submitted).as_secs_f64()*1e6,"total_us":started.elapsed().as_secs_f64()*1e6})
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
