# v1.0 Feature-Parity Gap Analysis (sidebar vs SidebarDiagnostics)

**Generated:** 2026-07-17 (iteration 3 of v1.0 certification).
**Reference:** [ArcadeRenegade/SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics) v3.6.3 (latest stable).
**Purpose:** Document the feature gaps between the reference product and `sidebar`,
decide IN / DEFER for v1.0, and drive the action plan.

## How to read this

Each row is a reference-product feature. The `Status` column is one of:
- **PARITY** — sidebar matches the reference.
- **EXCEEDS** — sidebar does more than the reference (no action).
- **GAP-v1** — missing; **will implement for v1.0** (action item below).
- **DEFER-v1.1** — missing; documented, lands post-v1 (justification given).
- **N/A** — reference feature that does not apply (e.g. the reference lacks
  something sidebar ships).

## Summary counts

| Category | PARITY | EXCEEDS | GAP-v1 | DEFER-v1.1 | N/A |
|---|---|---|---|---|---|
| Hardware sensors | 11 | 3 | 4 | 0 | 0 |
| Display / layout | 6 | 1 | 5 | 0 | 0 |
| Theme / fonts | 3 | 0 | 4 | 0 | 0 |
| Behavior / window | 5 | 1 | 3 | 1 | 0 |
| Settings UI | 3 | 0 | 1 | 0 | 0 |
| Hotkeys | 1 | 0 | 0 | 1 | 0 |
| Data / history / alerts | 3 | 2 | 1 | 0 | 0 |
| **Totals** | **32** | **7** | **18** | **2** | **0** |

So v1.0 must close **18 gaps** to reach parity; 2 items are explicitly deferred.

## 1. Hardware sensors

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 1.1 | CPU total load | PARITY | sysinfo `System::cpus()` aggregate | — |
| 1.2 | CPU per-core load | PARITY | sysinfo per-core | — |
| 1.3 | CPU package temp (Full) | PARITY | LHM `/data.json` | — |
| 1.4 | CPU per-core clocks | PARITY | sysinfo + LHM | — |
| 1.5 | CPU power draw (Full) | PARITY | LHM | — |
| 1.6 | CPU fan speed (Full) | PARITY | LHM | — |
| 1.7 | Voltage rails (Full) | PARITY | LHM | — |
| 1.8 | CPU bus/BCLK clock | GAP-v1 | not surfaced | Add a CPU bus-clock metric from LHM `cpu/bus` node |
| 1.9 | `AllCoreClocks` toggle (all cores vs summary) | GAP-v1 | shows all cores always | Add per-monitor param to collapse to summary |
| 1.10 | GPU core clock/load/VRAM/temp (Full) | PARITY | LHM + nvml | — |
| 1.11 | GPU fan/voltage (Full) | PARITY | LHM | — |
| 1.12 | GPU multi-adapter enable/rename | GAP-v1 | single GPU only | Multi-GPU is rare on consumer hardware; **DEFER** unless flagged |
| 1.13 | RAM used/free/total + load | PARITY | sysinfo | — |
| 1.14 | RAM clock + voltage (Full) | GAP-v1 | not surfaced | Add from LHM `ram/` node |
| 1.15 | Battery | EXCEEDS | sidebar ships battery (reference does not) | — |
| 1.16 | Per-drive used/free/total | PARITY | sysinfo Disks | — |
| 1.17 | Per-drive read/write throughput | PARITY | PDH counters | — |
| 1.18 | Drive used-space alert | GAP-v1 | only temp alerts | Add UsedSpaceAlert threshold |
| 1.19 | SSD SMART health/temp (Full) | PARITY | LHM `physical_disk` | — |
| 1.20 | Per-process top-N CPU/RAM | EXCEEDS | sidebar ships top-N (reference does not) | — |
| 1.21 | Per-NIC live throughput | PARITY | GetIfTable2 | — |
| 1.22 | Per-NIC IPv4 address | PARITY (v1.0) | `sidebar-platform::net_info::ipv4_for_luid` via GetAdaptersAddresses; surfaced in bandwidth_panel `nic_row` next to each NIC's friendly name | Shipped in Epic 18 (Story 18.6 follow-on) |
| 1.23 | External/public IP | DEFER-v1.1 | — | Requires network egress (G16); reference uses ipify. Post-v1 behind opt-in. |
| 1.24 | Monthly bandwidth tracking per NIC | EXCEEDS | sidebar ships SQLite-backed monthly tracking (reference does not) | — |

