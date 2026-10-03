#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod imp;

#[cfg(not(target_os = "windows"))]
#[path = "fallback.rs"]
mod imp;

pub use imp::Platform;
