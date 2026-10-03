use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

pub struct Platform;

impl Platform {
    pub fn new() -> Self {
        Self
    }

    pub fn configure_instance(&self, _descriptor: &mut wgpu::InstanceDescriptor) {}

    pub fn resumed(&mut self, _event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn window_attributes(&self, attributes: WindowAttributes) -> WindowAttributes {
        attributes
    }

    pub fn window_created(&self, _window: &Window) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn take_monitor_changed(&self) -> bool {
        false
    }
}
