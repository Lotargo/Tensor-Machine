use wgpu::{Device, Queue, Instance, RequestAdapterOptions};
use std::sync::Arc;

pub struct GpuContext {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub shader: Arc<wgpu::ShaderModule>,
}

pub const SHADER_SRC: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<f32>;

@compute @workgroup_size(64)
fn add_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index < arrayLength(&a)) {
        result[index] = a[index] + b[index];
    }
}

struct ScalarUniform {
    val: f32,
}
@group(0) @binding(3) var<uniform> scalar: ScalarUniform;

@compute @workgroup_size(64)
fn mul_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index < arrayLength(&a)) {
        result[index] = a[index] * scalar.val;
    }
}

struct FusionWeights {
    w1: f32,
    w2: f32,
}

@group(0) @binding(4) var<uniform> weights: FusionWeights;

@compute @workgroup_size(64)
fn fusion_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index < arrayLength(&a)) {
        result[index] = (a[index] * weights.w1) + (b[index] * weights.w2);
    }
}

@compute @workgroup_size(64)
fn matmul_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index < arrayLength(&a)) {
        result[index] = a[index] * b[index];
    }
}

// Single-dispatch partial selection sort for Top-K
@group(0) @binding(5) var<storage, read_write> data: array<f32>;
@group(0) @binding(6) var<storage, read_write> indices: array<u32>;
struct KUniform {
    val: u32,
}
@group(0) @binding(7) var<uniform> k_val: KUniform;

@compute @workgroup_size(1)
fn topk_sort_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let n = arrayLength(&data);
    let k = min(k_val.val, n);

    // Perform selection sort for the first k elements
    for (var i = 0u; i < k; i = i + 1u) {
        var max_idx = i;
        var max_val = data[i];

        for (var j = i + 1u; j < n; j = j + 1u) {
            if (data[j] > max_val) {
                max_val = data[j];
                max_idx = j;
            }
        }

        if (max_idx != i) {
            let tmp_val = data[i];
            data[i] = data[max_idx];
            data[max_idx] = tmp_val;

            let tmp_idx = indices[i];
            indices[i] = indices[max_idx];
            indices[max_idx] = tmp_idx;
        }
    }
}

struct ColbertParams {
    num_query_tokens: u32,
    dim: u32,
}
@group(0) @binding(8) var<uniform> colbert_params: ColbertParams;

@group(0) @binding(9) var<storage, read> offsets: array<vec2<u32>>;

@compute @workgroup_size(64)
fn colbert_maxsim_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let doc_idx = global_id.x;
    let num_docs = arrayLength(&result);
    if (doc_idx >= num_docs) {
        return;
    }

    let dim = colbert_params.dim;
    let num_q = colbert_params.num_query_tokens;
    let offs = offsets[doc_idx];
    let num_d = offs.y;

    var total_maxsim = 0.0;

    for (var q = 0u; q < num_q; q = q + 1u) {
        var max_sim = -1e10;
        for (var d = 0u; d < num_d; d = d + 1u) {
            var sim = 0.0;
            for (var i = 0u; i < dim; i = i + 1u) {
                let q_val = a[q * dim + i];
                let d_val = b[(offs.x + d) * dim + i];
                sim = sim + q_val * d_val;
            }
            if (sim > max_sim) {
                max_sim = sim;
            }
        }
        total_maxsim = total_maxsim + max_sim;
    }
    result[doc_idx] = total_maxsim;
}

// Tile size constants (must be compile-time for WGSL var<workgroup>)
const TILE_Q: u32 = 16u;
const TILE_D: u32 = 16u;
const DIM_MAX: u32 = 128u;

var<workgroup> q_tile: array<array<f32, DIM_MAX>, TILE_Q>;
var<workgroup> d_tile: array<array<f32, DIM_MAX>, TILE_D>;
var<workgroup> partial_maxsim: array<array<f32, TILE_D>, TILE_Q>;
var<workgroup> q_max: array<f32, TILE_Q>;

