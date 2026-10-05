use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, WM_DISPLAYCHANGE};
use winit::dpi::PhysicalPosition;
use winit::event_loop::ActiveEventLoop;
use winit::platform::windows::WindowAttributesExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

const OBSERVER_SUBCLASS_ID: usize = 1;

pub struct Platform {
    changed: Arc<AtomicBool>,
    observer: Option<Window>,
    registration: Option<(HWND, *const AtomicBool)>,
}

impl Platform {
    pub fn new() -> Self {
        Self {
            changed: Arc::new(AtomicBool::new(false)),
            observer: None,
            registration: None,
        }
    }

    pub fn configure_instance(&self, descriptor: &mut wgpu::InstanceDescriptor) {
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.backend_options.dx12.presentation_system =
            wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }

    pub fn resumed(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        if self.observer.is_some() {
            return Ok(());
        }

        let observer = event_loop.create_window(
            WindowAttributes::default()
                .with_visible(false)
                .with_skip_taskbar(true),
        )?;
        let hwnd = window_hwnd(&observer)?;
        // The callback owns an Arc reference until its subclass is removed.
        let signal = Arc::into_raw(Arc::clone(&self.changed));
        let installed = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(display_change_proc),
                OBSERVER_SUBCLASS_ID,
                signal as usize,
            )
        };
        if installed == 0 {
            unsafe { drop(Arc::from_raw(signal)) };
            anyhow::bail!("failed to observe display changes");
        }

        self.registration = Some((hwnd, signal));
        self.observer = Some(observer);
        Ok(())
    }

    pub fn window_attributes(&self, attributes: WindowAttributes) -> WindowAttributes {
        attributes.with_skip_taskbar(true)
    }

    pub fn window_created(&self, window: &Window) -> anyhow::Result<()> {
        let disabled: i32 = 1; // Win32 BOOL TRUE
        let result = unsafe {
            DwmSetWindowAttribute(
                window_hwnd(window)?,
                DWMWA_TRANSITIONS_FORCEDISABLED as u32,
                (&disabled as *const i32).cast(),
                size_of_val(&disabled) as u32,
            )
        };
        if result < 0 {
            anyhow::bail!("DwmSetWindowAttribute failed: {result:#x}");
        }
        Ok(())
    }

    pub fn take_monitor_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// Returns the pointer's position in physical desktop coordinates.
    pub fn cursor_position(&self) -> anyhow::Result<PhysicalPosition<f64>> {
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0 {
            anyhow::bail!("GetCursorPos failed");
        }
        Ok(PhysicalPosition::new(point.x as f64, point.y as f64))
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        if let Some((hwnd, signal)) = self.registration.take() {
            let removed = unsafe {
                RemoveWindowSubclass(hwnd, Some(display_change_proc), OBSERVER_SUBCLASS_ID)
            };
            // Only release the callback's Arc once its subclass is gone.
            if removed != 0 {
                unsafe { drop(Arc::from_raw(signal)) };
            }
        }
    }
}

fn window_hwnd(window: &Window) -> anyhow::Result<HWND> {
    let handle = window.window_handle()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        unreachable!("expected a Win32 window");
    };
    Ok(handle.hwnd.get() as HWND)
}

unsafe extern "system" fn display_change_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_DISPLAYCHANGE {
        let signal = unsafe { &*(data as *const AtomicBool) };
        signal.store(true, Ordering::Release);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
