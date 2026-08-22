// The heart: one worker thread owns ALL probing and actions, so probes can
// never pile up (the C# busyOps/refreshing guards are unnecessary by design).
// The UI thread only forwards tray callbacks as non-blocking messages; UI
// updates flow the other way through the UiSink trait (implemented in main.rs
// via slint::invoke_from_event_loop + Weak<AppTray>).
use crate::config;
use crate::log::Logger;
use crate::net::{http_probe, tcp_probe};
use crate::smolvm::{Smol, VmState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Health {
    pub vm: bool,
    pub mcp_port: bool,
    pub mcp_loop: bool,
    pub cdp_port: bool,
    pub cdp_loop: bool,
}

impl Health {
    pub const TOTAL: usize = 5;
    pub fn up(&self) -> usize {
        [self.vm, self.mcp_port, self.mcp_loop, self.cdp_port, self.cdp_loop]
            .iter()
            .filter(|b| **b)
            .count()
    }
    pub fn level(&self) -> TrayLevel {
        let up = self.up();
        if up == Self::TOTAL {
            TrayLevel::Green
        } else if up == 0 {
            TrayLevel::Red
        } else {
            TrayLevel::Yellow
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayLevel {
    Green,
    Yellow,
    Red,
}

impl TrayLevel {
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            TrayLevel::Green => crate::icon::GREEN,
            TrayLevel::Yellow => crate::icon::YELLOW,
            TrayLevel::Red => crate::icon::RED,
        }
    }
}

/// Everything the UI needs for one refresh, sent as one bundle.
#[derive(Debug, Clone)]
pub struct UiState {
    pub health: Health,
    pub tooltip: String,
    pub autostart_checked: bool,
    /// A start/stop operation in flight — the menu switch is disabled then.
    pub phase: ServicePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePhase {
    Idle,
    Starting,
    Stopping,
}

/// UI side of the boundary — decouples the state machine from Slint so the
/// tray backend could be swapped (e.g. the tray-icon crate) without touching
/// the core. Implemented in main.rs.
pub trait UiSink: Send {
    fn update(&self, state: &UiState);
    fn exit(&self);
}

#[derive(Debug, Clone, Copy)]
pub enum UserAction {
    /// One switch in the menu: start when stopped, stop when running.
    ToggleService,
    OpenCdp,
    OpenMcp,
    RestartChromium,
    OpenLogs,
    ToggleAutostart,
    Exit,
    /// Internal: launch-time bootstrap (start_all) — not from the menu.
    #[doc(hidden)]
    InitialStart,
}

pub enum WorkerMsg {
    Act(UserAction),
}

pub struct Worker {
    rx: Receiver<WorkerMsg>,
    cancel: Arc<AtomicBool>,
    smol: Smol,
    ui: Box<dyn UiSink>,
    log: Logger,
    last: Option<UiState>,
    /// Read once at startup and after each toggle — never re-spawn reg.exe
    /// every refresh (console flash + needless spawn).
    autostart_cached: bool,
    /// A start/stop operation in flight: the switch is disabled and repeat
    /// toggle messages are dropped.
    phase: ServicePhase,
}

impl Worker {
    /// Spawn the worker thread; returns the sender side. The thread's first act
    /// when `autostart_on_launch` is StartAll (unless disabled by env).
    pub fn spawn(
        smol: Smol,
        ui: Box<dyn UiSink>,
        log: Logger,
        autostart_on_launch: bool,
    ) -> Sender<WorkerMsg> {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let autostart_cached = crate::platform::autostart_enabled();
        let worker = Worker { rx, cancel, smol, ui, log, last: None, autostart_cached, phase: ServicePhase::Idle };
        std::thread::Builder::new()
            .name("smolvm-tray-worker".into())
            .spawn(move || worker.run())
            .expect("spawn worker thread");
        if autostart_on_launch {
            let _ = tx.send(WorkerMsg::Act(UserAction::InitialStart));
        }
        tx
    }

    fn run(mut self) {
        let mut last_refresh = Instant::now() - config::REFRESH_INTERVAL;
        loop {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(WorkerMsg::Act(action)) => {
                    // One operation at a time: while a start/stop is in flight
                    // the menu switch is disabled, and a queued toggle is
                    // dropped rather than re-run (the steps are idempotent but
                    // re-probing wastes seconds).
                    if matches!(action, UserAction::ToggleService) && self.phase != ServicePhase::Idle {
                        self.log.line("toggle dropped: start/stop already in progress");
                        continue;
                    }
                    self.log.line(&format!("action: {action:?} (cancelling refresh)"));
                    self.cancel.store(true, Ordering::SeqCst);
                    self.handle(action);
                    self.cancel.store(false, Ordering::SeqCst);
                    if matches!(action, UserAction::Exit) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if last_refresh.elapsed() >= config::REFRESH_INTERVAL {
                        if !self.cancel.load(Ordering::SeqCst) {
                            self.refresh();
                        }
                        last_refresh = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn push(&self, state: &UiState) {
        self.ui.update(state);
    }

    /// One full state probe: VM status -> host port probes (only when the VM is
    /// up) -> guest loop probes (only when the VM is up).
    fn refresh(&mut self) {
        let vm = self.smol.status() == VmState::Running;
        let http = config::http_probe_enabled();
        let (mcp_port, cdp_port) = if vm {
            (
                if http {
                    http_probe(config::MCP_PORT, config::MCP_PROBE_PATH, config::HTTP_TIMEOUT)
                } else {
                    tcp_probe(config::MCP_PORT, config::HTTP_TIMEOUT)
                },
                if http {
                    http_probe(config::CDP_PORT, config::CDP_PROBE_PATH, config::HTTP_TIMEOUT)
                } else {
                    tcp_probe(config::CDP_PORT, config::HTTP_TIMEOUT)
                },
            )
        } else {
            (false, false)
        };
        let (mcp_loop, cdp_loop) = if vm {
            (
                self.smol.loop_probe(config::GUEST_KITE_PROBE),
                self.smol.loop_probe(config::GUEST_CDP_PROBE),
            )
        } else {
            (false, false)
        };
        let health = Health { vm, mcp_port, mcp_loop, cdp_port, cdp_loop };
        let state = UiState {
            health,
            tooltip: format!("smolvm 服务台 · {}/{} 在线", health.up(), Health::TOTAL),
            autostart_checked: self.autostart_cached,
            phase: self.phase,
        };
        self.last = Some(state.clone());
        self.push(&state);
        self.log.line(&format!(
            "refresh vm={} mcp_port={} mcp_loop={} cdp_port={} cdp_loop={}",
            health.vm, health.mcp_port, health.mcp_loop, health.cdp_port, health.cdp_loop
        ));
    }

    fn handle(&mut self, action: UserAction) {
        match action {
            UserAction::ToggleService => {
                if self.smol.status() == VmState::Running {
                    self.stop_all();
                } else {
                    self.start_all();
                }
            }
            UserAction::InitialStart => self.start_all(),
            UserAction::OpenCdp => {
                crate::platform::open_url(&format!("http://127.0.0.1:{}", config::CDP_PORT));
            }
            UserAction::OpenMcp => {
                crate::platform::open_url(&format!(
                    "http://127.0.0.1:{}/mcp",
                    config::MCP_PORT
                ));
            }
            UserAction::RestartChromium => {
                if self.smol.status() != VmState::Running {
                    self.log.line("restart-chromium skipped: VM not running");
                    return;
                }
                let out = self.smol.exec(
                    &[
                        "sh",
                        "-c",
                        "kill -9 $(cat /tmp/.cdp-chromium.pid 2>/dev/null) 2>/dev/null; echo killed",
                    ],
                    Duration::from_secs(30),
                );
                self.log.line(&format!("cdp chromium restart: {}", out.trim()));
            }
            UserAction::OpenLogs => {
                crate::platform::open_dir(self.log.path().parent().unwrap_or(std::path::Path::new(".")));
            }
            UserAction::ToggleAutostart => {
                let on = !self.autostart_cached;
                crate::platform::set_autostart(on);
                self.autostart_cached = on;
                self.log.line(&format!("autostart -> {on}"));
                let mut state = self.last.clone().unwrap_or_else(|| self.current_ui_buffer());
                state.autostart_checked = on;
                self.push(&state);
            }
            UserAction::Exit => {
                self.log.line("exit requested — services left as-is");
                self.ui.exit();
            }
        }
    }

    fn current_ui_buffer(&self) -> UiState {
        // Cheap re-probe of just the display bits; the menu rows are stale-safe
        // because the next scheduled refresh repaints them.
        let vm = self.smol.status() == VmState::Running;
        UiState {
            health: Health { vm, mcp_port: false, mcp_loop: false, cdp_port: false, cdp_loop: false },
            tooltip: "smolvm 服务台".into(),
            autostart_checked: self.autostart_cached,
            phase: self.phase,
        }
    }

    fn start_all(&mut self) {
        // Switch disabled + label "启动中…" for the whole op.
        self.set_phase(ServicePhase::Starting);
        self.ensure_machine();
        self.start_vm();
        self.start_guest_loop(config::GUEST_CDP_SERVER, config::GUEST_CDP_PROBE);
        self.start_guest_loop(config::GUEST_KITE_SERVER, config::GUEST_KITE_PROBE);
        self.log.line("start-all finished");
        self.set_phase(ServicePhase::Idle);
        self.refresh();
    }

    /// First run on a fresh host: the machine does not exist yet — create it
    /// from the bundled `.smolmachine` (exe-dir or $SMOLVM_TRAY_PACK). Without
    /// a bundle, fail fast with a clear message instead of waiting 120 s.
    fn ensure_machine(&mut self) {
        if self.smol.machine_exists() {
            return;
        }
        let Some(pack) = config::pack_file() else {
            self.log.line("machine 'kite' missing and no kite.smolvmachine bundle found — manual machine create required");
            let mut state = self.last.clone().unwrap_or_else(|| self.current_ui_buffer());
            state.tooltip = "缺机器：请放 kite.smolvmachine 在托盘旁或设 SMOLVM_TRAY_PACK".into();
            self.push(&state);
            return;
        };
        self.log.line(&format!("machine missing — initializing from {}", pack.display()));
        let out = self.smol.create_from_pack(&pack);
        self.log.line(&format!("machine create --from: {}", out.trim()));
        if !self.smol.machine_exists() {
            self.log.line("WARN: create --from did not produce the machine");
        }
    }

    fn stop_all(&mut self) {
        self.set_phase(ServicePhase::Stopping);
        self.log.line("stopping all");
        let out = self.smol.stop();
        self.log.line(&format!("machine stop: {}", out.trim()));
        std::thread::sleep(Duration::from_secs(2));
        self.set_phase(ServicePhase::Idle);
        self.refresh();
    }

    /// Broadcast a phase-only update (menu switch label/enabled) using the
    /// last known health — no probing, instant feedback.
    fn set_phase(&mut self, phase: ServicePhase) {
        self.phase = phase;
        let mut state = self.last.clone().unwrap_or_else(|| UiState {
            health: Health { vm: false, mcp_port: false, mcp_loop: false, cdp_port: false, cdp_loop: false },
            tooltip: "smolvm 服务台".into(),
            autostart_checked: self.autostart_cached,
            phase,
        });
        state.phase = phase;
        self.last = Some(state.clone());
        self.log.line(&format!("phase -> {phase:?}"));
        self.push(&state);
    }

    /// VM up (idempotent) with `-p` insurance and a 120s boot poll, then both
    /// guest loops with probe guards.
    fn start_vm(&self) {
        if self.smol.status() == VmState::Running {
            self.log.line("vm already running");
            return;
        }
        let upd = self.smol.update_ports();
        self.log.line(&format!("machine update ports: {}", upd.trim()));
        self.smol.start_detached();
        for i in 0..40 {
            std::thread::sleep(Duration::from_secs(3));
            if self.smol.status() == VmState::Running {
                self.log.line("vm is running");
                return;
            }
            if i % 10 == 9 {
                self.log.line(&format!("waiting for vm boot ({}/120s)", (i + 1) * 3));
            }
        }
        self.log.line("WARN: vm not running after 120s");
    }

    fn start_guest_loop(&self, server: &str, probe: &str) {
        if self.smol.loop_probe(probe) {
            self.log.line(&format!("{probe} already RUNNING"));
            return;
        }
        self.log.line(&format!("starting guest loop {server}"));
        self.smol.start_guest_loop_detached(server);
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(1500));
            if self.smol.loop_probe(probe) {
                self.log.line(&format!("{probe} RUNNING"));
                return;
            }
        }
        self.log.line(&format!("WARN: {probe} still NONE after 30s"));
    }
}
