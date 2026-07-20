# PRD — sidebar (sidebar-v1)

**Change:** `sidebar-v1`
**Phase:** implementation snapshot (Epic 0–8 delivered; closure work pending)
**Status:** Requirements remain authoritative; current implementation gaps are listed in §12
**Date:** 2026-07-07 (v2 amendment, same date)

> **What changed in v2 (this amendment).** This is an UPDATE pass on an existing PRD; all original v1 content is preserved and extended. Four amendments are integrated below:
> 1. **Network adapter throughput is now IN scope** (was Out-of-Scope in v1). See §3 (Tier 4) and §7.
> 2. **Monthly bandwidth consumption tracking** (NEW marquee feature) — per-NIC, user-configurable billing-cycle start date, persistent across restarts, auto-rollover. See §3 (Tier 4) and new §5.5.
> 3. **NFR-8 (NEW) — Human-readable output by default.** See §6.
> 4. **Distribution format OQ-1 is now RESOLVED** with a zero-cost-first recommendation (SignPath + GitHub Releases + winget + Microsoft Store free onboarding). See §9 OQ-1.

> **Honest framing up front.** This product is **Rust-native except for CPU package temperature and a small set of low-level hardware sensors**, which require a bundled LibreHardwareMonitor (LHM) subprocess exposing the local HTTP `/data.json` endpoint. We do not claim "pure Rust" anywhere in this proposal or the accompanying architecture. The LHM bundling is a deliberate, research-validated design decision, not a fallback.

---

## 1. Product Vision

