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
| Capture-cloak (hide from OBS) | IN | DisplayConfig.hide_from_capture |
| Threshold config (warn/crit) | IN | ThresholdConfig |
| Metric enable/reorder | IN | MetricsConfig.order |
| Per-metric alert ack/snooze | IN (12.6) | AlertAck enum + hysteresis-preserving GUI actions |
| Clock/date header | IN (12.1) | format_clock, header render |
| Per-metric history graph | IN (12.2) | MetricHistory map; poller push + per-row sparkline render |
| Drag-reposition | CORE (12.3) | compute_new_offset; WM_NCHITTEST follow-up |
| Battery health | DEFERRED (12.5) | Needs supported Windows source |
| Adapter name/IP metadata | DEFERRED (12.5) | Display-only alongside LUID |
| Localization (labels/formats) | DEFERRED (12.7) | v1 locale-stable (OQ-5) |
| Layout presets | DEFERRED | Audit each preset vs NFR-4 before shipping |
| Custom metric presets | DEFERRED | Beyond MetricsConfig.order scope |
| Plugin/scripting support | OUT | PRD §4 explicitly excludes |
| Cloud sync | OUT | PRD §4 explicitly excludes |
| Auto-update check | DEFERRED (9.3) | v1.1; default OFF (G16) |

## NFR-1/NFR-4 guardrail

Every IN item is pure logic or a thin Win32 call already proven under
NFR-1 (≤0.5% CPU per source) + NFR-4 (≤80 MiB Basic RSS). DEFERRED items
require additional NFR evidence before promotion to IN.

## Cited
- Story 12.4 DoD, PRD §4 (out-of-scope), §5 (Tier-1..4 features),
  nfr-thresholds.md NFR-1/NFR-4, guardrails.md G8 (CostClass gate).
