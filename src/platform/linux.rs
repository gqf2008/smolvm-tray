// Linux platform adapter: XDG autostart .desktop, xdg-open for URLs/dirs.
// Single instance lives in the shared posix module.
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn log_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(dir).join("smolvm-tray");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local").join("state").join("smolvm-tray");
    }
    std::env::temp_dir()
}

pub fn autostart_enabled() -> bool {
    autostart_path().exists()
}

pub fn set_autostart(on: bool) {
    let path = autostart_path();
    if on {
        if let Some(exe) = std::env::current_exe().ok() {
            let escaped = exe.to_string_lossy().replace('"', "\\\"");
            let desktop = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=smolvm-tray\n\
                 Comment=smolvm kite VM tray\n\
                 Exec=\"{escaped}\"\n\
                 Terminal=false\n\
                 X-GNOME-Autostart-enabled=true\n"
            );
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, desktop);
        }
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

fn autostart_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("autostart")
            .join("smolvm-tray.desktop");
    }
    std::env::temp_dir().join("smolvm-tray.desktop")
}

pub fn open_url(url: &str) {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    let _ = c.spawn();
}

pub fn open_dir(dir: &std::path::Path) {
    let mut c = Command::new("xdg-open");
    c.arg(dir);
    let _ = c.spawn();
}
