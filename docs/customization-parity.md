# Customization Parity Audit — sidebar-v1 (Story 12.4)

Per the original SidebarDiagnostics app's customization surface, this audit
identifies which options are IN v1 scope (low-cost, NFR-1/NFR-4 safe) and
which are OUT (plugins, scripting, high-RSS layout engines). Story 12.4 is
doc-marked "deferred" — this audit is the v1 deliverable; the full
implementation of deferred items lands post-v1.

## Legend
- **IN** — shipped in v1 (Epic 0–8 + 12.1/12.2/12.3/12.6 cores).
- **DEFERRED** — documented, low-priority, lands post-v1.
- **OUT** — explicitly out of scope (NFR-1/NFR-4 risk or plugin/scripting).

## Customization audit

| Original-app option | v1 status | Notes |
|---|---|---|
| Poll interval (1–60s) | IN | T-3, Config.poll_interval_seconds |
| Temperature unit (C/F) | IN | T-29, DisplayConfig.temp_unit |
| Byte base (decimal/binary) | IN | T-28, DisplayConfig.decimal_base |
| Raw-values toggle | IN | NFR-8, DisplayConfig.raw_values |
| Top-N processes (1–50) | IN | T-21, ProcessConfig.top_n |
| Sparkline window (10–600) | IN | T-22, GraphConfig.window |
| Theme mode (Dark/Light/System) | IN | T-35, ThemeConfig.mode |
| Accent color | IN | T-35, ThemeConfig.accent (#RRGGBB) |
| Dock edge (L/R/T/B) | IN | DockConfig.edge |
| Monitor selection | IN | T-36, DockConfig.monitor_id |
| Billing-cycle start day | IN | T-26, BandwidthConfig.cycle_start_day |
| Hotkey (toggle click-through) | IN | T-34, HotkeyConfig.click_through |
| Extended hotkeys (toggle/show/hide/cycle-edge/cycle-screen/reload/toggle-reserve/close) | IN (v1.0 parity) | HotkeyConfig ×8 fields, HotkeyKind enum + multi-registration thread, settings "Hotkeys" section |
| Capture-cloak (hide from OBS) | IN | DisplayConfig.hide_from_capture |
| Threshold config (warn/crit) | IN | ThresholdConfig |
| Metric enable/reorder | IN | MetricsConfig.order |
| Per-metric alert ack/snooze | IN (12.6) | AlertAck enum + hysteresis-preserving GUI actions |
| Clock/date header | IN (12.1) | format_clock, header render |
| Per-metric history graph | IN (12.2 + v1.0 parity popup) | MetricHistory map; per-row sparkline + click-to-open line-graph popup (egui::Window plotting the metric's rolling history with current/min/max labels) |
| Drag-reposition | CORE (12.3) | compute_new_offset; WM_NCHITTEST follow-up |
| Sidebar width (100–300px) | IN (v1.0 parity) | DockConfig.width_px, settings slider |
| Font size (10–22px via zoom) | IN (v1.0 parity) | DisplayConfig.font_size, egui zoom_factor |
| UI scale (50–300%) | IN (v1.0 parity) | DisplayConfig.ui_scale_percent, composes with font zoom |
| Alert blink (accessibility) | IN (v1.0 parity) | DisplayConfig.alert_blink, ~1Hz color toggle on Critical |
| Custom background color + opacity | IN (v1.0 parity) | DisplayConfig.bg_color + bg_opacity_percent, global_style_mut |
| Custom font color | IN (v1.0 parity) | DisplayConfig.font_color, override_text_color |
| X/Y position offsets | IN (v1.0 parity) | DockConfig.offset_x_px + offset_y_px, send_dock_position |
| Run at Windows startup | IN (v1.0 parity) | DisplayConfig.run_at_startup, HKCU Run key (no admin) |
| Initially hidden | IN (v1.0 parity) | DisplayConfig.initially_hidden, ViewportCommand::Visible |
| Pause sensors when hidden | IN (v1.0 parity) | DisplayConfig.pause_when_hidden, drain_broadcast_only |
| Drive used-space alert | IN (v1.0 parity) | ThresholdConfig.drive_used_warn (% threshold) |
| Bandwidth in/out alerts (Mbps) | IN (v1.0 parity) | ThresholdConfig.bandwidth_{in,out}_alert_mbps |
| CPU bus clock (BCLK) | IN (v1.0 parity) | MetricKind::CpuBusClock, LHM clock sensor |
| RAM clock + voltage | IN (v1.0 parity) | MetricKind::RamClock + RamVoltage, LHM ram hardware |
| Battery health | DEFERRED (12.5) | Needs supported Windows source |
| Per-NIC IPv4 address | IN (v1.0 parity) | sidebar-platform::net_info::ipv4_for_luid (GetAdaptersAddresses), shown in bandwidth_panel nic_row |
| Localization (labels/formats) | IN (v1.0 parity) | sidebar-app::i18n — Label enum + Language (en default + es shipped) + t() lookup; Settings "Language" picker; per-variant exhaustive match enforces coverage |
| Layout presets | DEFERRED | Audit each preset vs NFR-4 before shipping |
| Custom metric presets | DEFERRED | Beyond MetricsConfig.order scope |
| External/public IP | DEFERRED | Requires network egress (G16); reference uses ipify |
| Extended hotkeys (toggle/show/hide/cycle) | IN (v1.0 parity) | 8 hotkeys shipped: click-through + toggle/show/hide/cycle-edge/cycle-screen/reload/toggle-reserve/close |
| Text align L/R | DEFERRED | Cosmetic; post-v1 |
| Auto background color (system accent) | DEFERRED | Needs Windows accent API; post-v1 |
| Auto-update check | DEFERRED (9.3) | v1.1; default OFF (G16) |
| Plugin/scripting support | OUT | PRD §4 explicitly excludes |
| Cloud sync | OUT | PRD §4 explicitly excludes |

## NFR-1/NFR-4 guardrail

Every IN item is pure logic or a thin Win32 call already proven under
NFR-1 (≤0.5% CPU per source) + NFR-4 (≤80 MiB Basic RSS). DEFERRED items
require additional NFR evidence before promotion to IN.

## Cited
- Story 12.4 DoD, PRD §4 (out-of-scope), §5 (Tier-1..4 features),
  nfr-thresholds.md NFR-1/NFR-4, guardrails.md G8 (CostClass gate).