## 2. Display / layout

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 2.1 | Dock edge L/R | PARITY | + T/B (exceeds) | — |
| 2.2 | Dock edge T/B | EXCEEDS | sidebar adds top/bottom | — |
| 2.3 | Screen/monitor selection | PARITY | DockConfig.monitor_id + picker (fixed in iter-3) | — |
| 2.4 | Reserve Space (AppBar) | PARITY | SHAppBarMessage wired, toggle via... | Expose as explicit toggle (currently always-on) |
| 2.5 | Horizontal/vertical offset | GAP-v1 | DockConfig.offset_px (1D only) | Add y-offset; rename to x/y |
| 2.6 | UI Scale (0.5–3.0) | GAP-v1 | not configurable | Add DisplayConfig.ui_scale (egui zoom) |
| 2.7 | Sidebar Width (100–300) | GAP-v1 | fixed 280px | Add DockConfig.sidebar_width |
| 2.8 | Drag-reposition | PARITY | implemented (Story 12.3) | — |
| 2.9 | Multi-monitor DPI-aware | PARITY | per-monitor DPI (Story 6.2) | — |

## 3. Theme / fonts

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 3.1 | Dark/Light/System theme | PARITY | ThemeConfig.mode | — |
| 3.2 | Accent color | PARITY | ThemeConfig.accent | — |
| 3.3 | Background color + opacity | GAP-v1 | theme-derived only | Add BGColor + BGOpacity |
| 3.4 | Font color + alert font color | GAP-v1 | theme-derived | Add font_color, alert_font_color |
| 3.5 | Font size presets (5) | GAP-v1 | fixed | Add DisplayConfig.font_size |
| 3.6 | Text align L/R | DEFER-v1.1 | centered | Cosmetic; post-v1 |
| 3.7 | Alert blink | GAP-v1 | color-only | Add alert blink animation |
| 3.8 | Auto background color (system accent) | DEFER-v1.1 | — | Post-v1; needs Windows accent API |

## 4. Behavior / window

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 4.1 | Always-on-top | PARITY | set_topmost | — |
| 4.2 | Click-through toggle | PARITY | hotkey + setting | — |
| 4.3 | Run at startup | GAP-v1 | not wired | Add scheduled-task / RunOnce registration + Settings toggle |
| 4.4 | Auto-update | DEFER-v1.1 | should_check always false (G16) | Post-v1; privacy review |
| 4.5 | Initially hidden | GAP-v1 | always visible | Add DisplayConfig.initially_hidden + tray toggle |
| 4.6 | Show tray icon | PARITY | tray wired | Verify menu items (Settings/Show/Hide/Close) |
| 4.7 | Pause polling when hidden | GAP-v1 | always polls | Skip poller tick when window hidden |
| 4.8 | Capture-cloak (hide from OBS) | EXCEEDS | WDA_EXCLUDEFROMCAPTURE (reference lacks) | — |
| 4.9 | Transparent borderless | PARITY | viewport transparent | — |

## 5. Settings UI

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 5.1 | Tabbed settings (5 tabs) | PARITY | single scrolling panel (acceptable for v1) | — |
| 5.2 | Save / Apply / Cancel | GAP-v1 | live-apply (no explicit Apply) | Acceptable for v1; defer Cancel |
| 5.3 | First-run wizard | PARITY | implemented (fixed in iter-2) | — |
| 5.4 | TOML/JSON config file | PARITY | config.toml | — |
| 5.5 | Auto-repair defaults | PARITY | Config::default fallback | — |

## 6. Hotkeys

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 6.1 | Toggle click-through | PARITY | implemented | — |
| 6.2 | Toggle/Show/Hide sidebar, CycleEdge/Screen, Reload, ReserveSpace, Close | PARITY (v1.0) | All 8 reference hotkeys shipped: HotkeyConfig ×8 fields + HotkeyKind enum + multi-registration thread + Settings "Hotkeys" section. Defaults unbound (user opts in via Settings) except click-through. | Shipped in Epic 18 Story 18.9 |

