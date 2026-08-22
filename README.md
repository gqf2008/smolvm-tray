# smolvm-tray

Cross-platform system-tray controller for a local **smolvm** microVM running
the "kite" workload — a kitewright MCP server (8090) and a CDP chromium console
(9222), both reached through smolvm's `-p HOST:GUEST` published-TCP forwarder
(inbound arrives at the guest NIC address; the guest services bind accordingly).

Built with **Rust + Slint 1.17.1** (`SystemTrayIcon` element, added in 1.17.0).
One codebase for Windows / macOS / Linux — CI builds release binaries for all
three (see [.github/workflows/build.yml](.github/workflows/build.yml)).

## Features

- Color-coded tray icon (green 5/5 · yellow 1–4 · red 0) + live menu rows,
  refreshed every 8 s:
  - Kite VM running (`smolvm machine status`)
  - MCP inbound port 8090 answering (HTTP probe)
  - MCP guest watchdog loop RUNNING (guest probe script)
  - CDP inbound port 9222 answering (HTTP probe)
  - CDP guest watchdog loop RUNNING
- Actions: 一键启动全部 (idempotent, with `-p` spec insurance + boot poll),
  一键停止全部 (`machine stop` — VM death clears services/loops),
  open CDP console / MCP endpoint, 重启 CDP 浏览器 (guest kill, watchdog
  respawns ≤ 3 s), 打开日志目录, 开机自动启动 toggle, 退出（不停止服务）
  (the detached `machine start` child keeps the VM alive).
- Single instance: Windows named mutex (`smolvm-tray-v2`, shared with the
  retired C# tray so both never coexist); macOS/Linux lockfile + `kill -0`
  stale recovery.
- Autostart: HKCU `Run` (Windows) / `~/Library/LaunchAgents` LaunchAgent plist
  (macOS) / `~/.config/autostart` .desktop (Linux).
- App log with rotation: `%TEMP%\smolvm-tray.log` /
  `~/Library/Logs/smolvm-tray.log` / `$XDG_STATE_HOME`(fallback `~/.local/state`)
  per platform.

## Configuration

| Env var | Meaning |
|---|---|
| `SMOLVM_TRAY_SMOLVM` | Path to the smolvm binary (default: `exe_dir/smolvm`, then the Windows dist path, then `$PATH`) |
| `SMOLVM_TRAY_NO_AUTOSTART=1` | Skip the launch-time start-all (debug) |
| `SMOLVM_TRAY_PORT_PROBE=tcp` | Switch port checks from HTTP to connect-only |

## Build

```sh
cargo build --release
```

Windows binary: `target/release/smolvm-tray.exe` (no console window in
release; Slint + winit native backend — no WebView2/signing needed).
macOS: build on a Mac (`cargo build --release --target aarch64-apple-darwin`);
Linux: `cargo build --release` (build deps: libxkbcommon, libfontconfig).

CI (`.github/workflows/build.yml`) builds and tests all three platforms on
push and uploads artifacts.

## Platform notes

- Linux tray = StatusNotifierItem over D-Bus (pure Rust `ksni`/zbus). Needs a
  desktop with an SNI host: KDE yes; GNOME requires the AppIndicator extension
  (`gnome-shell-extension-appindicator`); plain X11 without a host will not
  show the icon.
- macOS: left-click with an attached menu opens the menu instead of firing
  the click callback (AppKit behavior) — the CDP console opens from the menu
  entry instead.
- The guest setup (kite VM, `/root/kite-server.sh` + `/root/cdp-server.sh`)
  is common to all hosts; only this launcher is platform-specific. VM name and
  ports are constants in `src/config.rs`.