@compute @workgroup_size(256)
fn tiled_colbert_maxsim_main(
    @builtin(workgroup_id) group_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
) {
    let doc_idx = group_id.x;
    let num_docs = arrayLength(&result);
    if (doc_idx >= num_docs) {
        return;
    }

    let dim = colbert_params.dim;
    let num_q = colbert_params.num_query_tokens;

    let offs = offsets[doc_idx];
    let num_d = offs.y;
    let doc_start = offs.x;

    let tid = local_id.x;

    // Total maxsim for this document, kept in registers for lane 0
    var doc_score = 0.0;

    for (var q_start = 0u; q_start < num_q; q_start = q_start + TILE_Q) {
        let q_end = min(q_start + TILE_Q, num_q);
        let current_q_tile = q_end - q_start;

        // Initialize q_max for this query tile
        if (tid < current_q_tile) {
            q_max[tid] = -1e10;
        }

        // 1. Cooperative load of Q tile (hoisted outside D loop)
        // 256 threads loading (current_q_tile * dim) elements
        let total_q_elements = current_q_tile * dim;
        for (var i = tid; i < total_q_elements; i = i + 256u) {
            let q_row = i / dim;
            let q_col = i % dim;
            q_tile[q_row][q_col] = a[(q_start + q_row) * dim + q_col];
        }
        workgroupBarrier();

        for (var d_start = 0u; d_start < num_d; d_start = d_start + TILE_D) {
            let d_end = min(d_start + TILE_D, num_d);
            let current_d_tile = d_end - d_start;

            // 2. Cooperative load of D tile
            let total_d_elements = current_d_tile * dim;
            for (var i = tid; i < total_d_elements; i = i + 256u) {
                let d_row = i / dim;
                let d_col = i % dim;
                d_tile[d_row][d_col] = b[(doc_start + d_start + d_row) * dim + d_col];
            }
            workgroupBarrier();

            // 3. Compute dot products cooperatively
            // We have current_q_tile * current_d_tile pairs to compute
            let total_pairs = current_q_tile * current_d_tile;
            for (var i = tid; i < total_pairs; i = i + 256u) {
                let q_idx = i / current_d_tile;
                let d_idx = i % current_d_tile;

                var sim = 0.0;
                for (var k = 0u; k < dim; k = k + 1u) {
                    sim = sim + q_tile[q_idx][k] * d_tile[d_idx][k];
                }
                partial_maxsim[q_idx][d_idx] = sim;
            }
            workgroupBarrier();

            // 4. Update q_max cooperatively
            if (tid < current_q_tile) {
                var local_max = q_max[tid];
                for (var d_idx = 0u; d_idx < current_d_tile; d_idx = d_idx + 1u) {
                    let sim = partial_maxsim[tid][d_idx];
                    if (sim > local_max) {
                        local_max = sim;
                    }
                }
                q_max[tid] = local_max;
            }
            workgroupBarrier();
        }

        // 5. Accumulate total score for this document
        if (tid == 0u) {
            for (var i = 0u; i < current_q_tile; i = i + 1u) {
                doc_score = doc_score + q_max[i];
            }
        }
    }

    if (tid == 0u) {
        result[doc_idx] = doc_score;
    }
}
"#;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColbertOffsets {
    pub doc_start: u32,
    pub num_tokens: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ColbertParams {
    num_query_tokens: u32,
    dim: u32,
    _padding: [u32; 2],
}

