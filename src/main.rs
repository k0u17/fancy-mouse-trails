mod platform;

use anyhow::Context;
use fxhash::FxHashSet;
use platform::Platform;
use std::sync::Arc;
use wgpu::DeviceDescriptor;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

struct App {
    instance: wgpu::Instance,
    renderer: Option<AppRenderer>,
    platform: Platform,
    monitor_layout: Vec<(MonitorHandle, PhysicalPosition<i32>, PhysicalSize<u32>)>,
}

struct AppRenderer {
    overlays: Vec<Overlay>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

const VALID_ALPHA_MODES: [wgpu::CompositeAlphaMode; 2] = [wgpu::CompositeAlphaMode::PreMultiplied, wgpu::CompositeAlphaMode::PostMultiplied];
impl AppRenderer {
    async fn new(instance: &wgpu::Instance, windows: Vec<Arc<Window>>) -> anyhow::Result<Self> {
        let surfaces = windows.iter()
            .map(|window| instance.create_surface(window.clone()).unwrap())
            .collect::<Vec<_>>();
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surfaces.get(0),
            ..Default::default()
        }).await.with_context(|| "Failed to find a suitable GPU adapter")?;
        println!("Selected GPU adapter: {:?}", adapter.get_info());
        for surface in surfaces.iter().skip(1) {
            let caps = surface.get_capabilities(&adapter);
            println!("Surface capabilities: {:?}", caps);
            if VALID_ALPHA_MODES
                .into_iter()
                .all(|mode| !caps.alpha_modes.contains(&mode)) {
                anyhow::bail!("Transparent surface not supported");
            }
            if caps.formats.is_empty() {
                anyhow::bail!("Multiple surfaces with different formats are not supported");
            }
        }
        let (device, queue) = adapter.request_device(&DeviceDescriptor::default())
            .await.with_context(|| "Failed to create device")?;
        let overlays = surfaces.into_iter().zip(windows.into_iter())
            .map(|(surface, window)| {
                let caps = surface.get_capabilities(&adapter);
                let size = window.inner_size();
                Overlay::new(window, surface, wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: caps.formats[0],
                    color_space: wgpu::SurfaceColorSpace::Auto,
                    width: size.width,
                    height: size.height,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 0,
                    alpha_mode: VALID_ALPHA_MODES.into_iter().find(|mode| caps.alpha_modes.contains(mode)).unwrap(),
                    view_formats: vec![],
                }, &device)
            })
            .collect();
        Ok(AppRenderer { overlays, adapter, device, queue })
    }
}

struct Overlay {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Overlay {

    fn new(
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        device: &wgpu::Device,
    ) -> Self {
        surface.configure(&device, &config);
        Self { window, surface, config }
    }

    fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<()> {
        self.window.request_redraw();
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                return Ok(());
            },
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&device, &self.config);
                return Ok(());
            },
            wgpu::CurrentSurfaceTexture::Lost => {
                anyhow::bail!("Lost device");
            }
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder")
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: wgpu::StoreOp::Store
                }
            })],
            ..Default::default()
        });
        queue.submit(std::iter::once(encoder.finish()));
        queue.present(output);
        Ok(())
    }

    fn matches(&self, window_id: WindowId) -> bool {
        self.window.id() == window_id
    }
}

impl App {
    fn sync_windows(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let mut positions = FxHashSet::default();
        let mut monitors: Vec<_> = event_loop.available_monitors()
            .filter(|monitor| positions.insert(monitor.position()))
            .collect();
        monitors.sort_by_key(|monitor| (monitor.position().x, monitor.position().y));

        let layout: Vec<_> = monitors.iter()
            .map(|monitor| (monitor.clone(), monitor.position(), monitor.size()))
            .collect();
        if layout == self.monitor_layout {
            return Ok(());
        }
        self.renderer = None;
        let windows = monitors.iter().map(|monitor| {
            let attributes = WindowAttributes::default()
                .with_title("Trail Overlay")
                .with_decorations(false)
                .with_transparent(true)
                .with_visible(false)
                .with_active(false)
                .with_position(monitor.position())
                .with_inner_size(monitor.size())
                .with_window_level(WindowLevel::AlwaysOnTop);

            let window = event_loop.create_window(self.platform.window_attributes(attributes))?;
            self.platform.window_created(&window)?;
            window.set_cursor_hittest(false)?;
            Ok(Arc::new(window))
        }).collect::<anyhow::Result<_>>()?;
        let renderer = pollster::block_on(AppRenderer::new(&self.instance, windows))?;
        for overlay in &renderer.overlays {
            overlay.render(&renderer.device, &renderer.queue)?;
            overlay.window.set_visible(true);
        }
        self.renderer = Some(renderer);
        self.monitor_layout = layout;
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.platform.resumed(event_loop).unwrap();
        self.sync_windows(event_loop).unwrap();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                println!("Window {:?} has received the signal to close; closing...", window_id);
                event_loop.exit();
            },
            WindowEvent::RedrawRequested => {
                let Some(renderer) = self.renderer.as_ref() else { return; };
                let Some(overlay) = renderer.overlays.iter()
                    .find(|overlay| overlay.matches(window_id)) else { return; };
                overlay.render(&renderer.device, &renderer.queue).unwrap();
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.platform.take_monitor_changed() {
            self.sync_windows(event_loop).unwrap();
        }
    }
}

fn main() -> Result<(), anyhow::Error> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();

    let platform = Platform::new();
    platform.configure_instance(&mut desc);

    let mut app = App {
        instance: wgpu::Instance::new(desc),
        renderer: None,
        platform,
        monitor_layout: Vec::new(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
