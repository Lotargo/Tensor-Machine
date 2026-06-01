use wgpu::{Device, Queue, Instance, RequestAdapterOptions};
use std::sync::Arc;

pub struct GpuContext {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
}

pub const SHADER_SRC: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let index = global_id.x;
    if (index < arrayLength(&a)) {
        result[index] = a[index] + b[index];
    }
}
"#;

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

        Some(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
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

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging_buffer"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(a));
        self.queue.write_buffer(&buffer_b, 0, bytemuck::cast_slice(b));

        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Add Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let compute_pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Add Pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let bind_group_layout = compute_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Add Bind Group"),
            layout: &bind_group_layout,
            entries: &[
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
            let workgroup_count = (a.len() as u32 + 63) / 64;
            cpass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        encoder.copy_buffer_to_buffer(&buffer_result, 0, &staging_buffer, 0, size);
        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);

        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
            sender.send(v).unwrap();
        });

        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().ok()?.ok()?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        staging_buffer.unmap();

        Some(result)
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
            } else {
                println!("Skipping GPU test: no compatible GPU found.");
            }
        });
    }
}
