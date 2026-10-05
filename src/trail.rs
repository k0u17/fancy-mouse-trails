use std::collections::VecDeque;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalPosition;
use crate::AppRenderer;

trait Trail {

    fn update(&mut self);

    fn render(&self, pass: &mut wgpu::RenderPass);
}

struct Ribbon {
    buffer: wgpu::Buffer,
    capacity: usize,
    trajectory: VecDeque<SplinePoint>,
    trajectory_timestamps: VecDeque<Instant>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SplinePoint {
    control_first: [f64; 2],
    position: [f64; 2],
    control_second: [f64; 2]
}

const EXPIRATION_DURATION: Duration = Duration::from_secs(1);

impl Ribbon {

    fn append(&mut self, renderer: &AppRenderer, pos_x: f64, pos_y: f64) -> anyhow::Result<()> {
        let now = Instant::now();
        self.trajectory.push_back(SplinePoint {
            control_first: [pos_x, pos_y],
            position: [pos_x, pos_y],
            control_second: [pos_x, pos_y]
        });
        self.trajectory_timestamps.push_back(now);
        while let Some(timestamp) = self.trajectory_timestamps.front() {
            if now.duration_since(*timestamp) > EXPIRATION_DURATION {
                self.trajectory.pop_front();
                self.trajectory_timestamps.pop_front();
            } else {
                break;
            }
        }
        if self.trajectory.len() > self.capacity {
            self.capacity *= 2;
            self.buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Ribbon Buffer"),
                size: (self.capacity * size_of::<SplinePoint>()) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            });
            let mut view = self.buffer.slice(..).get_mapped_range_mut()?;
            view.copy_from_slice(
                bytemuck::cast_slice(&self.trajectory.make_contiguous())
            );
        } else {
            let (left, right) = self.trajectory.as_slices();
            renderer.queue.write_buffer(
                &self.buffer,
                0,
                bytemuck::cast_slice(left)
            );
            if !right.is_empty() {
                renderer.queue.write_buffer(
                    &self.buffer,
                    (left.len() * size_of::<SplinePoint>()) as u64,
                    bytemuck::cast_slice(right)
                );
            }
        }
        Ok(())
    }
}
