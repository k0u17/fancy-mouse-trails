use crate::AppRenderer;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::vec2d::Vec2d;

pub trait Trail {
    fn update(&mut self, renderer: &AppRenderer);

    fn render(&self, pass: &mut wgpu::RenderPass);
}

struct TrailPoint {
    pos: Vec2d,
    vertices: usize, // the number of vertices in the discretized curve this point accounts for
    expiration: Instant
}

impl TrailPoint {
    fn new(pos: Vec2d, vertices: usize) -> Self {
        Self {
            pos,
            vertices,
            expiration: Instant::now() + RIBBON_DURATION
        }
    }
}

pub struct Ribbon {
    buffer: wgpu::Buffer,
    capacity: usize,
    trajectory: VecDeque<TrailPoint>,
    // the last appended point, which there's no curve segment for yet,
    // because catmull-rom requires the next point to compute the curve
    pending: Option<Vec2d>,
    // whether to linearly extrapolate the first control point for a catmull-rom curve segment
    // instead of using the third previous point.
    should_extrapolate: bool,
    curve: VecDeque<[f64; 2]>
}


const RIBBON_DURATION: Duration = Duration::from_secs(1);
const EPSILON: f64 = 1e-6;

impl Ribbon {

    // pub fn append(&mut self, renderer: &AppRenderer, pos_x: f64, pos_y: f64) -> anyhow::Result<()> {
    //     let now = Instant::now();
    //     self.trajectory.push_back([pos_x, pos_y]);
    //     self.trajectory_timestamps.push_back(now);
    //     while let Some(timestamp) = self.trajectory_timestamps.front() {
    //         if now.duration_since(*timestamp) > RIBBON_DURATION {
    //             self.trajectory.pop_front();
    //             self.trajectory_timestamps.pop_front();
    //         } else {
    //             break;
    //         }
    //     }
    //     if self.trajectory.len() > self.capacity {
    //         self.capacity *= 2;
    //         self.buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
    //             label: Some("Ribbon Buffer"),
    //             size: (self.capacity * size_of::<[f64; 2]>()) as u64,
    //             usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
    //             mapped_at_creation: true,
    //         });
    //         let mut view = self.buffer.slice(..).get_mapped_range_mut()?;
    //         view.copy_from_slice(
    //             bytemuck::cast_slice(&self.trajectory.make_contiguous())
    //         );
    //     } else {
    //         let (left, right) = self.trajectory.as_slices();
    //         renderer.queue.write_buffer(
    //             &self.buffer,
    //             0,
    //             bytemuck::cast_slice(left)
    //         );
    //         if !right.is_empty() {
    //             renderer.queue.write_buffer(
    //                 &self.buffer,
    //                 (left.len() * size_of::<[f64; 2]>()) as u64,
    //                 bytemuck::cast_slice(right)
    //             );
    //         }
    //     }
    //     Ok(())
    // }


    fn add_point(&mut self, pos: Vec2d) {
        match self.pending {
            None => {
                if self.trajectory.is_empty() {
                    self.trajectory.push_back(TrailPoint::new(pos, 0));
                } else {
                    self.pending = Some(pos);
                    self.should_extrapolate = true;
                }
            },
            Some(pending) => {
                if (pos - pending).length_squared() < EPSILON {
                    return;
                }
                if let Some(TrailPoint { pos: prev, .. }) = self.trajectory.back() {
                    let first_point = if self.should_extrapolate || self.trajectory.len() < 2 {
                        self.should_extrapolate = false;
                        2.0 * *prev - pending
                    } else {
                        self.trajectory.get(self.trajectory.len() - 2).unwrap().pos
                    };
                } else {
                    self.trajectory.push_back(TrailPoint::new(pending, 0));
                    self.pending = Some(pos);
                    self.should_extrapolate = true;
                }
            }
        }
    }
}
