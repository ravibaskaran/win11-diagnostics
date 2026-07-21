# Changelog

All notable changes to sidebar are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-07-21

First public release. Beta signal — the public API + on-disk layout are stable
enough for everyday use, but code-signing + winget distribution land in a
follow-up release.

### Added

- **Live hardware telemetry sidebar** for Windows 11: CPU/GPU temperatures,
  clocks, utilization, fan speeds, voltages, power draw; memory and VRAM;
  per-drive storage and throughput; per-network-adapter throughput.
- **Monthly bandwidth tracking** per network interface — user-configurable
  billing-cycle start day, persistent across restarts (SQLite-backed), auto-
  rollover with bounded history retention.
- **Two-tier sensor model**: Basic mode (no admin, via sysinfo + PDH + NVML
  + GetIfTable2 + battery) degrades gracefully to Full mode (UAC, via
  LibreHardwareMonitor subprocess).
- **Settings panel**: temperature unit, raw/decimal display, theme,
  per-NIC + drive-used-space alert thresholds, hotkeys, monitor + edge
  selection, font/UI scale, background opacity, language.
- **First-run wizard** with monitor + billing-cycle + theme picker.
- **Inno Setup installer** (`sidebar-setup.exe`) — Start Menu + optional
  desktop shortcuts, Add/Remove Programs entry, in-place upgrades.
- **Portable ZIP** edition for users who can't/won't install.
- **Release pipeline** (GitHub Actions): build → ISCC → portable ZIP →
  draft GitHub Release with SHA-256 checksums in the body.
- **Atomic config writes** (`sidebar_platform::fs::atomic_write`) for both
  the app's TOML config and the bundled LHM user config.
- **Graceful degradation**: persistent `archive_cycle` failure surfaces a
  GUI banner instead of silently freezing the billing cycle.

### Changed

- **Threshold sliders** for CPU/GPU temperature now constrain their range
  dynamically (warn's max = critical − 5; critical's min = warn + 5) — the
  invalid position is unreachable instead of silently snapping after release.
- **Dock controls** (edge, monitor, offset X/Y) now re-dock the window live
  instead of requiring a restart.
- **Unavailable-sensor rendering**: shows `--` (same sentinel as NaN) instead
  of the raw string `unknown` for unrecognized MetricKind × Unit pairs.
- **`last_day_of_month`** rewritten to use chrono's calendar (the prior
  12-arm match had a defensive `_ => 30` arm that silently swallowed invalid
  months).
- **MetricHistory** now evicts stale windows for sensors that disappear
  (hot-plug NIC unplug, USB drive unmount) instead of growing unbounded.
- **Refresh-rate slider** + **billing-cycle-day** tooltip now say
  "Applies on next launch" / "Restart sidebar" so users know a restart is
  required.

### Fixed

- **Persistent `archive_cycle` failure** no longer silently freezes the
  billing cycle — bounded deferral counter, escalated log, and a GUI banner.
- **Slider silent-snap** — dragging warn above critical−5 used to silently
  snap back with zero feedback.
- **Non-atomic LHM config write** — `patch_lhm_user_config` used
  `std::fs::write` which truncates-then-streams; a crash mid-write could
  corrupt the third-party `LibreHardwareMonitor.config`. Now uses
  temp+rename atomic write.
- **Wizard `on_exit` persist error** — was silently swallowed, leaving
  `first_run_complete=true` in memory only, causing the wizard to reappear
  on every launch on locked-down machines.
- **`#[allow(dead_code)]`** removed from `MockTargets` — the dead struct
  fields are gone.
- **Tautological tests removed** (constant-vs-literal assertions, identity
  wrappers, import-silencer tests).
- **Workspace clippy clean** with `-D warnings`, **647 tests passing**.

### Known limitations

- **Unsigned.** v0.1.0 ships unsigned. SignPath Foundation application
  pending; signing lands in a future release. Windows SmartScreen will warn
  on first install — see the [install instructions](README.md#install).
- **winget not yet available.** The winget manifest is staged in
  `installer/winget/manifest.yaml` but submission is deferred until a signed,
  stable release exists (winget PackageIdentifier is permanent once
  published).
- **Windows Service not registered.** The installer bundles
  `sidebar-monitor-svc.exe` + `sidebar-monitor-host.exe` but does not register
  or start the service. The app uses the HTTP-to-LHM path which works without
  the service; the binaries are bundled so they're ready when the named-pipe
  consumer lands.
- **No NVIDIA GPU required.** The reference dev environment has integrated
  AMD graphics and no NVIDIA. NVML integration tests are `#[ignore]`'d in
  CI; AMD GPU coverage is via Full mode (LibreHardwareMonitor) only.
- **Per-process refresh.** The sysinfo adapter refreshes the full process
  table on every poll (NFR-1 budget risk); default config has no process
  metrics enabled so the impact today is bounded, but the adapter trait
  needs config-awareness before v1.0.

[0.1.0]: https://github.com/ravibaskaran/win11-diagnostics/releases/tag/v0.1.0
