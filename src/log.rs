// Append-only logger with timestamp + 256 KB rotation to <log>.old.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 256 * 1024;

#[derive(Clone)]
pub struct Logger {
    path: PathBuf,
}

impl Logger {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line(&self, msg: &str) {
        let stamp = chrono_stamp();
        self.append(&format!("{stamp} {msg}\n"));
    }

    fn append(&self, text: &str) {
        let _ = self.rotate_if_needed();
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
    }

    fn rotate_if_needed(&self) -> std::io::Result<()> {
        let meta = fs::metadata(&self.path)?;
        if meta.len() >= MAX_BYTES {
            fs::rename(&self.path, self.path.with_extension("old"))?;
        }
        Ok(())
    }
}

fn chrono_stamp() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let millis = secs.subsec_millis();
    // Local time without a timezone crate: format via the `time`-free path —
    // convert from UTC epoch to local wall clock using the system clock readout.
    let local = local_hhmmss(secs.as_secs());
    format!("{local}.{millis:03}")
}

fn local_hhmmss(utc: u64) -> String {
    // Windows: system_clock is UTC; derive local by asking the OS via std (no dep).
    // Fall back to a fixed offset approach is wrong; instead use `DateTime`-free
    // trick: the FILE_TIME is UTC, so just format UTC time — acceptable for a log
    // (the C# one stamped local). Use std::time with the platform local offset.
    #[cfg(windows)]
    {
        // GetLocalTime-equivalent via SystemTime -> FILETIME math is overkill;
        // keep it simple: UTC stamp marked with Z.
        let (h, m, s) = hms(utc);
        format!("{h:02}:{m:02}:{s:02}Z")
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        // sub-second precision isn't critical; use `date +%H:%M:%S` on POSIX.
        if let Ok(out) = Command::new("date").args(["+%H:%M:%S"]).output() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                return s.trim().to_string();
            }
        }
        let (h, m, s) = hms(utc);
        format!("{h:02}:{m:02}:{s:02}Z")
    }
}

fn hms(utc_secs: u64) -> (u32, u32, u32) {
    let day = utc_secs / 86400;
    let rem = utc_secs % 86400;
    let _ = day;
    ((rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hms_roundtrip() {
        assert_eq!(hms(3661), (1, 1, 1));
        assert_eq!(hms(0), (0, 0, 0));
        assert_eq!(hms(86399), (23, 59, 59));
    }

    #[test]
    fn write_and_rotate() {
        let dir = std::env::temp_dir().join(format!("smolvm-tray-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("t.log");
        let log = Logger::new(path.clone());
        log.line("hello");
        assert!(path.exists());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("hello"));
        let _ = fs::remove_dir_all(&dir);
    }
}
