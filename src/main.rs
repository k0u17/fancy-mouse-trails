mod platform;
mod trail;

use anyhow::Context;
use fxhash::{FxHashMap, FxHashSet};
use platform::Platform;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
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
    last_ticked: Option<Instant>
}

struct AppRenderer {
    windows: Vec<Arc<Window>>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    join_handle: Option<JoinHandle<()>>,
    sender: Option<mpsc::Sender<(WindowId, Option<Arc<(Mutex<bool>, Condvar)>>)>>,
}

const VALID_ALPHA_MODES: [wgpu::CompositeAlphaMode; 2] = [wgpu::CompositeAlphaMode::PreMultiplied, wgpu::CompositeAlphaMode::PostMultiplied];
impl AppRenderer {
    async fn start(instance: &wgpu::Instance, windows: Vec<Arc<Window>>) -> anyhow::Result<Self> {
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
        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default())
            .await.with_context(|| "Failed to create device")?;
        let overlays = surfaces.into_iter().zip(windows.iter())
            .map(|(surface, window)| {
                let caps = surface.get_capabilities(&adapter);
                let size = window.inner_size();
                let alpha_mode = VALID_ALPHA_MODES.into_iter().find(|mode| caps.alpha_modes.contains(mode)).unwrap();
                println!("Alpha mode for window {:?}: {:?}", window.id(), alpha_mode);
                Overlay::new(surface, wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: caps.formats[0],
                    color_space: wgpu::SurfaceColorSpace::Auto,
                    width: size.width,
                    height: size.height,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode,
                    view_formats: vec![],
                }, &device)
            })
            .collect::<Vec<Overlay>>();
        let (tx, rx) = mpsc::channel::<(WindowId, Option<Arc<(Mutex<bool>, Condvar)>>)>();
        let join_handle = {
            let device = device.clone();
            let queue = queue.clone();
            let overlays = FxHashMap::from_iter(
                windows.iter().map(|window| window.id()).zip(overlays.into_iter())
            );
            thread::spawn(move || {
                while let Ok((window_id, pair)) = rx.recv() {
                    let Some(overlay) = overlays.get(&window_id) else {
                        continue;
                    };
                    let result = overlay.render(&device, &queue);
                    if let Some(pair) = pair {
                        let (lock, cvar) = &*pair;
                        let mut rendered = lock.lock().unwrap();
                        *rendered = true;
                        cvar.notify_one();
                    }
                    result.unwrap();
                }
            })
        };
        Ok(AppRenderer { windows, adapter, device, queue, sender: Some(tx), join_handle: Some(join_handle) })
    }

    fn sender(&self) -> &mpsc::Sender<(WindowId, Option<Arc<(Mutex<bool>, Condvar)>>)> {
        self.sender.as_ref().unwrap()
    }

    fn enqueue_rendering(&self, window_id: WindowId) -> anyhow::Result<()> {
        self.sender().send((window_id, None)).with_context(|| "Render thread has exited unexpectedly")?;
        Ok(())
    }

    fn render_blocking(&self, window_id: WindowId) -> anyhow::Result<()> {
        let pair = Arc::new((Mutex::new(false), Condvar::new()));
        self.sender().send((window_id, Some(pair.clone()))).with_context(|| "Render thread has exited unexpectedly")?;
        let (lock, cvar) = &*pair;
        let mut rendered = lock.lock().ok().with_context(|| "Render thread has panicked")?;
        while !*rendered {
            rendered = cvar.wait(rendered).ok().with_context(|| "Render thread has panicked")?;
        }
        Ok(())
    }
}

impl Drop for AppRenderer {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            drop(sender);
        }
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        if let Err(e) = join_handle.join() {
            eprintln!("Error occurred while joining render thread: {:?}", e);
        }
    }
}

struct Overlay {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl Overlay {

    fn new(
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        device: &wgpu::Device,
    ) -> Self {
        surface.configure(&device, &config);
        Self { surface, config }
    }

    fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<()> {
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
                        a: 0.0
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
        let renderer = pollster::block_on(AppRenderer::start(&self.instance, windows))?;
        for window in &renderer.windows {
            renderer.render_blocking(window.id())?;
            window.set_visible(true);
        }
        self.renderer = Some(renderer);
        self.monitor_layout = layout;
        Ok(())
    }

    fn tick(&mut self) {

    }
}

const DT: Duration = Duration::from_micros(16_667);

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
                renderer.enqueue_rendering(window_id).unwrap();
                renderer.windows.iter()
                    .filter(|window| window.id() == window_id)
                    .for_each(|window| {
                        window.request_redraw();
                    });
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.platform.take_monitor_changed() {
            self.sync_windows(event_loop).unwrap();
        }
        let now = Instant::now();
        if let Some(last_ticked) = self.last_ticked {
            if now.duration_since(last_ticked) >= DT {
                self.last_ticked = Some(now);
                self.tick();
            }
        } else {
            self.last_ticked = Some(now);
            self.tick();
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
        last_ticked: None
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
