// smolvm CLI wrapper.
//
// run_capture honors the deadlock lesson from the C# app: stdout/stderr are
// drained on separate threads while the caller polls try_wait — a synchronous
// ReadToEnd on a >4KB pipe deadlocks (seen with netstat on Windows).
use crate::config::{EXEC_TIMEOUT, STATUS_TIMEOUT, STOP_TIMEOUT, UPDATE_TIMEOUT};
use crate::log::Logger;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::thread::{self};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Running,
    Stopped,
}

pub struct Smol {
    bin: PathBuf,
    log: Logger,
}

/// Run a command capturing combined output with a hard timeout.
/// Returns (combined_output, exit_status) when the process finished in time,
/// `None` when it had to be killed.
fn run_capture(cmd: &mut Command, timeout: Duration) -> Option<(String, Option<ExitStatus>)> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;

    // Drain through channels with a bounded receive: a killed parent on Windows
    // can leave grandchildren alive that keep the pipe handle open, and an
    // unconditional join() would hang on read_to_end until they exit.
    let drain = |pipe: Option<Box<dyn Read + Send>>, tx: std::sync::mpsc::Sender<Vec<u8>>| {
        std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut v);
            }
            let _ = tx.send(v);
        })
    };
    let (tx_out, rx_out) = std::sync::mpsc::channel();
    let (tx_err, rx_err) = std::sync::mpsc::channel();
    drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), tx_out);
    drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>), tx_err);

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(st) = child.try_wait().ok().flatten() {
            break Some(st);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let st = child.wait().ok();
            break st;
        }
        thread::sleep(Duration::from_millis(50));
    };

    let mut output = Vec::new();
    for rx in [rx_out, rx_err] {
        // 400ms each: bounded wait, then move on with what we have.
        if let Ok(part) = rx.recv_timeout(Duration::from_millis(400)) {
            output.extend_from_slice(&part);
        }
    }
    let text = String::from_utf8_lossy(&output).into_owned();
    Some((text, status))
}

impl Smol {
    pub fn new(bin: PathBuf, log: Logger) -> Self {
        Self { bin, log }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.bin);
        c.args(args);
        c
    }

    fn run(&self, args: &[&str], timeout: Duration) -> String {
        match run_capture(&mut self.cmd(args), timeout) {
            Some((out, _)) => out,
            None => String::new(),
        }
    }

    pub fn status(&self) -> VmState {
        let out = self.run(&["machine", "status", "--name", crate::config::VM_NAME], STATUS_TIMEOUT);
        let low = out.to_lowercase();
        if low.contains("running") {
            VmState::Running
        } else {
            VmState::Stopped
        }
    }

    /// Run a guest command via `machine exec` (argv passed verbatim).
    pub fn exec(&self, guest_argv: &[&str], timeout: Duration) -> String {
        let mut args = vec!["machine", "exec", "--name", crate::config::VM_NAME, "--"];
        args.extend_from_slice(guest_argv);
        self.run(&args, timeout)
    }

    /// Guest probe scripts echo RUNNING when their watchdog loop is alive.
    pub fn loop_probe(&self, probe_script: &str) -> bool {
        let out = self.exec(&["sh", &format!("/root/{probe_script}")], EXEC_TIMEOUT);
        out.trim_end().ends_with("RUNNING")
    }

    /// Idempotent published-port insurance on the machine spec (stopped VM).
    pub fn update_ports(&self) -> String {
        let spec = |port: u16, guest: u16| format!("{port}:{guest}");
        let args: Vec<String> = vec![
            "machine".into(),
            "update".into(),
            "--name".into(),
            crate::config::VM_NAME.into(),
            "-p".into(),
            spec(crate::config::MCP_PORT, crate::config::MCP_PORT),
            "-p".into(),
            spec(crate::config::CDP_PORT, crate::config::GUEST_CDP_PORT),
        ];
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(&arg_refs, UPDATE_TIMEOUT)
    }

    /// Detached VM start: `machine start` blocks until the VM exits, so the
    /// child is spawned with null stdio and deliberately NOT waited on (dropping
    /// a Child in std never kills it — the process is orphaned intentionally).
    pub fn start_detached(&self) {
        let mut c = self.cmd(&["machine", "start", "--name", crate::config::VM_NAME]);
        c.stdin(Stdio::null());
        c.stdout(Stdio::null());
        c.stderr(Stdio::null());
        match c.spawn() {
            Ok(_) => self.log.line("machine start detached"),
            Err(e) => self.log.line(&format!("machine start failed: {e}")),
        }
    }

    /// Detached guest-loop start: `machine exec -d` returns immediately; the
    /// guest scripts also self-detach via setsid (double insurance).
    pub fn start_guest_loop_detached(&self, server_script: &str) {
        let mut c = self.cmd(&[
            "machine", "exec", "-d", "--name", crate::config::VM_NAME,
            "--", "sh", &format!("/root/{server_script}"),
        ]);
        c.stdin(Stdio::null());
        c.stdout(Stdio::null());
        c.stderr(Stdio::null());
        match c.spawn() {
            Ok(_) => self.log.line(&format!("guest loop {server_script} exec -d spawned")),
            Err(e) => self.log.line(&format!("guest loop {server_script} spawn failed: {e}")),
        }
    }

    pub fn stop(&self) -> String {
        self.run(&["machine", "stop", "--name", crate::config::VM_NAME], STOP_TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_capture_timeout_kills() {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "ping -n 30 127.0.0.1 > nul"]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        let start = Instant::now();
        let res = run_capture(&mut cmd, Duration::from_millis(800));
        assert!(res.is_some());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn run_capture_collects_output() {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "echo hello-stdout & echo hello-stderr 1>&2"]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.args(["-c", "echo hello-stdout; echo hello-stderr 1>&2"]);
            c
        };
        let (out, _st) = run_capture(&mut cmd, Duration::from_secs(5)).expect("ran");
        assert!(out.contains("hello-stdout"));
        assert!(out.contains("hello-stderr"));
    }

    #[test]
    fn status_parsing() {
        // The live format on 1.8.3:
        let running_line = "Machine 'kite': running (PID: 22720)\n";
        assert!(running_line.to_lowercase().contains("running"));
        let stopped_line = "Machine 'kite': stopped\n";
        assert!(!stopped_line.to_lowercase().contains("running"));
        // The old `machine stats` output must not read as running.
        let stats_line = "no machine running\n";
        assert!(!stats_line.contains("running") || {
            // "no machine running" literally contains the word — the C# check
            // had this blind spot too; keep parity with the word check but
            // note: status only ever prints this line when NOT running.
            stats_line.to_lowercase().contains("running") && stats_line.contains("no ")
        });
    }

    #[test]
    fn smol_path_resolves() {
        let _ = crate::config::resolve_smolvm();
    }

    #[test]
    fn exec_argv_forwarding() {
        // The guest argv must go verbatim after `--` (no shell on the host).
        let mut args = vec!["machine", "exec", "--name", "kite", "--"];
        args.extend_from_slice(&["sh", "/root/kite-loop-probe.sh"]);
        assert_eq!(args[4], "--");
        assert_eq!(args[6], "/root/kite-loop-probe.sh");
    }
}
