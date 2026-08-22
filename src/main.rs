// smolvm-tray v4 — Rust + Slint cross-platform tray for the kite VM.
// Hosts kitewright MCP (8090) + CDP chromium (9222) via smolvm published-TCP
// forwarding. One codebase for Windows / macOS / Linux (Slint 1.17.1).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod config;
mod icon;
mod log;
mod net;
mod platform;
mod smolvm;
mod state;

use log::Logger;
use slint::Weak;
use state::{UiSink, UiState, UserAction, WorkerMsg};
use std::sync::mpsc::Sender;

struct UiSinkImpl {
    weak: Weak<AppTray>,
}

fn push_to_ui(weak: Weak<AppTray>, f: impl FnOnce(&AppTray) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(tray) = weak.upgrade() {
            f(&tray);
        }
    });
}

impl UiSink for UiSinkImpl {
    fn update(&self, state: &UiState) {
        let weak = self.weak.clone();
        let health = state.health;
        let level = health.level();
        let tooltip = state.tooltip.clone();
        let autostart = state.autostart_checked;
        let phase = state.phase;
        push_to_ui(weak, move |t| {
            t.set_tray_icon(icon::render_icon(level.rgb()));
            t.set_tray_tooltip(tooltip.into());
            t.set_list_kite_vm(marked(health.vm, "Kite VM", "运行中", "已停止").into());
            t.set_list_mcp_port(marked(health.mcp_port, "MCP 入站端口 (8090)", "监听中", "未运行").into());
            t.set_list_mcp_loop(marked(health.mcp_loop, "MCP 循环", "运行中", "未运行").into());
            t.set_list_cdp_port(marked(health.cdp_port, "CDP 入站端口 (9222)", "监听中", "未运行").into());
            t.set_list_cdp_loop(marked(health.cdp_loop, "CDP 循环", "运行中", "未运行").into());
            t.set_can_open_cdp(health.cdp_port);
            t.set_can_open_mcp(health.mcp_port);
            t.set_can_restart_chromium(health.vm);
            t.set_autostart_checked(autostart);
            // One switch, never two concepts — and never clickable while a
            // start/stop is in flight.
            let (label, enabled) = match phase {
                state::ServicePhase::Starting => ("⏳ 启动中…（请勿重复点击）", false),
                state::ServicePhase::Stopping => ("⏳ 停止中…", false),
                state::ServicePhase::Idle => {
                    if health.vm {
                        ("⏹ 停止服务", true)
                    } else {
                        ("▶ 启动服务（确保在线）", true)
                    }
                }
            };
            t.set_start_stop_label(label.into());
            t.set_start_stop_enabled(enabled);
        });
    }

    fn exit(&self) {
        let _ = slint::quit_event_loop();
    }
}

fn marked(up: bool, label: &str, up_text: &str, down_text: &str) -> String {
    let (dot, text) = if up { ("● ", up_text) } else { ("○ ", down_text) };
    format!("{dot}{label} — {text}")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log = Logger::new(platform::log_dir().join("smolvm-tray.log"));

    let _single = match platform::acquire_single_instance(&log) {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let smolvm_bin = config::resolve_smolvm();
    log.line(&format!("tray v4 started (rust+slint) smolvm={}", smolvm_bin.display()));

    // Initial UI: red icon until the first refresh lands.
    let tray = AppTray::new()?;
    tray.set_tray_icon(icon::render_icon(state::TrayLevel::Red.rgb()));
    tray.set_tray_tooltip("smolvm 服务台 · 正在启动…".into());
    tray.set_autostart_checked(platform::autostart_enabled());

    let ui: Box<dyn UiSink> = Box::new(UiSinkImpl { weak: tray.as_weak() });
    let smol = smolvm::Smol::new(smolvm_bin, log.clone());
    let tx = state::Worker::spawn(smol, ui, log.clone(), config::autostart_on_launch());

    let act = |tx: &Sender<WorkerMsg>, a: UserAction| {
        let _ = tx.send(WorkerMsg::Act(a));
    };
    tray.on_action_toggle({ let tx = tx.clone(); move || act(&tx, UserAction::ToggleService) });
    tray.on_action_open_cdp({ let tx = tx.clone(); move || act(&tx, UserAction::OpenCdp) });
    tray.on_action_open_mcp({ let tx = tx.clone(); move || act(&tx, UserAction::OpenMcp) });
    tray.on_action_restart_chromium({ let tx = tx.clone(); move || act(&tx, UserAction::RestartChromium) });
    tray.on_action_open_logs({ let tx = tx.clone(); move || act(&tx, UserAction::OpenLogs) });
    tray.on_action_toggle_autostart({ let tx = tx.clone(); move || act(&tx, UserAction::ToggleAutostart) });
    tray.on_action_exit({ let tx = tx.clone(); move || act(&tx, UserAction::Exit) });
    // Left-click opens the CDP console (Windows/Linux; macOS pops the menu).
    tray.on_tray_clicked({ let tx = tx.clone(); move || act(&tx, UserAction::OpenCdp) });

    tray.show()?;
    slint::run_event_loop()?;
    log.line("event loop exited");
    Ok(())
}