**sidebar** is a lightweight, always-on, transparent Windows 11 desktop sidebar that surfaces calm — not live — hardware telemetry: CPU/GPU temperatures, clocks, utilization, fan speeds, voltages, power draw; memory and VRAM; per-drive storage and throughput; per-network-adapter throughput; per-process top-N resource consumers; battery state; and **monthly bandwidth consumption tracking per network interface**. It is a ground-up Rust clone of the user-facing experience of [SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics) (C#/.NET/WPF + LibreHardwareMonitor), rebuilt natively for Windows 11 with a strict lightweight mandate and a two-tier sensor model that degrades gracefully when elevated privileges are unavailable.

**Tagline:** *"Glanceable system health, calmly."*

**The defining constraint.** Windows exposes no stable, documented, user-mode Rust-callable API for CPU package temperature, core clocks, fan speeds, or motherboard voltages. The `sysinfo` crate returns no CPU temperature readings on Windows. The current integration runs [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) as a sidecar and reads its local HTTP `GET /data.json` endpoint on loopback. LHM v0.9.5+ no longer publishes the historical WMI namespace.

**Implementation correction (2026-07-11):** LHM v0.9.6 no longer exposes the
WMI namespace described in the historical research sentence above. The shipped
bridge is HTTP-only (`/data.json` on literal loopback), with redirect suppression
and a 500 ms timeout.

**What makes this different from the C# original.**
1. **Rust memory safety** in the host process (GUI, polling, config, persistence).
2. **Strict lightweight mandate** (NFR-1) with a sensor-cost classifier applied at design time — every telemetry source must be defensible as lightweight or it is deferred.
3. **Calm UX by default** (NFR-2): 10-second polling interval, configurable 1–60s. No flickering second-by-second digits.
4. **Graceful two-tier degradation** with auto-detection on every launch — no UAC prompt unless the user has already granted it, no broken sensors when elevation is absent.
5. **Native Win11 chrome** via egui/eframe with transparency, always-on-top, docked AppBar semantics, and DWM peek exclusion.

---

## 2. Personas & Use Cases

| Persona | Primary need | Why sidebar |
|---|---|---|
| **Power User / PC builder** | Watch thermals during benchmarks, verify fan curves, spot thermal throttling. | Full mode delivers CPU/GPU temp, fan RPM, voltage rails, power draw. Docked sidebar is always visible without Alt-Tab. |
| **Developer / DevOps** | Spot runaway processes, memory leaks in long-running sessions, disk fill-up. | Per-process top-N by CPU/RAM, per-drive storage + R/W throughput, uptime. Lightweight enough to leave running all day. |
| **Gamer / streamer** | Overlay-adjacent monitoring without a full OBS plugin; check GPU utilization/VRAM during sessions. | GPU metrics via nvml-wrapper (NVIDIA) or OHM (AMD/Intel); VRAM; per-process GPU where cheap. Transparent sidebar doesn't steal focus. |
| **Laptop user** | Battery health, thermal throttling on the go, "why is my fan spinning?" | starship-battery crate for battery state; CPU temp + fan RPM (Full mode); no admin needed for Basic. |
| **Capped-ISP / metered-connection user** *(v2 amendment)* | Track monthly data usage against a billing cap; know how much of the monthly quota is consumed and how many days until reset. | Per-NIC monthly RX/TX/total GB, user-configurable billing-cycle start date, auto-rollover, persistent across reboots. See §3 Tier 4 and §5.5. |

**Secondary personas (explicitly under-served in v1):** users wanting custom themes/plugins; multi-PC fleet operators wanting centralized aggregation. These are deferred (§4). *(Note: per-network-adapter throughput and monthly bandwidth tracking — previously under-served — are now IN scope as of the v2 amendment; see §3 Tier 4 and new §5.5.)*

---

## 3. In-Scope Features (v1)

v1 ships **three tiers of telemetry, all IN scope**, gated only by the two-tier auto-detect model (§5) and the lightweight classifier (NFR-1).

### Tier 1 — Core hardware sensors
| Feature | Basic mode | Full mode | Source |
|---|---|---|---|
| CPU utilization (per-core + aggregate) | ✅ | ✅ | `sysinfo` (0.39.3) `System::cpus()` |
| CPU frequency (per-core) | ✅ | ✅ | `sysinfo` |
| CPU package temperature | ❌ | ✅ | LHM HTTP `/data.json` |
| CPU per-core temperatures | ❌ | ✅ | LHM HTTP `/data.json` |
| CPU power draw (package) | ❌ | ✅ | LHM HTTP `/data.json` |
| CPU fan speed(s) | ❌ | ✅ | LHM HTTP `/data.json` |
| Voltage rails (VCORE, +3.3V, +5V, +12V, etc.) | ❌ | ✅ | LHM HTTP `/data.json` |
| GPU utilization | ✅ (NVIDIA only) | ✅ (all vendors via OHM) | `nvml-wrapper` 0.12.0; OHM fallback |
| GPU temperature | ✅ (NVIDIA only) | ✅ (all vendors) | `nvml-wrapper`; OHM fallback |
| GPU memory utilization / VRAM | ✅ (NVIDIA only) | ✅ (all vendors) | `nvml-wrapper`; OHM fallback |
| GPU power draw | ✅ (NVIDIA only) | ✅ (all vendors) | `nvml-wrapper`; OHM fallback |
| GPU fan speed | ❌ | ✅ | LHM HTTP `/data.json` |
| GPU clocks | ❌ | ✅ | LHM HTTP `/data.json` |
| RAM used / free / total | ✅ | ✅ | `sysinfo` |
| RAM usage history (sparkline) | ✅ | ✅ | Derived from `sysinfo` |
| Battery (state, percent, rate, health) | ✅ | ✅ | `starship-battery` crate |
| System uptime | ✅ | ✅ | `sysinfo` |

### Tier 2 — Per-drive storage
| Feature | Basic | Full | Source |
|---|---|---|---|
| Per-drive used / free / total capacity | ✅ | ✅ | `sysinfo` `Disks` |
| Per-drive read/write throughput | ✅ | ✅ | Performance Data Helper (PDH) via `windows` crate counters — confirmed lightweight |
| SSD SMART health / endurance remaining | ❌ | ✅ | LHM HTTP `/data.json` (`hw.physical_disk.endurance_utilization`) |
| SSD temperature | ❌ | ✅ | LHM HTTP `/data.json` |

### Tier 3 — Per-process top-N
| Feature | Basic | Full | Source |
|---|---|---|---|
| Top-N processes by CPU% | ✅ | ✅ | `sysinfo` `System::processes()` |
| Top-N processes by RAM | ✅ | ✅ | `sysinfo` |
| Top-N processes by GPU% | ⚠️ NVIDIA-only if classifier permits | ⚠️ If classifier permits | `nvml-wrapper` `running_processes()` + `process_utilization()` (see NFR-1 caveat) |

**NFR-1 caveat for per-process GPU:** NVML's `nvmlProcessUtilization_t` retrieval enumerates all GPU contexts per call. Under the cost classifier (§6, NFR-1), this source is **provisionally IN** for v1 but is the first candidate to be **dropped** if profiling on reference hardware shows >0.5% CPU average attributable to the poller. This is called out as a conditional feature, not a hard commitment.

### Tier 4 — Per-network-adapter throughput + monthly bandwidth tracking *(v2 amendment — promoted from Out-of-Scope)*

Network adapter throughput was previously **Out-of-Scope** in v1 (Appendix A). It is now **IN scope**. This tier covers both live per-NIC throughput and the marquee **monthly bandwidth consumption tracking** feature.

**Clarification on terminology.** The user phrased the feature as *"monthly bandwidth consumption per port, where I can choose the start dates."* Best read: **"port" here means network interface (NIC), NOT TCP port.** A TCP-port reading would require deep packet inspection (ETW packet capture, port-to-process attribution) which is heavy and out of scope under NFR-1. A per-NIC reading (one set of counters per adapter) is lightweight, matches what ISP billing caps are measured against, and is what SidebarDiagnostics and Windows Task Manager both surface. **Open question OQ-4 (new)** records this interpretation so the orchestrator can correct it if the user actually meant TCP ports.

| Feature | Basic mode | Full mode | Source |
|---|---|---|---|
| Per-NIC bytes/sec received (RX) | ✅ | ✅ | `GetIfTable2` (`MIB_IF_ROW2.InOctets` raw counter; delta downstream) via the `windows` crate — **lightweight** at the configured tick. |
| Per-NIC bytes/sec sent (TX) | ✅ | ✅ | `GetIfTable2` (`MIB_IF_ROW2.OutOctets` raw counter; delta downstream) |
| Per-NIC packets/sec (RX + TX) | ✅ | ✅ | `GetIfTable2` row counters (future display extension) |
| Per-NIC error count (optional) | ✅ | ✅ | `GetIfTable2` row error counters (future display extension) |
| Per-NIC live throughput formatted in Mbps/Gbps | ✅ | ✅ | Derived from RX/TX deltas (NFR-8 formatting) |
| **Monthly RX bytes (per-NIC)** | ✅ | ✅ | Accumulated by `BandwidthAccountant` (see architecture) from live deltas; persisted to SQLite |
| **Monthly TX bytes (per-NIC)** | ✅ | ✅ | Same |
| **Monthly total bytes (per-NIC)** | ✅ | ✅ | RX + TX |
| **User-configurable billing-cycle start day-of-month** (1–28, plus "last day of month") | ✅ | ✅ | Config in TOML (`config.toml` `[bandwidth] cycle_start_day = 7`); UI day picker |
| **Auto-computed billing-cycle end date** | ✅ | ✅ | `sidebar-domain::billing::cycle_end(start_day, year, month)` — pure function, handles month-length + leap-year + "last day" edge cases |
| **Auto-rollover at cycle end** | ✅ | ✅ | Tokio task checks date on each poll tick; at rollover, current month is archived to history, counters reset |
| **Running monthly total in GB** (RX, TX, total, per-NIC) | ✅ | ✅ | Persisted across app close/reopen, reboot, sleep |
| **Short history table** (current cycle + last 1–2 cycles, per-NIC) | ✅ | ✅ | SQLite `bandwidth_history` table; UI shows a small table. Keep v1 simple: current + previous month only |

**NFR-1 implementation note.** The current adapter uses `GetIfTable2`, filters live non-loopback rows, and calls `FreeMibTable` before returning. It emits raw cumulative counters; the accountant computes deltas and monthly totals. This remains `CostClass::Lightweight` at the configured tick. `GetIfEntry2` is retained only as the originally researched lighter alternative.

**Persistence model (config vs. state).** The monthly bandwidth feature is the architectural reason a persistent store becomes necessary:
- **Config** (`config.toml`) holds *preferences*: `cycle_start_day`, which NICs to track, whether to show the history table. Small, human-editable, TOML (unchanged from v1).
- **Time-series state** (accumulated byte counts, rollover history) holds *data that grows over time and must survive crashes*. This goes to **SQLite** (`bandwidth.db` in `%APPDATA%\sidebar\`), NOT TOML. Rationale: TOML is a config format, not a database; appending to a TOML file requires full reparse + rewrite on every tick (expensive at 10s cadence over months), has no transactional safety against partial writes during crash/sleep, and cannot range-query history without loading the whole file. SQLite (via `rusqlite`, bundled, ~1 MB) gives append-friendly inserts, ACID durability, indexed date-range queries for the history table, and trivial schema migration. See architecture.md AD-11 for the formal decision. (Sources: SQLite time-series-in-Rust guide, https://medium.com/rustaceans/harnessing-the-power-of-sqlite-for-time-series-data-storage-in-rust-a-comprehensive-guide-321612470836, retrieved 2026-07-07; HN discussion of SQLite append-only audit-log suitability, https://news.ycombinator.com/item?id=17855045, retrieved 2026-07-07.)
- **Current schema:** persistence is at schema version 2. The `current_cycle_metadata` table stores cycle identity and reset metadata alongside the current counters; migrations are registered as v0 → v1 → v2 and remain idempotent on reopen.

### UX features
| Feature | In v1 |
|---|---|
| Transparent, borderless, always-on-top sidebar docked to screen edge | ✅ |
| AppBar registration (reserve edge space, slide on hover off-screen) | ✅ |
| DWM "exclude from peek" + capture exclusion via `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` | ✅ (option toggle) |
| Click-through toggle (Ctrl+drag to reposition) | ✅ |
| Configurable polling interval (1s–60s, default 10s) | ✅ |
| Per-metric enable/disable + reorder via drag | ✅ |
| Light/dark theme + accent color | ✅ |
| Multi-monitor: choose target monitor + DPI-aware | ✅ |
| Sparkline history (rolling window) | ✅ |
| Configurable thermal/power thresholds with visual alert | ✅ |
| Status pill (Basic / Full) with tooltip | ✅ |
| Settings file (TOML) at `%APPDATA%\sidebar\config.toml` | ✅ |
| **Bandwidth billing-cycle start day picker** (1–28 + "last day of month") *(v2)* | ✅ |
| **Monthly bandwidth panel**: per-NIC RX/TX/total in GB, days-until-reset countdown *(v2)* | ✅ |
| **Bandwidth history table** (current + previous cycle, per-NIC) *(v2)* | ✅ |
| **Raw-values toggle** in Settings (show Hz/bytes/bps instead of GHz/GB/Mbps) *(v2, NFR-8)* | ✅ |
| **Temperature unit toggle** (°C default / °F) *(v2, NFR-8)* | ✅ |
| **State database (SQLite) at `%APPDATA%\sidebar\bandwidth.db`** *(v2)* | ✅ |
| **Sidebar width** (100–300px slider) *(v1.0 parity with SidebarDiagnostics)* | ✅ |
| **Font size** (10–22px, applied as zoom) *(v1.0 parity)* | ✅ |
| **UI scale** (50–300%, composes with font zoom) *(v1.0 parity)* | ✅ |
| **Alert blink** (Critical alerts flash ~1Hz; accessibility for color-blind users) *(v1.0 parity)* | ✅ |
| **Custom background color + opacity** *(v1.0 parity)* | ✅ |
| **Custom font color** *(v1.0 parity)* | ✅ |
| **X/Y position offsets** (fine-tune dock position) *(v1.0 parity)* | ✅ |
| **Run at Windows startup** (HKCU Run key, no admin) *(v1.0 parity)* | ✅ |
| **Initially hidden** (start minimized to tray) *(v1.0 parity)* | ✅ |
| **Pause sensors when hidden** (save CPU/battery) *(v1.0 parity)* | ✅ |
| **Drive used-space alert** (configurable % threshold) *(v1.0 parity)* | ✅ |
| **Bandwidth in/out alerts** (Mbps thresholds per NIC) *(v1.0 parity)* | ✅ |
| **CPU bus clock (BCLK)** + **RAM clock + voltage** (Full mode via LHM) *(v1.0 parity)* | ✅ |
| **Per-NIC IPv4 address** (shown next to each adapter in the bandwidth panel) *(v1.0 parity)* | ✅ |
| **Extended hotkeys** (toggle/show/hide/cycle-edge/cycle-screen/reload/toggle-reserve/close) *(v1.0 parity)* | ✅ |
| **Per-metric line-graph popup** (click 📈 next to any row → resizable history window with current/min/max) *(v1.0 parity)* | ✅ |
| **Localization** (i18n: en + es shipped, extensible label-table system; Language selector in Settings) *(v1.0 parity)* | ✅ |

---

## 4. Out-of-Scope (v1)

Explicitly **deferred**. These are not "forgotten" — they are documented as future work and revisited in a later change.

- ~~**Per-network-adapter throughput**~~ *(v2 amendment: promoted to IN scope, see §3 Tier 4).* Retained here as a struck-through marker so the history is traceable.
- **Custom themes / plugin system / scripting.** v1 ships a small fixed theme set.
- **Cloud sync of config / telemetry export to remote.** v1 is local-only.
- **Mobile / companion app.** Not applicable.
- **Non-NVIDIA GPU metrics in Basic mode.** AMD/Intel GPU metrics require OHM (Full mode). No fallback.
- **CPU/GPU frequency limit reasons** (e.g., `HWiNFO`-style throttling reason codes). Out of scope; OHM does not expose them reliably.
- **Audio/session/media metadata.** Out of scope.
- **Per-TCP-port bandwidth attribution** (deep packet inspection / ETW packet capture). Out of scope under NFR-1 — see §3 Tier 4 clarification; "per port" is interpreted as per-NIC.
- **Per-process network attribution** (which process is using the bandwidth). Out of scope; would require IP Helper table walks per tick. Revisit post-v1.
- **Bandwidth quota alerts / hard caps** (auto-warn at 80% of cap). Out of scope for v1; the running total is shown, but no threshold alerting. Candidate for v1.1.

---

## 5. Two-Tier Model (Basic vs Full)

### 5.1 Definitions

> **Current implementation:** Full mode uses bundled `LibreHardwareMonitor.exe`
> and the local HTTP `GET http://127.0.0.1:<port>/data.json` bridge. The WMI
> description retained in the original paragraph below is historical research
> context only; it is not an implementation contract for LHM v0.9.6.

- **Basic mode.** No administrator privileges required. Telemetry sourced from: `sysinfo`, `nvml-wrapper` (NVIDIA-only GPU), `starship-battery`, `windows` PDH counters, Win32 storage APIs. **No CPU package temperature, no fan speeds, no voltages, no non-NVIDIA GPU sensors, no SMART.** The sidebar runs as a standard user process.
- **Full mode.** Bundled `LibreHardwareMonitor.exe` (LHM) is launched as a hidden elevated subprocess only after explicit user action. LHM serves the sensor tree at `http://127.0.0.1:<port>/data.json`; the sidebar host remains **non-elevated** and uses the local HTTP bridge. WMI is historical context only: LHM v0.9.5+ no longer publishes the WMI namespace.

### 5.2 Auto-detection (on every launch)

**Implementation correction (2026-07-11):** the current LHM v0.9.6 bridge is
HTTP-only: `GET http://127.0.0.1:<port>/data.json` with a 500 ms timeout and a
JSON-signature check. The host never auto-elevates. On explicit launch, the
supervisor patches `LibreHardwareMonitor.exe.config` (`runWebServerMenuItem`
and lowercase `listenerPort`), starts `LibreHardwareMonitor.exe` with
`ShellExecuteW("runas")`, and re-probes for up to 5 seconds. The WMI wording in
the original table above is historical and must not be used for implementation.

There is **no Settings toggle** for tier selection. On every launch, sidebar performs an HTTP probe:

1. **HTTP reachability probe.** Attempt `GET http://127.0.0.1:<port>/data.json` with the 500 ms timeout. A valid LHM JSON signature resolves Full; refusal, timeout, or a non-LHM response resolves Basic. Ports 17127–17137 are tried for collisions.
2. **No auto-elevation.** A failed probe leaves the host in Basic; the launch probe never calls `launch_elevated` and never prompts for UAC.
3. **Explicit launch decision.** Only the status-pill action may call `ShellExecuteW("runas")` after patching `LibreHardwareMonitor.exe.config` (`runWebServerMenuItem=true`, `listenerPort=<port>`), then re-probe for up to 5 seconds. The current UI callback is a known integration gap (see §12).

### 5.3 Status pill UX

- A small colored pill in the sidebar header reads **`BASIC`** (muted gray) or **`FULL`** (accent green).
- Hovering the pill shows a tooltip:
  - **Basic:** *"Basic mode. CPU temperature, fan speeds, voltages, and non-NVIDIA GPU sensors require LibreHardwareMonitor with administrator privileges. Click to learn how to enable Full mode."*
  - **Full:** *"Full mode. LibreHardwareMonitor is running. All sensors active."*
- Clicking the pill triggers the explicit LHM launch request (which UAC-prompts if needed); the supervisor-owner thread performs the launch. This remains the **only** elevation entry point. Real UAC/LHM behavior is still a manual §12 acceptance gate.

### 5.4 Privilege handling

- The sidebar host process is **never** auto-elevated. We do not embed a UAC manifest requesting `requireAdministrator`.
- OHM is launched via `ShellExecuteW(..., "runas", ...)` **only** when the user explicitly clicks the "Enable Full mode" button. Windows caches the resulting elevated child handle for the session; subsequent launches within the same Windows session that find the namespace already reachable do not re-prompt.
- On the next launch, if OHM is not running and namespace probe fails, we silently fall back to Basic. No nag.

### 5.5 Monthly Bandwidth Tracking *(v2 amendment — new section)*

The bandwidth tracking feature is **tier-agnostic**: it works identically in Basic and Full mode because it reads from the `windows` crate (`GetIfTable2`), not from LHM. It does not appear in the two-tier matrix in §7 as a Basic-vs-Full split; it is the same in both.

**Behavioral contract.**

1. **Cycle definition.** A billing cycle is defined by its **start day-of-month** (1–28, or "last day of month"). The cycle runs from `00:00:00 local` on the start day through `23:59:59.999` the day before the next start day. Example: `cycle_start_day = 7` → cycle is the 7th of one month through the 6th of the next month.
2. **End-date computation.** `cycle_end(start_day, year, month) -> NaiveDate` is a pure function in `sidebar-domain::billing`. It handles:
   - Short months (Feb 28/29) — if `start_day > days_in_month`, clamp to last day.
   - Leap years.
   - "Last day of month" selection — cycle end is the day before the last day of next month.
   - This function is **the** unit-test hotspot for the feature.
3. **Live accumulation.** On every poll tick (default 10s), the network adapter reads `InOctets`/`OutOctets` for each tracked NIC, computes the delta from the previous tick's reading, and adds it to the in-memory `MonthlyAccumulator { rx_bytes, tx_bytes, cycle_start: NaiveDate }`. The delta computation handles counter wraparound (64-bit counters do not wrap in practice on Win11, but the code defends against it).
4. **Persistence.** The accumulator is flushed to SQLite (`bandwidth.db`) on a debounced schedule (every ~60s, and on graceful shutdown, and on rollover). SQLite WAL mode allows cheap appends without blocking the poller.
5. **Rollover.** A tokio task (the `BandwidthAccountant`) checks `Local::today() >= current_cycle_end` on every poll tick. When true:
   - The current cycle's `{adapter, cycle_start, rx_bytes, tx_bytes}` row is moved into the `bandwidth_history` table.
   - A new accumulator is created with `cycle_start = next_cycle_start`.
   - The flush is forced (no data loss across the boundary).
6. **App sleep / shutdown / crash.** Because the accumulator flushes on graceful shutdown and SQLite is ACID, a clean close loses at most the last ~60s of data. A crash (kill -9, power loss) loses at most the last flush window — acceptable for a *consumption tracker* (not a billing system). The next launch re-reads the last-persisted accumulator and resumes.
7. **Adapter identity.** NICs are identified by their **LUID** (Locally Unique Identifier, `MIB_IF_ROW2.InterfaceLuid`) — NOT by name (names change when a user renames "Ethernet" to "WAN") and NOT by index (indexes reshuffle across reboots/dock events). The accumulator is keyed on LUID. If an adapter disappears (undocked), its accumulator is retained but frozen; if it reappears, accumulation resumes. *(Edge case: Windows LUIDs are stable across reboots per Microsoft's IP Helper contract — this is the documented guarantee that makes per-NIC tracking feasible.)*
8. **UI.** A dedicated "Bandwidth" panel shows, per tracked NIC: adapter friendly name, this-cycle RX/TX/total in GB, and a countdown "X days until reset (on YYYY-MM-DD)". A small history table below shows the previous cycle's totals. The billing-cycle start day is editable in Settings; changing it takes effect at the next rolver boundary (does not retroactively re-split the current cycle).

**What this feature does NOT do (out of scope for v1).**
- No per-process attribution (see §4).
- No TCP-port breakdown (see §4 and §3 Tier 4 clarification).
- No quota alerts / auto-warn at N% of cap (see §4).
- No cloud sync (see §4).
- No multi-year retention (history table keeps current + previous cycle only in v1; older rows are pruned on rollover).

---

## 6. Non-Functional Requirements

### NFR-1 — Lightweight mandate (CRITICAL)

**Statement.** *Every telemetry source sidebar picks up must be lightweight by definition. If anything is flagged as heavy, sidebar does not pick it up.*

**Quantitative threshold.** A source is **heavy** if, measured on a reference machine (any modern 8+ core x86_64 CPU, ≥ 16 GB RAM, Win11 24H2 or 25H2 — see `docs/backlog/nfr-thresholds.md` T-31 for the per-machine calibration approach), it contributes **> 0.5% CPU average across polling cycles** above the calibrated idle baseline, **or** it triggers expensive kernel/syscall churn disproportionate to a base tick — specifically:

- `NtQuerySystemInformation(SystemProcessInformation)` full-process enumeration more frequently than the configured base tick (default 10s), or
- `NtQuerySystemInformation(SystemPerformanceInformation)` more than once per tick, or
- Polling the LHM HTTP endpoint more frequently than the configured base tick, or
- Spawning more than one OHM subprocess instance, or
- Issuing more than one WQL query per sensor category per tick.

**Sensor cost classifier.** The architecture defines a `SensorCostClassifier` trait (see `architecture.md` §6) that tags each `SensorDescriptor` with a `CostClass` ∈ {`Lightweight`, `Watch`, `Heavy`, `Deferred`}. Only `Lightweight` and (with profiling evidence) `Watch` sources are wired into the v1 poller. `Heavy` and `Deferred` sources are documented in **Appendix A: Deferred/Heavy Sources** and revisited post-v1.

**Per-process GPU caveat.** NVML `running_processes()` + `process_utilization()` per-process polling is classified `Watch`. It is provisionally included in v1; the first profiling gate on reference hardware determines whether it ships as IN or is moved to Deferred. This is the only `Watch` source in v1.

**Enforcement.** The classifier is a compile-time gating mechanism in `sidebar-sensor`: adapters cannot register a source without an accompanying `CostClass`. CI runs a profiling micro-benchmark (`cargo bench --bench poll_cost`) on a Windows runner that fails if any source's rolling average exceeds the 0.5% threshold.

### NFR-2 — Default 10s polling interval (CRITICAL)

- **Default interval: 10 seconds.** Configurable range: **1s–60s**.
- All sensor sources are polled on the same tick by default (one tokio task, one interval). Independent per-source intervals are **out of scope** for v1.
- The interval is exposed as `config.poll_interval_seconds` (integer). Values outside [1, 60] are clamped and logged.

### NFR-3 — Cold-start latency

- **< 2 seconds** from process start to first complete frame on the reference machine, Basic mode.
- Full-mode cold start (including LHM subprocess launch + first HTTP round-trip): **< 6 seconds**. LHM's own startup is the dominant cost; we do not control it.

### NFR-4 — Memory footprint

- **< 80 MB resident set** in steady state (Basic), measured via `GetProcessMemoryInfo(WorkingSetSize)`.
- **< 120 MB resident set** in Full mode (host process only — OHM's memory is separate and reported independently).

### NFR-5 — Windows 11 compatibility

- **Target:** Windows 11 24H2 (build 26100) and 25H2. 64-bit only.
- Best-effort, unsupported: Windows 10 22H2. We do not block install on Win10 but do not fix bugs that are Win10-specific.
- Architecture: x86_64 only in v1. aarch64 Windows is deferred.

### NFR-6 — DPI and multi-monitor

- Per-monitor DPI-aware v2 (`SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`). No blurry scaling on 4K + 1080p mixed setups.
- Sidebar can be docked to any connected monitor's edge. Target monitor stored in config.

### NFR-7 — Always-on-top, transparent, click-through-optional

- **Always-on-top:** `HWND_TOPMOST` via `SetWindowPos`. Survives Win+D (show desktop).
- **Transparent:** egui `ViewportBuilder::with_transparent(true)` + `clear_color([0,0,0,0])` + `Frame::none()`. Confirmed available in current egui (0.35.0 latest, retrieved 2026-07-07).
- **Click-through-optional:** A hotkey (default `Ctrl+Shift+S`) toggles `WS_EX_TRANSPARENT` so clicks pass through to whatever is beneath. Off by default (sidebar is interactive).
- **DWM peek exclusion:** `DwmSetWindowAttribute(DWMWA_EXCLUDED_FROM_PEEK, TRUE)` so the sidebar doesn't disappear during Aero Peek (`Win+Tab`, hover-show-desktop).
- **Capture exclusion:** Optional `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` toggle for streamers who don't want sidebar in their capture. Off by default; DWM peek exclusion remains a separate `DwmSetWindowAttribute` call.

### NFR-8 — Human-readable output by default *(v2 amendment — NEW)*

**Statement.** *All UI-displayed telemetry values must default to human-readable formatting. Raw values (Hz, bytes, bytes/sec) are available only behind an explicit "raw values" toggle in Settings, off by default.*

**Specifics.** The `format` module (see architecture §4, `sidebar-domain::format`) provides pure functions, fully unit-tested, that map `(value, unit)` → display string. Defaults:

| Metric kind | Default display | Example | Notes |
|---|---|---|---|
| CPU clock | GHz (3 sig figs) | `3.84 GHz` | Derived from `Hertz` reading; `format_hz(u64)`. |
| RAM / VRAM used / total | GB (decimal) | `16.0 GB` / `1.84 TB` | **Decimal GB (10^9), not binary GiB (2^30).** Decimal is the end-user-friendly convention used by SidebarDiagnostics, Windows Task Manager (since Win10 1903), and disk manufacturers. Binary GiB available behind the raw toggle. `format_bytes(u64)`. |
| Storage capacity / used | GB / TB (decimal) | `512.0 GB`, `1.84 TB` | Same decimal-GB choice, same precedent. |
| Network throughput (live) | Mbps / Gbps (decimal) | `48.2 Mbps`, `1.20 Gbps` | Decimal Mbit/s = 10^6 bit/s. Bytes/sec only in raw toggle. `format_bps(u64)`. |
| Monthly bandwidth | GB / TB (decimal) | `123.4 GB` this cycle | Same as storage. |
| Temperatures | °C default | `62 °C` | °F available via Settings toggle (affects all temp readings app-wide). `format_temp(f64, TempUnit)`. |
| Voltages | V (3 decimals) | `1.248 V`, `12.040 V` | `format_voltage(f64)`. |
| Fan speeds | RPM | `1840 RPM` | `format_rpm(u32)`. |
| Power | W (2 decimals) | `45.20 W` | `format_power(f64)`. |
| Battery | % + state | `78% (Charging)` | `format_battery(u8, BatteryState)`. |
| Utilization | % | `42%` | integer percent; `format_percent(f64)`. |

**Decimal-vs-binary justification.** Decimal GB (10^9 bytes) is chosen for end-user friendliness: it matches what ISPs advertise (e.g. "1 TB cap" = 10^12 bytes), what disk manufacturers print on the box, and what SidebarDiagnostics (the product we're cloning) displays. Binary GiB (2^30) is more "correct" for memory but confuses users when their "16 GB" RAM shows as "14.9 GiB". The raw toggle exposes binary for power users. *(Precedent: Windows Task Manager switched RAM to decimal GB display in Win10 1903 for exactly this reason.)*

**Locale (open question, see OQ-5).** v1 uses `.` as the decimal separator and no thousands separator in the default format functions (e.g. `1.84 TB`, `123.4 GB`). Locale-aware separators (`,` vs `.`; `1,234.5` vs `1.234,5`) are deferred to v1.1. Called out so the orchestrator can confirm.

**Toggle surface.** A single "Display" section in Settings exposes: temperature unit (°C/°F), raw-values toggle (overrides all formatters to show Hz/bytes/bps), and decimal/binary toggle for byte values. Defaults: °C, human-readable, decimal.

---

## 7. Telemetry Coverage Matrix

Each sensor category × tier × source crate/API.

| Category | Metric | Basic source | Full source |
|---|---|---|---|
| CPU | utilization % | `sysinfo::System::cpus().cpu_usage()` | same |
| CPU | per-core freq | `sysinfo::System::cpus().frequency()` | same |
| CPU | package temp | — | LHM HTTP `/data.json` sensor tree |
| CPU | per-core temp | — | LHM HTTP `/data.json` |
| CPU | package power | — | LHM HTTP `/data.json` (`hw.power{hw.type="cpu"}`) |
| CPU | fan RPM | — | LHM HTTP `/data.json` |
| CPU | voltages | — | LHM HTTP `/data.json` |
| GPU (NVIDIA) | utilization % | `nvml-wrapper` `Device.utilization_rates()` | same |
| GPU (NVIDIA) | temp | `nvml-wrapper` `Device.temperature(TemperatureSensor::Gpu)` | same |
| GPU (NVIDIA) | VRAM used/total | `nvml-wrapper` `Device.memory_info()` | same |
| GPU (NVIDIA) | power draw | `nvml-wrapper` `Device.power_usage()` | same |
| GPU (AMD/Intel) | all metrics | — | LHM HTTP `/data.json` (`hw.type="gpu"`) |
| GPU | fan RPM | — | LHM HTTP `/data.json` |
| GPU | clocks | — | LHM HTTP `/data.json` |
| Memory | used/free/total | `sysinfo::System::used_memory()/total_memory()` | same |
| Storage | per-drive capacity | `sysinfo::Disks` | same |
| Storage | R/W throughput | PDH counter `\PhysicalDisk(*)\Disk Read/Write Bytes/sec` via `windows` crate | same |
| Storage | SMART endurance | — | LHM HTTP `/data.json` (`hw.physical_disk.endurance_utilization`) |
| Storage | SSD temp | — | LHM HTTP `/data.json` |
| Battery | state/percent/rate | `starship-battery` `Manager::batteries()` | same |
| Process | top-N CPU | `sysinfo::System::processes()` (one enumeration per tick) | same |
| Process | top-N RAM | `sysinfo::System::processes()` | same |
| Process | top-N GPU | `nvml-wrapper` `running_processes()` + `process_utilization()` (NVIDIA-only, **Watch** cost class) | same |
| Network *(v2)* | per-NIC bytes/sec RX | `GetIfTable2` `MIB_IF_ROW2.InOctets` raw counter via `windows` crate (**Lightweight**) | same |
| Network *(v2)* | per-NIC bytes/sec TX | `GetIfTable2` `MIB_IF_ROW2.OutOctets` raw counter | same |
| Network *(v2)* | per-NIC packets/sec | `GetIfTable2` row counters (future display extension) | same |
| Network *(v2)* | per-NIC error count | `GetIfTable2` row error counters (future display extension) | same |
| Bandwidth *(v2)* | monthly RX/TX/total bytes per-NIC | Accumulated from live deltas by `BandwidthAccountant`; persisted to SQLite `bandwidth.db` | same (tier-agnostic) |
| Bandwidth *(v2)* | billing-cycle start day | Config TOML `[bandwidth] cycle_start_day` (UI day picker) | same (tier-agnostic) |
| Bandwidth *(v2)* | cycle end date | Computed by `sidebar-domain::billing::cycle_end` (pure fn) | same (tier-agnostic) |
| Bandwidth *(v2)* | history (current + prev cycle) | SQLite `bandwidth_history` table | same (tier-agnostic) |

---

## 8. Risks & Mitigations

| ID | Risk | Severity | Mitigation |
|---|---|---|---|
| R1 | CPU/GPU package temperature requires OHM; no pure-Rust path exists on Windows | HIGH | Bundle OHM as documented. Do not pursue alternatives (e.g., ACPI thermal zones via WMI are unreliable; MSR access requires a kernel driver). This is the defining constraint, not a bug. |
| R2 | LHM integration depends on a local HTTP schema | HIGH | Pin LHM v0.9.6 and validate the `/data.json` signature + fixture; use a loopback-only client with redirects disabled. |
| R3 | UAC prompt for OHM launch is friction | HIGH | Two-tier auto-detect means Basic mode works with zero prompts. OHM elevation is opt-in via the status-pill button, not auto-requested. Document the one-time consent clearly. |
| R4 | egui transparent+borderless+topmost window on Win11 has thin precedent | MEDIUM | Validated via egui Discussion #2803 and #4228 (community precedent for transparent sidebars). SPIFF: `ViewportBuilder::with_transparent(true)` confirmed present in egui 0.35.0 docs. Manual smoke test on Win11 24H2 required in sdd-verify. |
| R5 | Non-NVIDIA GPUs (AMD/Intel iGPUs) have no Rust-native metric path | MEDIUM | Full mode covers them via OHM. Basic mode explicitly does not. Document in tooltip. |
| R6 | Per-process GPU polling may breach NFR-1 (NVML enumeration cost) | MEDIUM | Classified `Watch`. Profiling gate in CI (`cargo bench --bench poll_cost`). Auto-deferred if it breaches 0.5% CPU threshold. |
| R7 | LHM HTTP schema/port drift across versions | LOW | Probe on every launch, pin the LHM binary, patch the HTTP config keys, and reject non-LHM JSON responses. |
| R8 | TDD coverage illusion — heavy mock usage hides real adapter bugs | LOW | Strict TDD covers domain logic (~80% feasible). Adapters are integration-tested on a Windows CI runner, not mocked away. GUI E2E = manual smoke checklist. |
| R9 | *(v2)* Bandwidth cycle-end date arithmetic wrong on edge cases (Feb 29, "last day of month", month boundaries) | MEDIUM | `cycle_end` is a pure function in `sidebar-domain::billing` with exhaustive unit tests covering every edge case (28/29/30/31-day months, leap years, "last day" selection, year boundary Dec→Jan). Property-based tests (`proptest`) generate random valid `(start_day, year, month)` triples and assert invariants. |
| R10 | *(v2)* NIC identity drift across reboots/dock events makes per-NIC totals unreliable | MEDIUM | Track adapters by **LUID** (Locally Unique Identifier), not by name or index. Windows guarantees LUID stability across reboots per the IP Helper contract. If an adapter disappears (undocked), its accumulator is frozen and resumed on reappearance. Documented in §5.5. Fallback: if LUID proves unstable in sdd-verify profiling, fall back to MAC address (less stable across virtual adapters but acceptable). |
| R11 | *(v2)* SQLite `bandwidth.db` corruption on crash / forced shutdown loses data | LOW | SQLite in WAL mode is ACID-durable; a clean close loses at most the last ~60s (debounced flush). A crash loses at most the last flush window — acceptable for a *consumption tracker*, not a billing system. SQLite's own crash-recovery (journal rollback) handles the common case. No mitigation needed beyond WAL + debounced flush. |
| R12 | *(v2)* SignPath Foundation rejects the sidebar application (OSI license requirement, MFA requirement, "no hacking tools" clause) | LOW | sidebar is a clean telemetry tool, MPL-2.0 or MIT licensed (OSI-approved), no PUPs, no hacking-tool features. The OHM subprocess is MPL-2.0 (OSI-approved) — but note: SignPath's "sign your own binaries only" rule means we can sign our Rust binary but **OHM.exe remains unsigned** (it's upstream OSS, we redistribute it). This matches every other consumer of OHM. Documented in §9 OQ-1 resolution. |
| R13 | *(v2)* Microsoft Store AppContainer sandboxing breaks the LHM subprocess bridge (UAC `runas` from sandboxed parent, or local HTTP access blocked) | MEDIUM | Store-distributed MSIX runs sandboxed. The GitHub Releases / winget path is unsandboxed and has full feature parity. **Mitigation:** ship GitHub Releases + winget first (v1), add Store build as v1.1 milestone once sdd-verify confirms sandbox compatibility. If sandboxing proves incompatible, the Store build ships Basic-mode-only with a note directing Full-mode users to the GitHub build. Documented in §9 OQ-1. |

---

## 9. Open Questions (for orchestrator / later phases)

### OQ-1 — Distribution format — **RESOLVED (v2 amendment): zero-cost-first stack**

**Previous status:** TBD (three options: MSIX signed at ~$120/yr, MSIX+AppInstaller same cost, portable ZIP $0 but unsigned). **Now resolved** with a zero-cost-first recommendation driven by the user's hard constraint: **NO paid options, definitely not $120/yr** (India pricing context — the user explicitly rejected Azure Trusted Signing's ~$9.99/mo ≈ ₹1,000/mo ≈ ₹12,000/yr as too expensive).

**2026 research findings (retrieved 2026-07-07):**

| Channel / signing path | Cost (2026) | What it gives us | Source |
|---|---|---|---|
| **SignPath Foundation** (free OSS code signing) | **$0** | Free code signing certificate + signing service for OSI-licensed OSS projects built via trusted CI (GitHub Actions supported). Removes most SmartScreen friction over time as reputation accrues. | https://signpath.org/terms.html, https://signpath.io/solutions/open-source-community, https://docs.signpath.io/trusted-build-systems/github (retrieved 2026-07-07). Eligibility: OSI-approved license, public repo, free downloads, no malware/PUPs, no hacking tools, MFA enforced on signers, code signing policy published. |
| **Microsoft Store (Partner Center) — new onboarding** | **$0** (both Individual AND Company accounts) | Free distribution + auto-update infrastructure + **signed automatically by Microsoft** (no separate signing needed for Store-distributed MSIX). Sandbox restrictions apply. | https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account (retrieved 2026-07-07): *"With the new onboarding experience, there are no registration fees for either account type."* Entry point: storedeveloper.microsoft.com. **India pricing: $0 (free) for both account types.** This eliminates the legacy ~$19 individual / ~$99 company fees. |
| **winget** (community package repo) | **$0** | Free discoverability + `winget install sidebar` UX. PR-based submission to `microsoft/winget-pkgs`. No signing of its own; uses whatever we ship. | https://learn.microsoft.com/en-us/windows/package-manager/package/repository, https://github.com/microsoft/winget-pkgs (retrieved 2026-07-07). Tool: `winget-create`. Processing time ~1 hour per PR. |
| **GitHub Releases** | **$0** | Free binary hosting on our repo's Releases page. The canonical download endpoint. | (Standard GitHub feature, stable.) |
| **Scoop / Chocolatey** (community package managers) | **$0** | Alternative install channels for power users. Community-maintained manifests (we submit, community reviews). | (Standard ecosystem feature.) |
| **Azure Trusted Signing** (formerly Azure Code Signing) | **~$9.99/mo** (Basic, 5,000 sigs/mo) + $0.005/sig over quota ≈ **$120/yr** | Standard OV code signing via Azure. **REJECTED by user on cost grounds.** Listed for completeness. | https://azure.microsoft.com/en-us/pricing/details/artifact-signing/, https://www.infoworld.com/article/2337355/understanding-microsofts-trusted-signing-service.html (retrieved 2026-07-07). No free tier exists in 2026. |
| **OV cert from DigiCert/Sectigo** | **$150–300/yr** | Traditional OV cert. **REJECTED on cost grounds.** EV certs no longer give instant SmartScreen bypass since 2024, so there's no premium reason to consider EV. | (Carried over from v1 research.) |
| **Self-signed + SmartScreen reputation building** | **$0** | Free but bad UX — SmartScreen hard-blocks unsigned/untrusted EXEs for early users. Reputation accrues slowly over downloads. **Not recommended as primary path; viable only as a fallback if SignPath is denied.** | (Standard behavior.) |

**Recommendation (RESOLVED): the zero-cost-first distribution stack.**

```
                        ┌─────────────────────────────────────────────────┐
                        │  sidebar v1 distribution stack (zero-cost)       │
                        └─────────────────────────────────────────────────┘

  Build (GitHub Actions) ──▶ SignPath Foundation ──▶ Signed sidebar.exe
       (trusted CI)            (free OSS cert)         (OV, reputation builds)
                                  │
                                  ├──▶ GitHub Release (portable ZIP)
                                  │      ↳ for power users, no installer
                                  │
                                  ├──▶ winget manifest PR (microsoft/winget-pkgs)
                                  │      ↳ `winget install sidebar`
                                  │
                                  ├──▶ Scoop / Chocolatey community manifests
                                  │      ↳ alternative power-user channels
                                  │
                                  └──▶ Microsoft Store (MSIX, signed by Microsoft)
                                         ↳ mainstream users, auto-update built-in
                                         ↳ Partner Center account = $0 (new flow)

  Bundled OHM.exe ──▶ remains UNSIGNED in all channels (it's upstream OSS,
                       we redistribute it; SignPath signs OUR binary only).
                       OHM is launched with `runas` regardless of signing.
```

**Total annual cost: $0.** No Azure subscription, no CA cert, no Partner Center fee.

**Caveats and caveats-on-caveats:**

1. **SignPath signs our Rust binary, NOT LHM.exe.** The bundled `LibreHardwareMonitor.exe` is upstream OSS (MPL-2.0); per SignPath's "sign your own binaries only" rule, we cannot re-sign it under our cert. It ships unsigned in every channel. LHM is launched with `ShellExecuteW("runas")` regardless, so signing status does not affect the elevation flow.
2. **SignPath application can be rejected.** The Foundation reviews each application. sidebar's profile (clean telemetry tool, OSI license, no PUPs, no hacking features) is squarely within their scope, but approval is not guaranteed. **Fallback if rejected:** ship unsigned via GitHub Releases + winget (SmartScreen warnings for early users, reputation builds over time) and pursue the Microsoft Store path (which signs via Microsoft and sidesteps the cert question entirely).
3. **Microsoft Store sandboxing.** Store-distributed MSIX apps run in an AppContainer sandbox. This *may* affect: (a) the bundled LHM subprocess launch (UAC `runas` from a sandboxed parent — needs sdd-verify testing), (b) local loopback HTTP access, (c) file system writes to `%APPDATA%\sidebar\` (fine — AppData is in the sandbox's writable region). **Risk R13 (new):** if Store sandboxing breaks the LHM bridge, the Store build ships Basic-mode-only with a note, and Full-mode users are directed to the GitHub Releases build. Documented in §8 as R13.
4. **SmartScreen reputation is per-cert, per-filename.** SignPath's cert is shared across all their OSS projects, so reputation accrues faster than a brand-new solo cert — but a brand-new filename (`sidebar.exe`) still starts cold. Expect some SmartScreen warnings in the first weeks regardless of signing. The Microsoft Store path sidesteps this entirely (Store apps don't trigger SmartScreen).
5. **Auto-update mechanism.** The Store path gives free auto-update. The GitHub Releases + winget path gives `winget upgrade sidebar` (manual or scriptable). The portable ZIP path has NO auto-update (user replaces files). For v1, this is acceptable; v1.1 can add an in-app "new version available" check that links to the GitHub Release.

**What this changes in the architecture:** see architecture.md §11 (new) — Build & Release Pipeline. The GitHub Actions workflow gains a SignPath signing step; the release artifact matrix expands to {portable ZIP, winget manifest, MSIX-for-Store}. The crate structure, OHM bundling, and config/state story are **unchanged**.

**Open sub-question (deferred to sdd-tasks):** Should we publish to the Microsoft Store at all for v1, given the sandboxing risk (R13)? Recommendation: ship GitHub Releases + winget first (no sandbox, full feature parity), add the Store build as a v1.1 milestone once sandbox compatibility is verified in sdd-verify. This is a sequencing decision, not an architectural one.

### OQ-2 — Per-process GPU feature ship/defer

Resolved by NFR-1 profiling gate at implementation time. Documented as conditional.

### OQ-3 — Rust edition

sdd-init recorded edition 2021 (re-evaluate 2024 once MSRVs align). `sysinfo` 0.39.3 now requires MSRV 1.95 (retrieved 2026-07-07), which is itself a 2024-edition-capable toolchain. **Tentative: stay on edition 2021 for v1**, revisit when all transitive deps certify against edition 2024. No action needed in this phase.

### OQ-4 — "Per port" interpretation (NEW, v2 amendment)

The user phrased the bandwidth feature as *"monthly bandwidth consumption per port."* This proposal interprets **"port" = network interface (NIC)**, not TCP port. Rationale: (a) ISP billing caps are measured per-connection (per-NIC), not per-TCP-port; (b) per-TCP-port attribution requires deep packet inspection / ETW packet capture, which is heavy and out of scope under NFR-1; (c) SidebarDiagnostics (the product we're cloning) surfaces per-NIC, not per-port; (d) "where I can choose the start dates" strongly implies a billing-cycle concept, which maps to per-NIC. **If the user actually meant TCP port, this is a significant scope change** — flag for orchestrator confirmation before sdd-spec. Current best-read stands: per-NIC.

### OQ-5 — Locale-aware number formatting (NEW, v2 amendment)

NFR-8 defaults to `.` decimal separator and no thousands separator (e.g. `1.84 TB`, `123.4 GB`). Locale-aware formatting (`,` vs `.`; `1,234.5` vs `1.234,5`) is deferred to v1.1. The `format` module is structured to accept a `Locale` parameter later without API breakage. Flag for orchestrator confirmation that the locale-free default is acceptable for v1.

### OQ-2 — Per-process GPU feature ship/defer

Resolved by NFR-1 profiling gate at implementation time. Documented as conditional.

### OQ-3 — Rust edition

sdd-init recorded edition 2021 (re-evaluate 2024 once MSRVs align). `sysinfo` 0.39.3 now requires MSRV 1.95 (retrieved 2026-07-07), which is itself a 2024-edition-capable toolchain. **Tentative: stay on edition 2021 for v1**, revisit when all transitive deps certify against edition 2024. No action needed in this phase.

---

## 10. Success Metrics

**Quantitative (measured in sdd-verify):**
- NFR-1: Poller CPU average ≤ 0.5% on reference hardware over a 5-minute window (Basic and Full). This now **includes the network adapter poller** (`GetIfTable2` snapshot) — the budget covers all providers.
- NFR-2: Default config ships with `poll_interval_seconds = 10`.
- NFR-3: Cold start ≤ 2s (Basic) / ≤ 6s (Full) on reference hardware, p95 over 20 launches.
- NFR-4: Steady-state RSS ≤ 80 MB (Basic) / ≤ 120 MB (Full). *(Note: the bundled SQLite for bandwidth tracking adds ~1 MB to the working set; the 80/120 budgets were set before this amendment and remain achievable. If profiling shows otherwise, raise to 82/122.)*
- NFR-5: Smoke-pass on Win11 24H2 and 25H2.
- NFR-8 *(v2)*: Every UI-displayed value passes through a `format_*` function; raw-value display is gated behind an explicit toggle. Unit-test coverage of `sidebar-domain::format` = 100% of public functions.
- Bandwidth *(v2)*: `cycle_end` pure function passes all edge-case unit tests (28/29/30/31-day months, leap years, "last day of month", year boundary). Rollover survives a simulated app-restart-during-cycle-boundary integration test.
- Test coverage ≥ 80% line coverage in `sidebar-domain` and `sidebar-sensor` (mocked trait surface).

**Qualitative:**
- Manual smoke checklist (transparency on light/dark wallpaper, dock reservation on all four edges, multi-monitor DPI, hotkey click-through, status-pill tooltip accuracy) passes without regressions.
- Two-tier auto-detect correctly falls back to Basic when launched unprivileged on a clean machine, with no error dialog.
- No UAC prompt appears on first launch of a default install (Basic mode).

---

## 11. Development Environment

See `CONTRIBUTING.md` for the contributor setup guide and `docs/backlog/nfr-thresholds.md` T-44 for the prerequisite contract. Summary below.

**System prerequisites** (must pre-exist on any contributor's machine — not relocatable):
- Rust ≥ 1.95 (MSRV forced by `sysinfo` 0.39.3).
- `rustup component add llvm-tools-preview` (required by `cargo-llvm-cov`).
- MSVC Build Tools + Windows SDK (for the `windows` crate FFI link).
- PowerShell 7+, Git for Windows.

**Project-local tooling** (under `tools/`, relocatable):
- `cargo-deny`, `cargo-audit`, `cargo-llvm-cov` (NOT `cargo-tarpaulin` — Linux-only), `cargo-nextest` via `cargo binstall --install-root`.
- `actionlint`, `winget-create`, `sqlite3` via scoop or direct download.

**Activation:** `scripts/env.ps1` prepends `tools/` to PATH. `scripts/verify-dev-env.ps1` (Story 0.7) asserts all prerequisites; used as a CI pre-flight gate.

**Coverage tool note:** The backlog originally specified `cargo-tarpaulin` for coverage (Story 11.2, T-42). **Corrected to `cargo-llvm-cov`** everywhere — tarpaulin uses ptrace and is Linux-only. See T-43.

**Hardware note:** The reference dev environment has integrated AMD graphics and no NVIDIA GPU. NVML-dependent tests (Story 3.2) are `#[ignore]`'d; AMD GPU coverage is via Story 3.6 (OHM Full mode).

---

## 12. Current implementation state and known gaps (2026-07-12)

The integration slice is implemented and verified in the current worktree, but
it is not release-complete until the changes are committed/reviewed and the
manual Windows gates below pass:

- **Runtime hook preservation:** `SidebarApp::run` rebinds the launch callback,
  accountant `BandwidthView` receiver, and OHM liveness probe into the eframe
  app instance created by the native runner.
- **Status-pill launch:** BASIC clicks now send a non-blocking request to the
  supervisor-owner thread, which invokes `launch_elevated` and preserves the
  explicit-user-action/UAC boundary.
- **Bandwidth view and history:** the accountant publishes snapshots through a
  watch channel; each snapshot loads retained SQLite history rows before the
  GUI drains it into `SidebarView`.
- **OHM degradation:** child liveness is polled on the supervisor-owner thread
  and degrades Full→Basic once only when the sidebar explicitly launched that
  child. External LHM remains unowned and does not trigger degradation.
- **Theme and header:** a missing or malformed `AppsUseLightTheme` registry
  value defaults to Dark; the 12.1 header renders a locale-stable `HH:MM`
  clock plus ISO date without network time.
- **Verified evidence:** `cargo test --workspace --all-targets` reports 625
  passed, 0 failed, 13 ignored; formatting and diff checks pass.
- **Remaining delivery gates:** 6.6 Win32 hotkey/monitor/theme smoke, 9.x
  SignPath/release/winget work, 10.1–10.2 NFR and smoke evidence, and 11.x
  regression/coverage gates remain pending. Real UAC/LHM launch, Job Object,
  capture, multi-monitor, and release-walker checks are manual HITL gates.

These are release-verification and delivery gates, not changes to the
Win11-only, Basic no-admin, per-NIC bandwidth product boundary.

## Appendix A — Deferred / Heavy Sources (NFR-1 audit)

Sources evaluated and **excluded** from v1 under the lightweight mandate:

| Source | Why heavy / out-of-scope | Revisit |
|---|---|---|
| `NtQuerySystemInformation(SystemFullProcessInformation)` with full handle/thread enumeration | Disproportionate syscall churn per tick; `sysinfo` already enumerates processes at the level we need | Never — `sysinfo` is the lightweight proxy |
| ETW real-time trace sessions for disk/network | ETW session setup is heavy; buffer management overhead | Post-v1, behind a feature flag |
| Raw MSR reads for CPU temp (via kernel driver) | Requires shipping a kernel driver — completely out of scope | Never in v1.x |
| ~~Per-network-adapter throughput via `GetIfTable2`/PDH `\Network Interface(*)\Bytes Total/sec`~~ | ~~Actually lightweight, but scope-deferred per locked decision (not heavy)~~ | ~~v1.1 candidate~~ — **PROMOTED to IN scope in the v2 amendment (§3 Tier 4).** The implementation uses `GetIfTable2` snapshots and downstream delta accounting. Retained here as a struck-through record of the original deferral decision. |
| WMI `Win32_TemperatureProbe` (MBFM) | Almost universally returns zero/unimplemented on modern boards; unreliable | Never |
| AMD ADX/xNVML wrappers for non-NVIDIA GPUs in-process | Vendor SDK licensing + per-process cost uncertain; OHM covers it | Post-v1 evaluation |

---

## Citations (retrieved 2026-07-07)

1. **MetricsHub LibreHardwareMonitor connector** — https://www.metricshub.com/docs/latest/connectors/librehardwaremonitor — confirms `root\LibreHardwareMonitor` namespace + `SELECT Name FROM WMINET_InstrumentedAssembly` activation probe + metric coverage (cpu/fan/gpu/memory/physical_disk/temperature/voltage).
2. **Sentry Software Hardware Connectors Library v41** — https://www.sentrysoftware.com/docs/hardware-connectors/latest/connectors/MS_HW_LibreHardwareMonitor.html — documentation as of 2026-03-04; confirms same WMI activation + monitor coverage.
3. **LibreHardwareMonitor project** — https://github.com/LibreHardwareMonitor/LibreHardwareMonitor — last commit within past week (per libs.tech/project/99942769/librehardwaremonitor, indexed 2026-06-24); MPL-2.0 license; .NET Framework 4.7.2 + .NET 10.0 targets.
4. **sysinfo crate** — https://docs.rs/sysinfo — v0.39.3; MSRV 1.95; Windows CPU temperature returns empty iterator (Linux hwmon only).
5. **nvml-wrapper crate** — https://docs.rs/nvml-wrapper — v0.12.0 (Mar 2026); `running_processes()` and `process_utilization()` available.
6. **wmi crate** — https://docs.rs/wmi — v0.18.4 (Mar 2026).
7. **starship-battery crate** — https://docs.rs/starship-battery — actively maintained fork (used by starship); Windows 7+ supported.
8. **egui** — https://docs.rs/egui — v0.35.0 latest; `ViewportBuilder::with_transparent` confirmed.
9. **windows crate** — https://docs.rs/windows — v0.62.2.
10. **Microsoft Learn: Code signing options** — https://github.com/MicrosoftDocs/windows-dev-docs/blob/docs/hub/apps/package-and-deploy/code-signing-options.md — ms.date 2026-04-20; Azure Artifact Signing ~$9.99/mo, OV $150-300/yr, EV no longer instant bypass since 2024.
11. **Microsoft Learn: MSIX AppInstaller auto-update** — https://learn.microsoft.com/en-us/windows/msix/app-installer/auto-update-and-repair--overview — ms.date 2026-04-10; `.appinstaller` schema 2021, `HoursBetweenUpdateChecks`, `OnLaunchUpdateCheck`, available on all Win11.
12. ***(v2)* SignPath Foundation — conditions for OSS projects** — https://signpath.org/terms.html — retrieved 2026-07-07. Eligibility: OSI-approved license, public repo, free downloads, no malware/PUPs, no hacking tools, MFA required, code signing policy published. Free cert issued to SignPath Foundation (they are the publisher of record).
13. ***(v2)* SignPath.io — Open Source Community solution** — https://signpath.io/solutions/open-source-community — retrieved 2026-07-07. Free code signing + integrity tools for OSS.
14. ***(v2)* SignPath — Trusted Build Systems: GitHub** — https://docs.signpath.io/trusted-build-systems/github — retrieved 2026-07-07. GitHub Actions integration for OSS signing.
15. ***(v2)* Microsoft Learn: Open a Microsoft Store developer account (Partner Center)** — https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account — retrieved 2026-07-07. *"With the new onboarding experience, there are no registration fees for either account type."* Entry point: storedeveloper.microsoft.com. **India 2026: $0 for Individual and Company.**
16. ***(v2)* Microsoft Learn: Submit to Windows Package Manager (winget)** — https://learn.microsoft.com/en-us/windows/package-manager/package/repository + https://github.com/microsoft/winget-pkgs — retrieved 2026-07-07. PR-based submission; `winget-create` tool; ~1 hour processing.
17. ***(v2)* Azure Trusted Signing pricing** — https://azure.microsoft.com/en-us/pricing/details/artifact-signing/ + https://www.infoworld.com/article/2337355/understanding-microsofts-trusted-signing-service.html — retrieved 2026-07-07. Basic plan $9.99/mo (5,000 sigs), $0.005/sig over quota. **No free tier in 2026.** Rejected by user on cost grounds.
18. ***(v2)* Microsoft Learn: GetIfEntry2 (netioapi.h)** — https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getifentry2 — retrieved 2026-07-07. Fills caller-provided `MIB_IF_ROW2` for a single interface; lighter than `GetIfTable2` (which allocates a full table).
19. ***(v2)* Microsoft Learn: GetIfTable2 (netioapi.h)** — https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getiftable2 — retrieved 2026-07-07. Enumerates all interfaces into `MIB_IF_TABLE2`; caller must `FreeMibTable`. Heavier than per-adapter `GetIfEntry2`.
20. ***(v2)* SQLite for time-series in Rust (guide)** — https://medium.com/rustaceans/harnessing-the-power-of-sqlite-for-time-series-data-storage-in-rust-a-comprehensive-guide-321612470836 — retrieved 2026-07-07. SQLite append-friendly inserts, indexed date-range queries, ACID durability — the rationale for SQLite over TOML for accumulated byte-count state.
21. ***(v2)* HN: JSON Changelog with SQLite (append-only audit log suitability)** — https://news.ycombinator.com/item?id=17855045 — retrieved 2026-07-07. Community consensus that SQLite's journal is suitable for append-only / immutable audit-log patterns.

---

**End of PRD.** Companion document: `architecture.md`.
