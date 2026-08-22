// Windows platform adapter.
//
// The mutex name stays "smolvm-tray-v2" — the same name the retired C# tray
// used — so the Rust and C# binaries cannot both appear in the tray during the
// migration window. Rename to "smolvm-tray" only after the C# exe is gone.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const MUTEX_NAME: &str = "smolvm-tray-v2";
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "smolvm-tray";

pub struct SingleInstance {
    _handle: HANDLE, // must stay alive for the process lifetime
}

impl SingleInstance {
    pub fn acquire(log: &crate::log::Logger) -> Option<Self> {
        let wide: Vec<u16> = MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: pointer to a null-terminated wide buffer valid during the call.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            log.line("CreateMutexW failed — proceeding without single-instance guard");
            return Some(Self { _handle: handle });
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            log.line("another instance holds smolvm-tray-v2 — exiting");
            // Release our reference to the existing mutex before exiting.
            unsafe { CloseHandle(handle) };
            return None;
        }
        Some(Self { _handle: handle })
    }
}

pub fn log_dir() -> PathBuf {
    std::env::var_os("TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// reg.exe is a console app too — without CREATE_NO_WINDOW every autostart
/// check (8s refresh) flashes a black console from this GUI parent.
fn reg_args(args: &[&str]) -> Command {
    let mut c = Command::new("reg");
    c.args(args);
    c.creation_flags(CREATE_NO_WINDOW);
    c.stdout(Stdio::null());
    c.stderr(Stdio::null());
    c
}

pub fn autostart_enabled() -> bool {
    reg_args(&["query", RUN_KEY, "/v", RUN_VALUE])
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

pub fn set_autostart(on: bool) {
    if on {
        if let Some(exe) = std::env::current_exe().ok() {
            let mut c = reg_args(&["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d"]);
            c.arg(&exe);
            c.arg("/f");
            let _ = c.status();
        }
    } else {
        let _ = reg_args(&["delete", RUN_KEY, "/v", RUN_VALUE, "/f"]).status();
    }
}

pub fn open_url(url: &str) {
    let mut c = Command::new("explorer.exe");
    c.arg(url);
    let _ = c.spawn();
}

pub fn open_dir(dir: &std::path::Path) {
    let mut c = Command::new("explorer.exe");
    c.arg(dir);
    let _ = c.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_exe_is_absolute() {
        let exe = std::env::current_exe().unwrap();
        assert!(exe.is_absolute());
    }
}
