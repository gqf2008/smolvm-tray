//! Shared POSIX (macOS + Linux) single-instance guard: `create_new` lockfile
//! with a stale check via `kill -0`. Atomic like a mutex; the sentinel PID
//! makes crash leftovers (whose process is gone) recoverable.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct SingleInstance {
    path: PathBuf,
}

impl SingleInstance {
    pub fn acquire(log: &crate::log::Logger) -> Option<Self> {
        let dir = lock_dir();
        let path = dir.join(LOCK_NAME);
        match acquire_lockfile(&path) {
            Ok(()) => Some(Self { path }),
            Err(reason) => {
                log.line(&format!("single instance: {reason}"));
                None
            }
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// macOS stores temp files in $TMPDIR; Linux prefers the per-user runtime dir
/// (tmpfs, cleaned at logout) with /tmp as fallback.
fn lock_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        std::env::var_os("TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    } else {
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
    }
}

const LOCK_NAME: &str = "smolvm-tray.lock";

fn acquire_lockfile(path: &Path) -> Result<(), String> {
    let pid = format!("{}\n", std::process::id());
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            use std::io::Write;
            let _ = f.write_all(pid.as_bytes());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::read_to_string(path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(|pid| !pid_alive(pid))
                .unwrap_or(true);
            if stale {
                let _ = std::fs::remove_file(path);
                return acquire_lockfile(path);
            }
            Err("another instance is running (lockfile held)".into())
        }
        Err(e) => Err(format!("lockfile error: {e}")),
    }
}

fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|st| st.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_pid_is_replaced() {
        let dir = std::env::temp_dir().join(format!("smolvm-tray-locktest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.lock");
        // 4 billion — can't be a live process
        let _ = std::fs::write(&path, b"99999999\n");
        assert!(acquire_lockfile(&path).is_ok());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_pid_is_rejected() {
        let dir = std::env::temp_dir().join(format!("smolvm-tray-locktest2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t2.lock");
        let own = format!("{}\n", std::process::id());
        let _ = std::fs::write(&path, own);
        assert!(acquire_lockfile(&path).is_err());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