## 7. Data / history / alerts

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 7.1 | Per-metric sparkline | PARITY | RollingWindow | — |
| 7.2 | Per-metric graph popup | PARITY (v1.0) | egui::Window plotting MetricHistory; click 📈 next to any row; current/min/max + sample count; resizable | Shipped in Epic 18 Story 18.10 |
| 7.3 | Alert thresholds (temp/drive/bandwidth) | GAP-v1 | temp only (drive + bandwidth need adding) | Add drive-used + bandwidth alerts |
| 7.4 | Alert ack/snooze | EXCEEDS | sidebar ships ack/snooze (reference lacks) | — |
| 7.5 | Monthly bandwidth history | EXCEEDS | SQLite history (reference lacks) | — |
| 7.6 | CSV export | EXCEEDS | sidebar ships export (reference lacks) | — |

## 8. Integrations / packaging

| # | Reference feature | Status | sidebar current | Action |
|---|---|---|---|---|
| 8.1 | LibreHardwareMonitor (Full) | PARITY | bundled LHM HTTP bridge | — |
| 8.2 | NVIDIA NVML (Basic) | PARITY | nvml-wrapper | — |
| 8.3 | Windows PDH (disk/net) | PARITY | PDH + GetIfTable2 | — |
| 8.4 | Installer (setup.exe) | PARITY | Inno Setup (B1 fixed iter-2) | — |
| 8.5 | Portable ZIP | PARITY | release.yml ships both | — |
| 8.6 | Localization (48 langs) | PARITY (v1.0 infrastructure, 2 langs shipped) | `sidebar-app::i18n` — Label enum + Language (en + es) + t() lookup + Settings picker. Adding more languages is pure data. Reference ships 48; sidebar ships 2 + the extensible system. | Shipped in Epic 18 Story 18.11 |
| 8.7 | Run as admin optional | EXCEEDS | sidebar Basic mode needs no admin (reference requires admin) | — |

---

## v1.0 Action items (18 gaps to close)

Prioritized by user impact for non-technical users.

### Tier A — High user-impact (do first)
1. **2.7 Sidebar width** (100–300px slider) — users notice the fixed width immediately
2. **3.5 Font size** (presets) — readability is #1 for non-technical users
3. **3.7 Alert blink** — alerts need to be noticeable beyond color (accessibility)
4. **4.3 Run at startup** — core to "always-on sidebar" value proposition
5. **1.18 Drive used-space alert** — parity with temp alerts

### Tier B — Medium user-impact
6. **1.8 CPU bus clock** + **1.14 RAM clock/voltage** — sensor coverage parity
7. **1.22 Per-NIC IPv4 address** — useful for multi-NIC machines
8. **2.5 X/Y offset** — fine-grained positioning
9. **2.6 UI scale** — helps high-DPI users
10. **3.3 Background color/opacity** + **3.4 Font color** — visual customization
11. **4.5 Initially hidden** + **4.7 Pause-when-hidden** — power-user polish
12. **7.3 Bandwidth alert thresholds** — extends the marquee bandwidth feature

### Tier C — Lower user-impact
13. **1.9 AllCoreClocks toggle**
14. **1.12 Multi-GPU enable/rename**
15. **2.4 AppBar explicit toggle**
16. **5.2 Apply/Cancel buttons** (defer — live-apply is acceptable)

### Deferred to v1.1 (documented)
- 1.23 External IP (egress)
- 3.6 Text align, 3.8 Auto BG color
- 4.4 Auto-update

---

## Cited
- Reference README: https://github.com/ArcadeRenegade/SidebarDiagnostics
- Reference Settings.cs / Monitoring.cs / Settings.xaml (source of truth for params)
- sidebar `docs/PRD.md` §3 (In-Scope), §4 (Out-of-Scope)
- sidebar `docs/customization-parity.md` (existing audit)
- sidebar `crates/sidebar-domain/src/config.rs` (current Config surface)
