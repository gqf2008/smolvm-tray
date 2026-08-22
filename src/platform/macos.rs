// macOS platform adapter: LaunchAgent autostart, `open` for URLs/dirs.
// Single instance lives in the shared posix module.
use std::path::PathBuf;
use std::process::{Command, Stdio};

const PLIST_PATH: &str = "Library/LaunchAgents/com.smolvm.tray.plist";
const PLIST_LABEL: &str = "com.smolvm.tray";

pub fn log_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library").join("Logs")
}

pub fn autostart_enabled() -> bool {
    autostart_path().exists()
}

pub fn set_autostart(on: bool) {
    let path = autostart_path();
    if on {
        if let Some(exe) = std::env::current_exe().ok() {
            let escaped = exe
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n\
                 <dict>\n\
                 \t<key>Label</key><string>{label}</string>\n\
                 \t<key>ProgramArguments</key>\n\
                 \t<array>\n\
                 \t\t<string>{exe}</string>\n\
                 \t</array>\n\
                 \t<key>RunAtLoad</key><true/>\n\
                 \t<key>ProcessType</key><string>Interactive</string>\n\
                 </dict>\n\
                 </plist>\n",
                label = PLIST_LABEL,
                exe = escaped,
            );
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            if std::fs::write(&path, plist).is_ok() {
                // Activate for the current session; failure is logged not fatal.
                let _ = Command::new("launchctl")
                    .args(["load", "-w"])
                    .arg(&path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    } else {
        let _ = Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_file(&path);
    }
}

fn autostart_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(PLIST_PATH)
}

pub fn open_url(url: &str) {
    let mut c = Command::new("open");
    c.arg(url);
    let _ = c.spawn();
}

pub fn open_dir(dir: &std::path::Path) {
    let mut c = Command::new("open");
    c.arg(dir);
    let _ = c.spawn();
}
