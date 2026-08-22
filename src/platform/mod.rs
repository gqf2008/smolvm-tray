//! Per-platform adapters: single-instance, autostart, open-url/dir, log dir.

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod posix;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use posix::SingleInstance;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

/// True when this instance owns the single-instance slot; false when another
/// instance is already running (caller should exit immediately).
pub fn acquire_single_instance(log: &crate::log::Logger) -> Option<SingleInstance> {
    SingleInstance::acquire(log)
}
