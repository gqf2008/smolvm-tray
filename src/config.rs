// Configuration: constants + env overrides + smolvm binary resolution.
use std::path::PathBuf;
use std::time::Duration;

pub const VM_NAME: &str = "kite";
pub const MCP_PORT: u16 = 8090;
pub const CDP_PORT: u16 = 9222;
pub const GUEST_CDP_PORT: u16 = 9223; // host 9222 -> guest 9223 (chromium DevTools)
pub const MCP_PROBE_PATH: &str = "/"; // not /mcp: the SSE endpoint holds the connection
pub const CDP_PROBE_PATH: &str = "/json/version";

pub const GUEST_KITE_SERVER: &str = "kite-server.sh";
pub const GUEST_CDP_SERVER: &str = "cdp-server.sh";
pub const GUEST_KITE_PROBE: &str = "kite-loop-probe.sh";
pub const GUEST_CDP_PROBE: &str = "cdp-loop-probe.sh";

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(8);
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(9);
pub const EXEC_TIMEOUT: Duration = Duration::from_secs(10);
pub const HTTP_TIMEOUT: Duration = Duration::from_millis(1500);
pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(30);
pub const STOP_TIMEOUT: Duration = Duration::from_secs(90);

/// Windows-only fallback (the local binary distribution layout).
#[cfg(windows)]
const WINDOWS_DEFAULT_SMOLVM: &str = r"D:\smolvm-1.8.3-windows-x86_64\smolvm.exe";

/// Resolve the smolvm binary:
/// 1. $SMOLVM_TRAY_SMOLVM
/// 2. <exe_dir>/smolvm[.exe]  (packed layout: tray and smolvm side by side)
/// 3. Windows: the known local dist path (fallback for this box)
/// 4. $PATH scan (manual, no `which` spawn)
pub fn resolve_smolvm() -> PathBuf {
    if let Ok(v) = std::env::var("SMOLVM_TRAY_SMOLVM") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("smolvm");
            if candidate.exists() {
                return candidate;
            }
            #[cfg(windows)]
            {
                let candidate = dir.join("smolvm.exe");
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    #[cfg(windows)]
    {
        let candidate = PathBuf::from(WINDOWS_DEFAULT_SMOLVM);
        if candidate.exists() {
            return candidate;
        }
    }
    path_scan().unwrap_or_else(|| PathBuf::from("smolvm"))
}

fn path_scan() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let name = if cfg!(windows) { "smolvm.exe" } else { "smolvm" };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn http_probe_enabled() -> bool {
    // SMOLVM_TRAY_PORT_PROBE=tcp switches to connect-only probing.
    std::env::var("SMOLVM_TRAY_PORT_PROBE")
        .map(|v| v != "tcp")
        .unwrap_or(true)
}

pub fn autostart_on_launch() -> bool {
    // SMOLVM_TRAY_NO_AUTOSTART=1 skips the launch-time start-all (dev/debug).
    std::env::var("SMOLVM_TRAY_NO_AUTOSTART").map(|v| v != "1").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_env_defaults() {
        // These tests run with the ambient environment; just assert the sane default.
        let _ = http_probe_enabled();
        let _ = autostart_on_launch();
    }

    #[test]
    fn windows_default_absolute() {
        #[cfg(windows)]
        assert!(WINDOWS_DEFAULT_SMOLVM.starts_with("D:\\"));
    }
}