impl GpuContext {
    pub async fn new() -> Option<Self> {
        let instance = Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await.ok()?;

        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Global Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        Some(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            shader: Arc::new(shader),
        })
    }

    fn run_compute_shader(
        &self,
        entry_point: &str,
        bind_groups: &[wgpu::BindGroupEntry],
        workgroup_count: u32,
        result_buffer: &wgpu::Buffer,
        size: wgpu::BufferAddress,
    ) -> Option<Vec<u8>> {
        let compute_pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry_point),
            layout: None,
            module: &self.shader,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        });

        let bind_group_layout = compute_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(entry_point),
            layout: &bind_group_layout,
            entries: bind_groups,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            cpass.set_pipeline(&compute_pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging_buffer"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_buffer_to_buffer(result_buffer, 0, &staging_buffer, 0, size);
        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);

        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
            sender.send(v).unwrap();
        });

        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().ok()?.ok()?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<u8> = data.to_vec();
        drop(data);
        staging_buffer.unmap();

        Some(result)
    }

    pub fn add(&self, a: &[f32], b: &[f32]) -> Option<Vec<f32>> {
        if a.len() != b.len() {
            return None;
        }
        let size = (a.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

        let buffer_a = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_a"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_b"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_result"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(a));
        self.queue.write_buffer(&buffer_b, 0, bytemuck::cast_slice(b));

        let bind_groups = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer_b.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer_result.as_entire_binding(),
            },
        ];

        let workgroup_count = (a.len() as u32 + 63) / 64;
        let result = self.run_compute_shader("add_main", &bind_groups, workgroup_count, &buffer_result, size)?;
        Some(bytemuck::cast_slice(&result).to_vec())
    }

    pub fn mul(&self, a: &[f32], scalar: f32) -> Option<Vec<f32>> {
        let size = (a.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

        let buffer_a = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_a"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_result"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffer_scalar = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_scalar"),
            size: std::mem::size_of::<f32>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(a));
        self.queue.write_buffer(&buffer_scalar, 0, bytemuck::bytes_of(&scalar));

        let bind_groups = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer_result.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: buffer_scalar.as_entire_binding(),
            },
        ];

        let workgroup_count = (a.len() as u32 + 63) / 64;
        let result = self.run_compute_shader("mul_main", &bind_groups, workgroup_count, &buffer_result, size)?;
        Some(bytemuck::cast_slice(&result).to_vec())
    }

    pub fn fusion(&self, a: &[f32], b: &[f32], w1: f32, w2: f32) -> Option<Vec<f32>> {
        if a.len() != b.len() {
            return None;
        }
        let size = (a.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

        let buffer_a = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_a"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_b"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_result"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffer_weights = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_weights"),
            size: (2 * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(a));
        self.queue.write_buffer(&buffer_b, 0, bytemuck::cast_slice(b));
        self.queue.write_buffer(&buffer_weights, 0, bytemuck::cast_slice(&[w1, w2]));

        let bind_groups = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer_b.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer_result.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: buffer_weights.as_entire_binding(),
            },
        ];

        let workgroup_count = (a.len() as u32 + 63) / 64;
        let result = self.run_compute_shader("fusion_main", &bind_groups, workgroup_count, &buffer_result, size)?;
        Some(bytemuck::cast_slice(&result).to_vec())
    }

    pub fn matmul(&self, a: &[f32], b: &[f32]) -> Option<f32> {
        if a.len() != b.len() {
            return None;
        }
        let size = (a.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

        let buffer_a = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_a"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_b"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_result"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(a));
        self.queue.write_buffer(&buffer_b, 0, bytemuck::cast_slice(b));

        let bind_groups = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer_b.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer_result.as_entire_binding(),
            },
        ];

        let workgroup_count = (a.len() as u32 + 63) / 64;
        let result = self.run_compute_shader("matmul_main", &bind_groups, workgroup_count, &buffer_result, size)?;

        let multiplied: &[f32] = bytemuck::cast_slice(&result);
        let sum = multiplied.iter().sum();
        Some(sum)
    }

    pub fn topk(&self, data: &[f32], k: usize) -> Option<(Vec<usize>, Vec<f32>)> {
        let n = data.len();
        if n == 0 {
            return Some((vec![], vec![]));
        }

        let size = (n * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let index_size = (n * std::mem::size_of::<u32>()) as wgpu::BufferAddress;

        let buffer_data = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_data"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let buffer_indices = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_indices"),
            size: index_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let buffer_k_val = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_k_val"),
            size: std::mem::size_of::<u32>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let indices: Vec<u32> = (0..n as u32).collect();
        self.queue.write_buffer(&buffer_data, 0, bytemuck::cast_slice(data));
        self.queue.write_buffer(&buffer_indices, 0, bytemuck::cast_slice(&indices));
        let k_u32 = k as u32;
        self.queue.write_buffer(&buffer_k_val, 0, bytemuck::bytes_of(&k_u32));

        let compute_pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("TopK Pipeline"),
            layout: None,
            module: &self.shader,
            entry_point: Some("topk_sort_main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let bind_group_layout = compute_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("TopK Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: buffer_data.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: buffer_indices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: buffer_k_val.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            cpass.set_pipeline(&compute_pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(1, 1, 1);
        }
        self.queue.submit(Some(encoder.finish()));

        let staging_buffer_data = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging_buffer_data"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let staging_buffer_indices = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging_buffer_indices"),
            size: index_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&buffer_data, 0, &staging_buffer_data, 0, size);
        encoder.copy_buffer_to_buffer(&buffer_indices, 0, &staging_buffer_indices, 0, index_size);
        self.queue.submit(Some(encoder.finish()));

        let (sender1, receiver1) = std::sync::mpsc::channel();
        let buffer_slice_data = staging_buffer_data.slice(..);
        buffer_slice_data.map_async(wgpu::MapMode::Read, move |v| {
            sender1.send(v).unwrap();
        });

        let (sender2, receiver2) = std::sync::mpsc::channel();
        let buffer_slice_indices = staging_buffer_indices.slice(..);
        buffer_slice_indices.map_async(wgpu::MapMode::Read, move |v| {
            sender2.send(v).unwrap();
        });

        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver1.recv().ok()?.ok()?;
        receiver2.recv().ok()?.ok()?;

        let mapped_data = buffer_slice_data.get_mapped_range();
        let sorted_data: Vec<f32> = bytemuck::cast_slice(&mapped_data).to_vec();
        drop(mapped_data);
        staging_buffer_data.unmap();

        let mapped_indices = buffer_slice_indices.get_mapped_range();
        let sorted_indices: Vec<u32> = bytemuck::cast_slice(&mapped_indices).to_vec();
        drop(mapped_indices);
        staging_buffer_indices.unmap();

        let k = k.min(n);
        let final_scores = sorted_data.into_iter().take(k).collect();
        let final_indices = sorted_indices.into_iter().take(k).map(|x| x as usize).collect();

        Some((final_indices, final_scores))
    }

    pub fn colbert_maxsim(&self, query_tokens: &[f32], doc_tokens: &[f32], num_docs: u32, num_q: u32, offsets: &[ColbertOffsets], dim: u32) -> Option<Vec<f32>> {
        self.colbert_maxsim_variant(query_tokens, doc_tokens, num_docs, num_q, offsets, dim, true)
    }

    pub fn colbert_maxsim_variant(&self, query_tokens: &[f32], doc_tokens: &[f32], num_docs: u32, num_q: u32, offsets: &[ColbertOffsets], dim: u32, use_tiled: bool) -> Option<Vec<f32>> {
        let size_q = (query_tokens.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let size_d = (doc_tokens.len() * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let size_res = (num_docs as usize * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let size_offsets = (offsets.len() * std::mem::size_of::<ColbertOffsets>()) as wgpu::BufferAddress;

        let buffer_q = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_q"),
            size: size_q,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_d = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_d"),
            size: size_d,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_result = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_result"),
            size: size_res,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let buffer_params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_params"),
            size: std::mem::size_of::<ColbertParams>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buffer_offsets = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buffer_offsets"),
            size: size_offsets,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_q, 0, bytemuck::cast_slice(query_tokens));
        self.queue.write_buffer(&buffer_d, 0, bytemuck::cast_slice(doc_tokens));
        self.queue.write_buffer(&buffer_offsets, 0, bytemuck::cast_slice(offsets));
        let params = ColbertParams {
            num_query_tokens: num_q,
            dim,
            _padding: [0; 2],
        };
        self.queue.write_buffer(&buffer_params, 0, bytemuck::bytes_of(&params));

        let bind_groups = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer_q.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer_d.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffer_result.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: buffer_params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: buffer_offsets.as_entire_binding(),
            },
        ];

        let (entry_point, workgroup_count) = if use_tiled {
            ("tiled_colbert_maxsim_main", num_docs)
        } else {
            ("colbert_maxsim_main", (num_docs + 63) / 64)
        };

        let result = self.run_compute_shader(entry_point, &bind_groups, workgroup_count, &buffer_result, size_res)?;
        Some(bytemuck::cast_slice(&result).to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_add() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let a = vec![1.0, 2.0, 3.0, 4.0];
                let b = vec![5.0, 6.0, 7.0, 8.0];
                let result = gpu.add(&a, &b).unwrap();
                assert_eq!(result, vec![6.0, 8.0, 10.0, 12.0]);
            }
        });
    }

    #[test]
    fn test_gpu_mul() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let a = vec![1.0, 2.0, 3.0, 4.0];
                let result = gpu.mul(&a, 2.0).unwrap();
                assert_eq!(result, vec![2.0, 4.0, 6.0, 8.0]);
            }
        });
    }

    #[test]
    fn test_gpu_fusion() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let a = vec![1.0, 1.0, 1.0];
                let b = vec![2.0, 2.0, 2.0];
                let result = gpu.fusion(&a, &b, 0.5, 0.5).unwrap();
                assert_eq!(result, vec![1.5, 1.5, 1.5]);
            }
        });
    }

    #[test]
    fn test_gpu_matmul() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let a = vec![1.0, 2.0, 3.0];
                let b = vec![4.0, 5.0, 6.0];
                let result = gpu.matmul(&a, &b).unwrap();
                assert_eq!(result, 32.0);
            }
        });
    }

    #[test]
    fn test_gpu_topk() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let data = vec![1.0, 5.0, 3.0, 9.0, 2.0];
                let (indices, scores) = gpu.topk(&data, 3).unwrap();
                assert_eq!(indices, vec![3, 1, 2]);
                assert_eq!(scores, vec![9.0, 5.0, 3.0]);
            }
        });
    }

    #[test]
    fn test_gpu_colbert_maxsim() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Some(gpu) = GpuContext::new().await {
                let dim = 128;
                let num_q = 10;
                let num_d = 20;
                let num_docs = 2;

                let mut q_tokens = vec![0.1f32; num_q * dim];
                // Make first q-token special
                for i in 0..dim { q_tokens[i] = 1.0; }

                let mut d_tokens = vec![0.0f32; num_docs * num_d * dim];
                // Doc 0, first d-token matches q-token 0 perfectly
                for i in 0..dim { d_tokens[i] = 1.0; }

                // Doc 1, first d-token matches q-token 0 partially
                for i in 0..dim { d_tokens[num_d * dim + i] = 0.5; }

                let offsets = vec![
                    ColbertOffsets { doc_start: 0, num_tokens: num_d as u32 },
                    ColbertOffsets { doc_start: num_d as u32, num_tokens: num_d as u32 },
                ];

                let result = gpu.colbert_maxsim(&q_tokens, &d_tokens, num_docs as u32, num_q as u32, &offsets, dim as u32).unwrap();

                assert_eq!(result.len(), num_docs);
                assert!(result[0] > result[1]);

                // CPU Reference implementation
                let mut cpu_results = vec![0.0f32; num_docs];
                for doc_idx in 0..num_docs {
                    let mut doc_score = 0.0f32;
                    for q in 0..num_q {
                        let mut max_sim = -1e10f32;
                        for d in 0..num_d {
                            let mut sim = 0.0f32;
                            for i in 0..dim {
                                let q_val = q_tokens[q * dim + i];
                                let d_val = d_tokens[(doc_idx * num_d + d) * dim + i];
                                sim += q_val * d_val;
                            }
                            if sim > max_sim {
                                max_sim = sim;
                            }
                        }
                        doc_score += max_sim;
                    }
                    cpu_results[doc_idx] = doc_score;
                }

                for i in 0..num_docs {
                    assert!((result[i] - cpu_results[i]).abs() < 1e-4);
                }
            }
        });
    }
}
