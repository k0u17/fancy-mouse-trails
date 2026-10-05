use std::ffi::c_void;

use winit::dpi::PhysicalPosition;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: *const c_void) -> *const c_void;
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

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
        false // TODO: Implement monitor change detection for macOS
    }

    /// Returns the pointer's position in physical desktop coordinates.
    pub fn cursor_position(&self) -> anyhow::Result<PhysicalPosition<f64>> {
        let event = unsafe { CGEventCreate(std::ptr::null()) };
        if event.is_null() {
            anyhow::bail!("CGEventCreate failed");
        }
        let location = unsafe { CGEventGetLocation(event) };
        unsafe { CFRelease(event) };
        Ok(PhysicalPosition::new(location.x, location.y))
    }
}
