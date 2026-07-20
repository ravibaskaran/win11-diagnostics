# Architecture — sidebar (sidebar-v1)

**Change:** `sidebar-v1`
**Phase:** implementation snapshot (Epic 0–8 delivered; closure work pending)
**Status:** Current architecture and known integration gaps (see §14)
**Date:** 2026-07-11 (implementation reconciliation)
**Workspace:** 12-package workspace: 11 libraries + 1 binary
**Companion document:** `PRD.md`

> **What changed in v2 (this amendment).** This is an UPDATE pass; all original architecture content is preserved and extended. Converges with PRD v2. Four amendments:
> 1. **New `sidebar-adapter-net` provider crate** for per-NIC throughput via `GetIfTable2` (now in scope; the implementation enumerates the table and frees it with `FreeMibTable`).
> 2. **New `sidebar-bandwidth` crate** — the `BandwidthAccountant` component + SQLite persistence layer (`bandwidth.db`) + `sidebar-domain::billing` pure functions. This is the architectural reason SQLite enters the stack.
> 3. **`format` module expanded** with explicit `format_bytes` / `format_hz` / `format_bps` / `format_temp` / `format_voltage` / `format_rpm` / `format_power` pure functions (NFR-8).
> 4. **New §11 Build & Release Pipeline** — the zero-cost distribution stack (SignPath + GitHub Releases + winget + optional Microsoft Store). OQ-1 resolved.
> New AD entries: **AD-11** (SQLite for time-series state), **AD-12** (per-NIC identity by LUID), **AD-13** (format module), **AD-14** (distribution stack).

> This architecture converges with the PRD. Same product, same two-tier model, same NFRs, same lightweight mandate. The honest-framing rule applies: **sidebar is Rust-native except for CPU temperature and a small set of low-level sensors, which are sourced from a bundled LibreHardwareMonitor (LHM) subprocess via its HTTP `/data.json` endpoint** (revised 2026-07-08 — was WMI; LHM dropped WMI in v0.9.5+. See AD-2). Nothing in this document claims "pure Rust."

---

## 1. Technical Approach

sidebar is a single-binary Windows 11 desktop application structured as a Cargo workspace with one binary crate and **11 library crates** (12 workspace packages total). The binary (`sidebar-app`) owns the GUI thread, spawns the tokio runtime, drives the sensor poller, bridges to the bundled **LibreHardwareMonitor (LHM)** subprocess when Full mode is active (via its HTTP `/data.json` endpoint on `127.0.0.1:17127` — AD-2, revised 2026-07-08), and *(v2)* hosts the `BandwidthAccountant` task that accumulates monthly per-NIC byte counts to SQLite.

The defining architectural choices are:

1. **egui + eframe** for the GUI — immediate-mode, supports transparent borderless viewports, runs on wgpu, has community precedent for sidebar-style overlays. (egui 0.35.0 latest, retrieved 2026-07-07.)
2. **A `SensorProvider` trait** as the keystone abstraction. Every telemetry source implements it. Domain logic (formatting, smoothing, alerting, aggregation) operates on `Vec<Reading>` produced by the trait, never on concrete adapter types. This is what makes strict TDD feasible for ~80% of the codebase.
3. **A `SensorCostClassifier`** that gates every source at design time against NFR-1. No source ships without a `CostClass` and (for `Watch`/`Lightweight`) profiling evidence.
4. **Two-tier auto-detect** at every launch via an HTTP probe to LHM's `/data.json` endpoint (AD-7). The host process is never auto-elevated; LHM elevation is opt-in via the status-pill button.
5. **Bundled LHM subprocess** for Full mode. The host queries LHM's HTTP endpoint at `http://127.0.0.1:17127/data.json` (default port; T-45); it does not load LHM as a library or reimplement its drivers.
6. **tokio** for async polling. One interval, one tick, all sources polled in parallel within the tick. Results published to GUI via `tokio::sync::broadcast`.
7. ***(v2)* Per-NIC network adapter poller** via `GetIfTable2` (full table snapshot per tick, freed with `FreeMibTable`) — Lightweight cost class. Tier-agnostic (Basic + Full). See `sidebar-adapter-net`.
8. ***(v2)* `BandwidthAccountant`** — a component that subscribes to live network readings, accumulates per-NIC byte deltas into a monthly total, persists to SQLite (`bandwidth.db`), and auto-rolls over at the user-configured billing-cycle boundary. The marquee new feature. See `sidebar-bandwidth`.
9. ***(v2)* SQLite for time-series state, TOML for config** — config stays in `config.toml` (human-editable preferences); accumulated bandwidth state goes to `bandwidth.db` (SQLite, WAL mode, append-friendly, ACID). Two distinct persistence layers with two distinct purposes. See AD-11.
10. ***(v2)* `format` module (NFR-8)** — pure functions `format_bytes` / `format_hz` / `format_bps` / `format_temp` / `format_voltage` / `format_rpm` / `format_power` that default to human-readable output (GHz, GB, Mbps, °C, V, RPM, W). Raw-value display is a toggle. See AD-13.
11. ***(v2)* Zero-cost distribution stack** — SignPath Foundation for free OSS code signing + GitHub Releases for hosting + winget manifest for discoverability + optional Microsoft Store (free Partner Center onboarding, signs via Microsoft). No $120/yr. See §11.

---

## 2. Architecture Decisions

Each decision follows Choice / Alternatives / Rationale.

### AD-1 — GUI framework: egui + eframe

- **Choice:** egui 0.35.0 + eframe (epi) for the desktop shell.
- **Alternatives considered:**
  - *Tauri* — uses a webview; transparency support on Win11 is brittle and we'd ship a full Chromium-content-process (~150 MB RSS), violating NFR-4.
  - *iced* — retained-mode, pure-Rust; transparency + borderless viewport support is less documented than egui for our use case.
  - *slint* — declarative UI DSL; strong option but its commercial-licence terms for closed-source and its younger ecosystem make egui a safer bet.
  - *Win32 + Direct Composition directly* — maximum control, maximum unsafe surface; we'd reimplement what eframe gives us.
  - *egui_overlay* — exists but is a niche wrapper with thin precedent; rejected (R6 in PRD §8).
- **Rationale:** egui is immediate-mode (trivial re-render on every sensor tick), ships transparency via `ViewportBuilder::with_transparent(true)`, has community precedent for sidebar overlays (egui Discussion #2803, #4228), and the whole render path is Rust. The wgpu backend on Win11 hits <80 MB RSS easily.

### AD-2 — OHM bridge: bundled subprocess + HTTP `/data.json` endpoint *(revised 2026-07-08 — was WMI)*

- **Choice:** Bundle **LibreHardwareMonitor v0.9.6** (MPL-2.0, license-compatible; the `.NET 10` build `LibreHardwareMonitor.zip`) alongside the sidebar binary. In Full mode, launch it as a hidden subprocess via `ShellExecuteW("runas")` (UAC only on explicit user action). The sidebar host queries LHM's **HTTP endpoint** at `http://127.0.0.1:<port>/data.json` via the `ureq` crate (2.x, sync, no async runtime required; MIT/Apache-2.0).
- **Port:** **17127** (default), configurable in `[ohm] http_port = 17127`. sidebar writes this port into LHM's `LibreHardwareMonitor.config` before launching, probes it on startup, and falls back to 17128–17137 if 17127 is occupied. See T-45.
- **Why HTTP, not WMI:** LHM dropped WMI output in v0.9.5 (Jan 2026) because .NET 10 removed WMI provider support. The maintainer-confirmed replacement is the HTTP endpoint (issue [#2143](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/2143)): *"Yes, this is removed as .NET 10 doesn't support it anymore. Either integrate the library directly or use the HTTP endpoint."* v0.9.4 (Nov 2024) is the last WMI-publishing build but predates AMD Ryzen AI 300-series / Intel Core Ultra sensor support — unacceptable for the project's reference hardware (Ryzen AI 7 350). HTTP is the future-proof, maintainer-blessed path.
- **HTTP endpoint contract** (verified from `LibreHardwareMonitor.Windows.Forms/Utilities/HttpServer.cs` on master, retrieved 2026-07-08):
  - `GET /data.json` → JSON tree of all sensors (the canonical integration point we use).
  - `GET /metrics` → Prometheus/OpenMetrics format (alternative; not used in v1).
  - `GET /Sensor?action=Get&id=<node-path>` → single sensor REST (not used in v1).
  - Port + bind IP are configurable via `LibreHardwareMonitor.config` (`ListenerPort`, `ListenerIp`).
- **JSON schema (Sketch):** `/data.json` returns a tree of `{ id, text, children, min, value, max, imageindex, type }` nodes — `type ∈ { "Node", "Sensor" }`; sensor leaves carry `value`, `min`, `max`, `text` (name), parent `Node` for hierarchy (e.g. `/amdcpu/0/temperature/0`). The `sidebar-adapter-ohm` crate parses this into `Vec<Reading>` via `serde_json` (deserialization against a `LhmNode` struct, 1 allocation per node — Lightweight per T-1).
- **Why `ureq` (sync) not `reqwest` (async):** The OHM poll is one HTTP GET per tick (10s default); async adds no value at this cadence and would pull in tokio onto the adapter's `spawn_blocking` path. `ureq` has zero deps beyond `rustls`, compiles in <5s, and keeps `sidebar-adapter-ohm` sync — matching the `SensorProvider::read_all` signature.
- **Alternatives considered:**
  - *WMI via `wmi` crate (v0.9.4 net472)* — REJECTED: v0.9.4 predates Ryzen AI 7 sensor support; WMI is permanently removed from .NET 10 builds.
  - *In-process `LibreHardwareMonitorLib` via COM host / `nethost`* — REJECTED: requires loading the CLR into the Rust process; brittle, ABI-fragile, adds .NET 10 runtime as a hard dep.
  - *Prometheus `/metrics` parser instead of `/data.json`* — viable; structured line format. Rejected for v1 because `/data.json` is richer (min/max/sample history) and avoids a Prometheus-format parser dep.
  - *Raw kernel driver for MSR reads* — out of scope; we do not ship a driver.
- **Rationale:** Subprocess isolation means LHM crashes don't take down sidebar. HTTP-on-localhost is simpler and more reliable than WMI-COM from Rust (no COM apartment init per thread, no `wmi::COMConnection` lifecycle). The port-collision fallback (T-45) addresses the maintainer's "can't pick a port that's not occupied" concern. ~8 MB extra install size (the v0.9.6 zip) is acceptable for the sensor coverage gained — including the project's reference hardware.

### AD-3 — Native-Rust sources: sysinfo + nvml-wrapper + starship-battery + windows PDH

- **Choice:** For Basic mode and for Full-mode metrics that don't need OHM, use:
  - `sysinfo` 0.39.3 (MSRV 1.95) — CPU utilization, CPU frequency, RAM, disks (capacity), processes, networks (unused in v1), uptime.
  - `nvml-wrapper` 0.12.0 — NVIDIA GPU metrics.
  - `starship-battery` — battery state.
  - `windows` crate 0.62.2 PDH counters — per-drive R/W throughput via `\PhysicalDisk(*)\Disk Read Bytes/sec` and `\Disk Write Bytes/sec`.
- **Alternatives:** `heim` (unmaintained), `rust-battery` (superseded by `starship-battery`).
- **Rationale:** These are the actively-maintained 2026-vintage crates (versions confirmed retrieved 2026-07-07). All are defensibly lightweight per NFR-1 (no full-process `NtQuerySystemInformation` beyond what `sysinfo` already does once per tick).

### AD-4 — SensorProvider trait as keystone abstraction

- **Choice:** Define `SensorProvider` in `sidebar-sensor`. All adapters implement it. The domain layer depends only on the trait + the `Reading` struct, never on adapter types. `mockall::automock` generates a mock for unit tests.
- **Alternatives:** Concrete-adapter-everywhere (couples domain to IO, untestable without Windows); an async `async-trait` version (adds heap allocation per call; not needed at our 10s cadence).
- **Rationale:** This is the single decision that makes strict TDD feasible. Domain logic (smoothing, alerting, formatting, graph windowing, top-N selection, config defaults) becomes pure functions of `Vec<Reading>`.

### AD-5 — Sensor cost classifier (NFR-1 enforcement)

- **Choice:** Define `SensorCostClassifier` in `sidebar-sensor`. Each `SensorDescriptor` carries a `CostClass`. Adapters cannot register a source without one. CI runs `cargo bench --bench poll_cost` on a Windows runner; the bench fails if any source exceeds 0.5% CPU average over the rolling window.
- **Alternatives:** Trust + post-hoc profiling; or omit the classifier and hope.
- **Rationale:** NFR-1 is a CRITICAL constraint from the user. "If anything is flagged as heavy, do not pick it up." The classifier makes the rule enforceable at design time, not a hope at runtime.

### AD-6 — Async runtime: tokio 1.x

- **Choice:** tokio 1.x multi-threaded runtime, sized to 2 worker threads (one for the poller, one for HTTP/IO and `spawn_blocking` work). GUI runs on the main thread (egui is single-threaded immediate-mode). Poll results published via `tokio::sync::broadcast::Sender<Vec<Reading>>`; the GUI thread holds a `Receiver` and drains on each repaint.
- **Alternatives:** async-std (lower mindshare for our deps); smol (same); pure std::thread + mpsc (works but loses structured concurrency).
- **Rationale:** `sysinfo`, `ureq` (called from `spawn_blocking`), `nvml-wrapper` all play well under tokio. `broadcast` is the right channel for fan-out (multiple subscribers: GUI, optional future logging, optional future alerts).

### AD-7 — Two-tier auto-detect probe *(revised 2026-07-08 — was WMI)*

- **Choice:** On every launch, run a **500 ms-timeout HTTP probe** (`GET http://127.0.0.1:17127/data.json` with `ureq` + `.timeout(Duration::from_millis(500))`). If HTTP 200 → Full mode (LHM is already running and reachable). If connection-refused/timeout → check whether sidebar can launch the bundled LHM (privilege + binary present); if yes, launch it, wait up to T-11 (5s) for the HTTP endpoint to become reachable, re-probe; reachable → Full. Otherwise → Basic silently.
- **Port handling:** Default 17127 per T-45. On launch, sidebar probes 17127; if occupied by something other than LHM (HTTP response is not LHM's JSON signature), tries 17128..17137, picks first free, writes it into `LibreHardwareMonitor.config` (`ListenerPort`) before launching. This deterministically avoids port occupation — the concern raised by the LHM maintainer in issue #2143.
- **Why HTTP, not WMI:** See AD-2 — WMI is permanently removed in LHM v0.9.5+. The HTTP probe is faster (no COM init), has a clean timeout primitive (`ureq::Agent::timeout`), and produces a structured response we already parse.
- **LHM-vs-other discrimination:** The probe isn't just "is something listening?" — it verifies the response body looks like an LHM sensor tree (top-level JSON is an array; first element has `Text` and `Children` keys). A different service returning 200 on 17127 fails this check → treated as occupied → port fallback triggers.
- **Alternatives considered:**
  - *WMI namespace probe (v0.9.4)* — REJECTED per AD-2.
  - *TCP connect-only probe (faster, no HTTP overhead)* — REJECTED: doesn't discriminate LHM from other services; the JSON-signature check is worth the ~2ms cost.
  - *A Settings toggle* — REJECTED by user (auto-detect only, per PRD §5.2).
  - *Caching the tier result on disk* — REJECTED: privileges and LHM presence change between launches.
- **Rationale:** No user friction. Always reflects current reality. The HTTP probe + JSON-signature check is more discriminating than the old WMI namespace probe. Matches the user's PRD §5.2 auto-detect mandate.

### AD-8 — OHM subprocess management

- **Choice:** `sidebar-platform` owns an `OhmSupervisor` that:
  1. Checks if LHM is already running (by process name + by HTTP `/data.json` reachability on port 17127 per AD-7).
  2. If not and Full mode is desired, **writes the chosen HTTP port (T-45) into `LibreHardwareMonitor.config`** before launching, then launches `LibreHardwareMonitor.exe` from the install directory via `ShellExecuteW("runas", ..., SW_HIDE)`.
  3. Monitors the child handle; on LHM crash, marks Full mode degraded, falls back to Basic for the remainder of the session, surfaces in the status pill.
  4. On sidebar shutdown, terminates LHM **only if sidebar launched it** (don't kill a user-started LHM).
- **Alternatives:** Let LHM auto-start via a Windows service (out of scope; adds service-install complexity); require the user to start LHM manually (rejected — too much friction).
- **Rationale:** Subprocess isolation + clear ownership semantics + graceful degradation. The "don't kill user-started LHM" rule prevents us from interfering with power users who run LHM separately. Writing the port into config before launch is what makes the port-collision fallback (T-45) deterministic. Child-exit monitoring is an exposed callback seam; the app-level monitor/degrade loop remains a wiring task (see §14).

### AD-9 — Config format: TOML

- **Choice:** TOML at `%APPDATA%\sidebar\config.toml` via `toml` + `serde`. Schema is a single `Config` struct with versioned migrations (`config_version = 1`).
- **Alternatives:** JSON (less human-friendly); YAML (gotchas with implicit typing); RON (niche).
- **Rationale:** TOML is the Rust-ecosystem default for app config. Human-editable, schema-able, well-supported.

### AD-10 — Crate layout

See §4.

### AD-11 — Persistence: SQLite for time-series state, TOML for config *(v2 amendment — NEW)*

- **Choice:** Two distinct persistence layers:
  - **Config** → `%APPDATA%\sidebar\config.toml` (TOML via `toml` + `serde`). Unchanged from AD-9. Holds preferences: `poll_interval_seconds`, theme, docked edge, `cycle_start_day`, tracked-NIC list, display toggles (°C/°F, raw-values, decimal/binary). Small (~1 KB), human-editable, versioned with `config_version`.
  - **Time-series state** → `%APPDATA%\sidebar\bandwidth.db` (SQLite via `rusqlite` 0.32+, bundled `sqlite3.dll` ~1 MB, WAL journal mode). Holds accumulated monthly byte counts per-NIC + rollover history.
- **Alternatives considered:**
  - *TOML for everything* (rewrite bandwidth state to `bandwidth.toml` on every tick). **Rejected:** TOML requires full-file reparse + rewrite on each append; at 10s cadence over months of uptime this is wasteful, and a crash mid-rewrite corrupts the whole file. TOML has no transactional safety, no indexed range queries, and grows linearly with no compaction.
  - *JSON / JSONL append file* (one line per flush). Viable for append-only, but reading history requires a full scan; partial writes on crash leave a malformed trailing line; no schema, no migration path.
  - *sled / redb / rustlite / other embedded Rust DBs.* Viable but younger ecosystems, less tooling, harder to inspect with `sqlite3` CLI during debugging. SQLite is the boring, universally-understood choice.
  - *In-memory only (no persistence).* Violates the user requirement that bandwidth totals survive restart/reboot/sleep.
- **Rationale:** The monthly bandwidth feature is the architectural reason SQLite enters the stack. SQLite in WAL mode gives: (a) append-friendly inserts that don't block the poller, (b) ACID durability against crash/sleep, (c) indexed date-range queries for the history table, (d) trivial schema migration via `user_version` PRAGMA, (e) crash-recovery via journal rollback on the rare partial write. The ~1 MB bundled-binary cost and ~1 MB RSS overhead are acceptable within NFR-4 (80/120 MB budget; SQLite sits comfortably under the headroom). Community consensus affirms SQLite for append-only / audit-log patterns in Rust (sources in PRD §Citations 20–21). Schema:
  ```sql
  -- bandwidth.db
  CREATE TABLE current_cycle (
      adapter_luid   INTEGER PRIMARY KEY,   -- MIB_IF_ROW2.InterfaceLuid
      adapter_name   TEXT NOT NULL,          -- friendly name snapshot
      cycle_start    TEXT NOT NULL,          -- ISO date 'YYYY-MM-DD'
      rx_bytes       INTEGER NOT NULL DEFAULT 0,
      tx_bytes       INTEGER NOT NULL DEFAULT 0,
      updated_at     TEXT NOT NULL           -- ISO timestamp
  );
  CREATE TABLE bandwidth_history (
      rowid          INTEGER PRIMARY KEY AUTOINCREMENT,
      adapter_luid   INTEGER NOT NULL,
      adapter_name   TEXT NOT NULL,
      cycle_start    TEXT NOT NULL,
      cycle_end      TEXT NOT NULL,
      rx_bytes       INTEGER NOT NULL,
      tx_bytes       INTEGER NOT NULL,
      archived_at    TEXT NOT NULL
  );
  CREATE INDEX idx_history_luid_cycle ON bandwidth_history(adapter_luid, cycle_start);
  CREATE TABLE current_cycle_metadata (
      id               INTEGER PRIMARY KEY CHECK (id = 1),
      cycle_start      TEXT NOT NULL,
      cycle_start_rule TEXT NOT NULL
  );
  PRAGMA user_version = 2;   -- schema version, migrate via PRAGMA
  PRAGMA journal_mode = WAL;
  ```
  *(v1 retention: history pruned to current + previous cycle on each rollover. Older rows are deleted; v1.1 may extend retention.)*

### AD-12 — Per-NIC identity by LUID *(v2 amendment — NEW)*

- **Choice:** Track network adapters by their **LUID** (`MIB_IF_ROW2.InterfaceLuid`, a 64-bit Locally Unique Identifier), NOT by name and NOT by index.
- **Alternatives considered:**
  - *By name* (`InterfaceAlias`, e.g. "Ethernet"). **Rejected:** users rename adapters ("Ethernet" → "WAN"); names are localized (a French Windows shows "Connexion réseau"); names collide.
  - *By index* (`InterfaceIndex`). **Rejected:** indexes reshuffle across reboots and dock/undock events; not stable.
  - *By MAC address.* Viable fallback if LUID proves unstable in sdd-verify. Less stable for virtual adapters (Hyper-V vSwitches, VPN tunneis, container vNICs may share/spoof MACs), but acceptable.
- **Rationale:** Windows guarantees LUID stability across reboots per the IP Helper contract (`InterfaceLuid` is documented as persistent for the lifetime of the adapter installation). This is the documented guarantee that makes long-term per-NIC bandwidth tracking feasible. If an adapter disappears (undocked), its `current_cycle` row is retained but frozen (no deltas added); if it reappears, accumulation resumes against the same row. PRD R10 documents the fallback to MAC if sdd-verify disproves LUID stability.

### AD-13 — `format` module for human-readable output (NFR-8) *(v2 amendment — NEW)*

- **Choice:** A pure `format` module in `sidebar-domain::format` exposing:
  ```rust
  pub fn format_hz(hz: u64) -> String;              // 3_840_000_000 -> "3.84 GHz"
  pub fn format_bytes(bytes: u64, base: Base) -> String; // Base::Decimal -> "1.84 TB", Base::Binary -> "1.67 TiB"
  pub fn format_bps(bps: u64) -> String;            // 48_200_000 -> "48.2 Mbps"
  pub fn format_temp(celsius: f64, unit: TempUnit) -> String; // TempUnit::Celsius -> "62 °C", ::Fahrenheit -> "144 °F"
  pub fn format_voltage(volts: f64) -> String;      // 1.248 -> "1.248 V"
  pub fn format_rpm(rpm: u32) -> String;            // 1840 -> "1840 RPM"
  pub fn format_power(watts: f64) -> String;        // 45.2 -> "45.20 W"
  pub fn format_percent(pct: f64) -> String;        // 42.0 -> "42%"
  pub fn format_battery(pct: u8, state: BatteryState) -> String; // 78 + Charging -> "78% (Charging)"
  pub enum Base { Decimal, Binary }     // 10^9 vs 2^30
  pub enum TempUnit { Celsius, Fahrenheit }
  ```
  All functions are pure (no IO, no global state), fully unit-testable, locale-stable in v1 (`.` decimal separator, no thousands separator). A future `Locale` parameter can be added without API breakage (see PRD OQ-5).
- **Alternatives:** Per-call ad-hoc formatting inline in the GUI render code. **Rejected:** untestable, inconsistent, drifts between rows. Centralizing in a pure module is what makes NFR-8 verifiable.
- **Rationale:** NFR-8 mandates human-readable-by-default output. Centralizing formatting in a pure module makes the rule enforceable (every GUI display call site goes through `format_*`), fully unit-testable (every `MetricKind × Unit × Base/TempUnit` combination), and future-proof (locale, decimal/binary, °C/°F toggles are parameter swaps, not refactors).

### AD-14 — Distribution stack: zero-cost-first *(v2 amendment — NEW)*

- **Choice:** Zero-cost distribution: **SignPath Foundation** (free OSS code signing) + **GitHub Releases** (free binary hosting) + **winget** manifest PR (free discoverability) + **optional Microsoft Store** (free Partner Center onboarding, signs via Microsoft). Total annual cost: $0.
- **Alternatives considered:** Azure Trusted Signing (~$9.99/mo ≈ $120/yr) — rejected by user on cost grounds (India pricing context); OV cert from DigiCert/Sectigo ($150–300/yr) — rejected on cost grounds; self-signed only — bad UX (SmartScreen hard-blocks), kept as a fallback only if SignPath is denied. See PRD §9 OQ-1 for the full decision matrix.
- **Rationale:** The user's hard constraint is zero cost. The 2026 research (retrieved 2026-07-07) surfaced three facts that make this viable: (a) SignPath Foundation offers free OSS code signing for OSI-licensed projects built via trusted CI, (b) Microsoft's new Partner Center onboarding flow charges **$0 for both Individual and Company accounts** (eliminating the legacy ~$19/$99 fees), (c) winget submission is a free PR to `microsoft/winget-pkgs`. The bundled OHM.exe remains unsigned (SignPath signs our binary only) — this matches every other OHM consumer. See §11 for the build/release pipeline integration.

---

## 3. Data Flow Diagram

```
                        ┌──────────────────────────────────────────────────────────────┐
                        │                    sidebar-app (binary)                        │
                        │                                                              │
                        │   ┌─────────────────┐         ┌──────────────────────────┐    │
                        │   │   GUI thread     │         │   tokio runtime (2 thr)   │   │
                        │   │   (egui/eframe)  │         │                          │    │
                        │   │                  │  Vec<   │  ┌────────────────────┐  │    │
                        │   │  AppState:       │◀Reading│  │  Poller task        │  │    │
                        │   │  - readings:     │  via    │  │  interval=10s       │  │    │
                        │   │    Arc<RwLock<   │ broadcast│ │  tokio::select! {   │  │    │
                        │   │     Snapshot>>   │ channel │  │    provider.read()  │  │    │
                        │   │  - config        │         │  │  }                  │  │    │
                        │   │  - tier: Basic/  │         │  └─────────┬──────────┘  │    │
                        │   │    Full          │         │            │             │    │
                        │   │  - status_pill   │         │  ┌─────────▼──────────┐  │    │
                        │   └────────┬─────────┘         │  │ SensorProvider     │  │    │
                        │            │                   │  │ registry           │  │    │
                        │            │                   │  └─────────┬──────────┘  │    │
                        │            │                   │            │             │    │
                        │            ▼                   │   ┌────────┼─────────┐   │    │
                        │    Win32 viewport              │   ▼        ▼         ▼   │    │
                        │    (transparent, topmost,      │ sysinfo  nvml    starship │    │
                        │     docked AppBar)             │ adapter  adapter battery │   │
                        │                                │   │       │         │    │    │
                        └────────────────────────────────┘   │       │         │    │    │
                                                             ▼       ▼         ▼    │    │
                                              ┌─────────────────────────────────┐   │    │
                                              │  sidebar-adapter-ohm (Full mode) │◀──┘    │
                                              │  ureq → GET /data.json           │        │
                                              │  http://127.0.0.1:17127/...      │        │
                                              └──────────────┬──────────────────┘        │
                                                             │ HTTP (localhost)           │
                                                             ▼                           │
                                              ┌──────────────────────────────────┐       │
                                              │  LibreHardwareMonitor.exe        │       │
                                              │  (LHM subprocess, elevated,      │       │
                                              │   HTTP server on port 17127)     │       │
                                              └──────────────────────────────────┘       │
                                                                                         │
                                              ┌──────────────────────────────────┐       │
                                              │  sidebar-platform::OhmSupervisor  │◀──────┘
                                              │  (launch/monitor bundled LHM.exe,│
                                              │   write port to LHM.config,      │
                                              │   Job-Object reap on host crash) │
                                              └──────────────────────────────────┘
```

**Key flows:**
- **A.** Poller task fires every 10s (configurable). It runs each provider on Tokio's blocking pool, waits on a shared 100 ms deadline, skips timed-out/panicking providers, and never queues overlapping ticks (`MissedTickBehavior::Delay`).
- **B.** Each adapter returns `Vec<Reading>`. The poller concatenates into a single `Vec<Reading>`, stamps a timestamp, and publishes via `broadcast`.
- **C.** GUI thread drains its receiver on each egui repaint request. Updates `AppState.readings` (behind `Arc<RwLock<Snapshot>>`). egui re-renders.
- **D.** On launch, `OhmSupervisor::probe()` runs the HTTP `/data.json` reachability check on port 17127 (AD-7). Sets `AppState.tier`. If Full mode desired but LHM not running and user has not consented, tier = Basic with pill tooltip explaining why.
- **E.** User flow: status-pill click → supervisor-owner request → `OhmSupervisor::launch_elevated()` writes port to LHM config → `ShellExecuteW("runas")` → LHM starts → wait up to T-11 (5s) → re-probe HTTP → tier = Full. The callback is preserved through eframe app creation; real UAC/LHM behavior remains a §14 manual gate.
- **F. *(v2)*** The `BandwidthAccountant` holds its own `broadcast::Receiver<Vec<Reading>>`. On each tick, it filters for `MetricKind::NetRxBytes` / `NetTxBytes` readings, computes per-LUID deltas from the previous tick, and adds them to the in-memory `MonthlyAccumulator`. Every ~60s (debounced), on graceful shutdown, and on rollover, it flushes to `bandwidth.db` (SQLite WAL).
- **G. *(v2)*** On each tick, `BandwidthAccountant` checks `Local::today() >= current_cycle_end`. Rollover flushes the old in-memory tail, archives `current_cycle` transactionally, prunes history, resets the accumulator, advances `cycle_start`, and persists the new-cycle state.
- **H. *(v2)*** The GUI bridge is a separate `BandwidthView` payload (derived state, not raw readings). The current worktree publishes live snapshots, including retained history, over a watch channel; native visual acceptance remains a §14 manual gate.
- **I. *(v2)*** The `sidebar-adapter-net` provider snapshots `GetIfTable2` rows and emits raw `InOctets`/`OutOctets` counters; the accountant computes deltas and monthly totals, keeping the adapter stateless.

---

## 4. Crate / Module Structure

Cargo workspace, **12 packages (11 libraries + 1 binary)**. The tree below is the intended module map; implementation status is recorded in §13–§14.

```
sidebar/                                    # workspace root
├── Cargo.toml                              # [workspace] members, shared deps
├── PRD.md                                  # ✓ exists (this phase)
├── architecture.md                         # ✓ exists (this phase)
├── crates/
│   ├── sidebar-domain/                     # PURE: no IO, no OS deps
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── reading.rs                  # Reading, SensorId, MetricKind, Unit
│   │       ├── snapshot.rs                 # Snapshot (Vec<Reading> + timestamp + tier)
│   │       ├── smooth.rs                   # EWMA smoother (pure fn)
│   │       ├── alert.rs                    # threshold breach detection (pure fn)
│   │       ├── format.rs                   # Unit-aware formatting (pure fn) — EXPANDED v2
│   │       ├── graph.rs                    # rolling-window sparkline (pure fn)
│   │       ├── aggregate.rs                # top-N process selection (pure fn)
│   │       ├── billing.rs                  # *(v2)* cycle_end, cycle_start arithmetic (pure fn)
│   │       └── config.rs                   # Config struct + defaults + migration
│   ├── sidebar-sensor/                     # SensorProvider trait + cost classifier
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs                 # SensorProvider trait (+ mockall::automock)
│   │       ├── descriptor.rs               # SensorDescriptor, CostClass
│   │       └── classifier.rs               # SensorCostClassifier (const-eval where poss.)
│   ├── sidebar-adapter-sysinfo/            # CPU util/freq, RAM, disks, processes, uptime
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── sidebar-adapter-nvml/               # NVIDIA GPU
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── sidebar-adapter-battery/            # starship-battery
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── sidebar-adapter-ohm/                # LHM HTTP bridge (Full mode) — revised 2026-07-08
│   │   ├── Cargo.toml                      #   ureq + serde_json (NOT wmi)
│   │   └── src/lib.rs                      #   GET http://127.0.0.1:17127/data.json → Vec<Reading>
│   ├── sidebar-adapter-pdh/                # Per-drive throughput via PDH
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── sidebar-adapter-net/                # *(v2)* Per-NIC throughput via GetIfTable2
│   │   ├── Cargo.toml                      #   windows crate, MIB_IF_TABLE2 snapshot
│   │   └── src/lib.rs                      #   emits NetRxBytes/NetTxBytes/NetRxPps/...
│   ├── sidebar-persistence/                # *(v2)* SQLite bandwidth state store
│   │   ├── Cargo.toml                      #   rusqlite (bundled), WAL mode
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── schema.rs                   # CREATE TABLE current_cycle/history, PRAGMAs
│   │       ├── bandwidth_repo.rs           # load/save accumulator, archive cycle, prune
│   │       └── migrate.rs                  # user_version schema migrations
│   ├── sidebar-bandwidth/                  # *(v2)* BandwidthAccountant (domain orchestrator)
│   │   ├── Cargo.toml                      #   depends on sidebar-domain + sidebar-persistence
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── accountant.rs               # tokio task: subscribe to readings, accumulate, flush, rollover
│   │       ├── accumulator.rs              # MonthlyAccumulator (per-LUID rx_bytes/tx_bytes)
│   │       └── view.rs                     # BandwidthView (DTO for GUI: current + history)
│   ├── sidebar-platform/                   # Win32: window, AppBar, DWM, OhmSupervisor
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── appbar.rs                   # SHAppBarMessage
│   │       ├── dwm.rs                      # DWM peek exclusion + SetWindowDisplayAffinity capture exclusion
│   │       ├── window.rs                   # HWND_TOPMOST, WS_EX_TRANSPARENT toggle
│   │       ├── dpi.rs                      # SetProcessDpiAwarenessContext
│   │       └── ohm_supervisor.rs           # launch/monitor bundled LibreHardwareMonitor.exe (HTTP, port 17127)
│   └── sidebar-app/                        # BINARY: gui + runtime wiring
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs                     # entry, runtime, AppState wiring
│       │   ├── gui.rs                      # eframe::App impl, immediate-mode render
│       │   ├── gui/
│       │   │   ├── mod.rs
│       │   │   ├── status_pill.rs          # Basic/Full pill + tooltip
│       │   │   ├── metric_row.rs           # one row per sensor (uses format::*)
│       │   │   ├── bandwidth_panel.rs      # *(v2)* monthly GB + days-until-reset + history
│       │   │   └── settings_panel.rs       # config editor (incl. cycle_start_day, temp unit, raw toggle)
│       │   ├── poller.rs                   # tokio interval task, broadcast publish
│       │   └── provider_registry.rs        # builds Vec<Box<dyn SensorProvider>> by tier
│       └── tests/
│           └── smoke.rs                    # integration: launches app headless-ish
├── benches/
│   └── poll_cost.rs                        # NFR-1 enforcement bench (Windows CI only)
├── resources/
│   └── LibreHardwareMonitor.exe            # bundled LHM v0.9.6 binary (MPL-2.0, unsigned — signed upstream by maintainers)
├── .github/workflows/
│   ├── ci.yml                              # Windows runner: test + bench + clippy + fmt
│   └── release.yml                         # *(v2)* build → SignPath sign → GitHub Release + winget PR
```

---

## 5. Interfaces (Rust trait sketches)

### 5.1 Core types (`sidebar-domain::reading`)

```rust
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    CpuUtilization,
    CpuFrequency,
    CpuTemperature,
    CpuPower,
    FanSpeed,
    Voltage,
    GpuUtilization,
    GpuTemperature,
    GpuMemoryUtilization,
    GpuPower,
    GpuFanSpeed,
    GpuFrequency,
    MemoryUsed,
    MemoryTotal,
    DiskUsed,
    DiskTotal,
    DiskReadBytesPerSec,
    DiskWriteBytesPerSec,
    DiskSmartEndurance,      // Full only
    DiskTemperature,         // Full only
    // --- v2 amendment: network + bandwidth ---
    NetRxBytes,              // cumulative counter (InOctets); delta computed downstream
    NetTxBytes,              // cumulative counter (OutOctets)
    NetRxPackets,            // cumulative (InUcastPkts + InNUcastPkts)
    NetTxPackets,            // cumulative
    NetRxErrors,             // cumulative (InErrors)
    NetTxErrors,             // cumulative (OutErrors)
    BandwidthRxBytes,        // derived: accumulated monthly RX bytes per-LUID (from BandwidthAccountant)
    BandwidthTxBytes,        // derived: accumulated monthly TX bytes per-LUID
    // --- end v2 ---
    BatteryPercent,
    BatteryState,            // Charging/Discharging/Idle
    BatteryPowerRate,
    ProcessCpuPercent,
    ProcessMemoryBytes,
    ProcessGpuPercent,       // Watch cost class
    UptimeSeconds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Celsius,
    Fahrenheit,
    Kelvin,
    Hertz,
    Bytes,
    BytesPerSec,
    Watts,
    Volts,
    Rpm,
    Seconds,
    Count,
    // --- v2 amendment ---
    BitsPerSec,              // for formatted network throughput display (Mbps/Gbps)
    PacketsPerSec,
}

/// Stable identifier for a sensor instance.
/// e.g. SensorId::new("cpu", "package") / SensorId::new("gpu/0", "nvidia") / SensorId::new("drive", "C:")
/// *(v2)* network adapters use category "net" with instance = LUID as decimal string
///        (e.g. SensorId::new("net", "1294567890123456")). The friendly name is looked up
///        separately; the LUID is the stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SensorId {
    pub category: &'static str,   // "cpu", "cpu/core", "gpu", "ram", "drive", "net", "battery", "process"
    pub instance:  String,        // "package", "0", "nvidia", "C:", "1294567890123456" (luid), "1234" (pid)
}

#[derive(Debug, Clone)]
pub struct Reading {
    pub sensor:    SensorId,
    pub kind:      MetricKind,
    pub value:     f64,
    pub unit:      Unit,
    pub timestamp: Instant,
}
```

### 5.2 SensorProvider trait (`sidebar-sensor::provider`)

```rust
use sidebar_domain::reading::Reading;

/// One telemetry source. Implementations live in sidebar-adapter-*.
/// Send + Sync so the poller can hold them behind Arc<dyn SensorProvider>.
#[cfg_attr(test, mockall::automock)]
pub trait SensorProvider: Send + Sync {
    /// Human-readable name + cost class + supported metric kinds.
    fn descriptor(&self) -> &SensorDescriptor;

    /// Poll this source once. Called on every tick.
    /// Implementations should be cheap (NFR-1) and non-blocking;
    /// use `tokio::task::spawn_blocking` if the underlying call is sync-syscall heavy.
    fn read_all(&self) -> Vec<Reading>;
}
```

***(v2) Counter vs. rate semantics.*** Two flavors of `Reading` flow through this trait:
- **Gauge readings** (most sensors): `value` is the current instantaneous measurement (e.g. CPU utilization %, temperature °C, disk bytes/sec already delta'd by the adapter). These are displayed directly.
- **Cumulative-counter readings** (network adapter byte/packet/error counts): `value` is the **raw OS counter** (e.g. `InOctets` since adapter up). These are **NOT** displayed directly. The `BandwidthAccountant` (and, for live throughput, a delta-and-divide step in the domain layer) consumes these to produce (a) bytes/sec via `(current - previous) / tick_seconds`, and (b) accumulated monthly totals via `current - cycle_start_baseline`. The `Reading` for a cumulative counter carries its raw value; downstream consumers know from `MetricKind` (e.g. `NetRxBytes` is documented as cumulative) to delta it. This keeps adapters thin (no state, no delta logic) and concentrates the arithmetic in pure, testable domain code.

### 5.3 Sensor descriptor + cost class (`sidebar-sensor::descriptor`)

```rust
use sidebar_domain::reading::MetricKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostClass {
    /// Profiling evidence: < 0.1% CPU avg per tick. Ship unconditionally.
    Lightweight,
    /// Profiling evidence: 0.1–0.5% CPU avg per tick. Ship, but bench in CI.
    Watch,
    /// > 0.5% CPU avg OR disproportionate syscall churn. DO NOT ship in v1.
    Heavy,
    /// Lightweight by measurement, but out of v1 scope (e.g., network adapters).
    Deferred,
}

#[derive(Debug, Clone)]
pub struct SensorDescriptor {
pub name:          &'static str,             // "sysinfo-cpu", "ohm-cpu-temp", "net-getiftable2", etc.
    pub cost_class:    CostClass,
    pub metrics:       &'static [MetricKind],    // what this provider emits
    pub requires_tier: Tier,                     // Basic, Full, or Both (v2: network + bandwidth are tier-agnostic)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Basic,
    Full,
    Both,   // *(v2)* tier-agnostic — provider runs regardless of Basic/Full (e.g. network, bandwidth)
}
```

### 5.4 Sensor cost classifier (`sidebar-sensor::classifier`)

```rust
/// Compile-time gate. Called by provider_registry when building the poller's provider list.
/// Filters out Heavy + Deferred sources. Emits a tracing::warn! for each rejected source
/// so the orchestrator can audit which sources were dropped.
pub fn classify_for_v1(descriptors: &[SensorDescriptor], active_tier: Tier) -> Vec<&SensorDescriptor> {
    descriptors
        .iter()
        .filter(|d| matches!(d.cost_class, CostClass::Lightweight | CostClass::Watch))
        // *(v2)* tier filtering: a provider runs if its required_tier is Basic, Full (when active),
        //        or Both (tier-agnostic — always runs). Basic providers also run in Full mode
        //        (Full is a superset).
        .filter(|d| match (d.requires_tier, active_tier) {
            (Tier::Both, _) => true,
            (Tier::Basic, _) => true,           // Basic providers run in both modes
            (Tier::Full, Tier::Full) => true,
            (Tier::Full, _) => false,
        })
        .collect()
}
```

The classifier is intentionally trivial — the discipline is in the **adapter authors** being required to assign a `CostClass` with profiling evidence (a comment pointing to the bench result). *(v2)* The tier-filtering step was added to express that `sidebar-adapter-net` is tier-agnostic (`Tier::Both`) — it runs in Basic and Full because the IpHelper `GetIfTable2` snapshot is a user-mode API with no elevation requirement.

---

## 6. Threading Model

```
Process boundary
│
├── Main thread ─────────── eframe event loop (egui immediate-mode)
│                            owns AppState (Arc<RwLock<Snapshot>>)
│                            owns broadcast::Receiver<Vec<Reading>>
│                            owns the optional BandwidthView payload (watch bridge from accountant)
│                            repaints on egui's vsync + on broadcast recv
│
├── tokio worker 1 ──────── Poller task
│                            interval tick every config.poll_interval_seconds
│                            spawn_blocking per provider + shared 100ms deadline
│                            publishes Vec<Reading> via broadcast::Sender
│
├── tokio worker 2 ──────── IO-heavy adapters can spawn_blocking here
│                            (HTTP/ureq calls, nvml calls)
│                            *(v2)* sidebar-persistence SQLite flushes (debounced, non-blocking in WAL)
│
├── OhmSupervisor child handles (Full mode only)
│                             Job Object reaps owned LHM on crash/shutdown
│                             app-level child monitor/degrade callback (only for sidebar-owned child)
│
└── (spawned, *(v2)*) BandwidthAccountant task
                             holds broadcast::Receiver<Vec<Reading>>
                             on each tick: filter NetRxBytes/NetTxBytes, delta vs prev, accumulate per-LUID
                             every ~60s (debounced): flush to SQLite (spawn_blocking on worker 2)
                             on rollover (Local::today() >= cycle_end): flush + archive + reset + persist
                             on shutdown signal: force-flush
                             publishes BandwidthView snapshots (including retained history) via watch
```

- **Single source of truth for "current readings":** `AppState.readings` behind `Arc<RwLock<Snapshot>>`. Only the GUI thread writes; the poller publishes via broadcast, GUI drains into the RwLock.
- ***(v2)* Bandwidth state boundary:** `BandwidthAccountant` owns the SQLite-backed accumulator and publishes a derived `BandwidthView` over a watch channel; the GUI drains it without touching SQLite.
- **Tier changes** (Basic↔Full mid-session) are broadcast on the same channel as a special `Event::TierChanged(Tier)` variant. GUI updates the status pill atomically. *(Network + bandwidth providers are `Tier::Both` — unaffected by tier changes.)*
- **No blocking calls on the GUI thread.** egui repaint is ~16ms; we never call `sysinfo`/`nvml`/HTTP/SQLite from the render path.
- **Shutdown:** `Ctrl+C`/GUI close → shared `ShutdownSignal` cancels workers and emits one `Event::Shutdown` → bounded force-flush (≤500 ms) → owned-LHM teardown (≤2 s cumulative) → bounded joins. Production starts a 3 s watchdog that force-exits if the total budget is exceeded; SQLite WAL makes completed flushes durable.

---

## 7. Testing Strategy

Strict TDD is **ENABLED**. Feasible coverage target: **~80% line coverage** across `sidebar-domain` + `sidebar-sensor`. Adapter crates are integration-tested on Windows CI; GUI is manual smoke.

### 7.1 Unit tests (run everywhere, no Windows required)

- **`sidebar-domain`** — pure functions, exhaustively tested:
  - `smooth::ewma` — known input sequence → expected smoothed output.
  - `alert::check_threshold` — below/at/above threshold edge cases, hysteresis.
  - `format::format_reading` *(v2 expanded)* — every `MetricKind × Unit` combination, locale-stable. Specific cases: `format_hz(3_840_000_000) == "3.84 GHz"`, `format_bytes(1_840_000_000_000, Decimal) == "1.84 TB"`, `format_bytes(1_840_000_000_000, Binary) == "1.67 TiB"`, `format_bps(48_200_000) == "48.2 Mbps"`, `format_temp(62.0, Celsius) == "62 °C"`, `format_temp(62.0, Fahrenheit) == "144 °F"`, `format_voltage(1.248) == "1.248 V"`, `format_rpm(1840) == "1840 RPM"`, `format_power(45.2) == "45.20 W"`, `format_percent(42.0) == "42%"`. Round-trip + boundary (0, u64::MAX, f64::NAN) tests.
  - `graph::rolling_window` — window sliding, eviction, overflow.
  - `aggregate::top_n` — stable sort by CPU/RAM, ties, N > input length.
  - `config::migrate` — v0→v1 migration, missing fields → defaults. *(v2)* now includes `[bandwidth]` section defaults (`cycle_start_day = 1`).
  - **`billing::cycle_end` *(v2, NEW)*** — exhaustive edge cases: 28/29/30/31-day months, Feb 28 vs Feb 29 (leap year), "last day of month" selection, year boundary (Dec 31 → Jan 1), `cycle_start_day` values 1–28 + "last". Property-based tests (`proptest`) generate random valid `(start_day, year, month)` triples and assert invariants (cycle_end > cycle_start; cycle_end - cycle_start ∈ [27, 31] days; next cycle_start = cycle_end + 1 day).
  - **`billing::next_cycle_start` *(v2, NEW)*** — computes the start of the cycle after a given date.
- **`sidebar-sensor`** — trait surface tested via `mockall::automock`:
  - `MockSensorProvider::read_all` returns canned `Vec<Reading>`; verify the poller concatenates, timestamps, and publishes correctly.
  - `classify_for_v1` *(v2)* — rejects Heavy and Deferred, accepts Lightweight and Watch; tier-filtering: `Tier::Both` providers run in both modes, `Tier::Basic` runs in both, `Tier::Full` runs only when active_tier=Full.
- **`sidebar-bandwidth` *(v2, NEW)*** — the accountant is a domain orchestrator with an in-memory `MonthlyAccumulator`; persistence is mocked via a trait:
  - `accumulator::add_delta` — per-LUID rx/tx byte deltas accumulate correctly; counter wraparound (current < previous) is detected and handled (treat as if counter reset to 0 then counted up).
  - `accountant::rollover` — feed a sequence of readings spanning a cycle boundary; assert the current cycle is archived, a new cycle starts, totals reset to 0 (or to the bytes accumulated after the boundary).
  - `accountant::tick` — feed readings including non-network metrics; assert only `NetRxBytes`/`NetTxBytes` are consumed.
  - `view::build` — accumulator + history → `BandwidthView` DTO for GUI.
- **`sidebar-persistence` *(v2, NEW)*** — tested against a temp-file SQLite (not the real `%APPDATA%` path):
  - `schema::init` — fresh DB creates tables (including `current_cycle_metadata`) + PRAGMAs (`user_version=2`, `journal_mode=WAL`).
  - `bandwidth_repo::save_and_load` — round-trip accumulator state.
  - `bandwidth_repo::archive_cycle` — current cycle row → history table; current cycle row reset.
  - `bandwidth_repo::prune_history` — keeps only current + previous cycle; older rows deleted.
  - `migrate::v0_to_v1` + `migrate::v1_to_v2` — empty DB → v2 schema; legacy v1 DB → v2 metadata addition.

### 7.2 Integration tests (Windows CI runner only, `#[cfg(target_os = "windows")]`)

- **Adapter smoke tests:**
  - `sidebar-adapter-sysinfo` — `read_all()` returns non-empty CPU util + RAM readings on the CI runner.
  - `sidebar-adapter-nvml` — skipped if no NVIDIA GPU (CI runner may lack one; mark `#[ignore]`).
  - `sidebar-adapter-ohm` — gated on OHM being installed on the CI runner; otherwise `#[ignore]`.
  - `sidebar-adapter-pdh` — PDH counter returns non-zero R/W bytes/sec under a synthetic disk load.
- **`sidebar-adapter-net` *(v2, NEW)*** — `GetIfTable2` returns non-zero `InOctets`/`OutOctets` for the primary adapter; counters are monotonically non-decreasing across two ticks; LUID is stable across two reads within one process.
- **OHM supervisor:** round-trip launch/probe/terminate against the bundled OHM binary.
- **`sidebar-bandwidth` end-to-end *(v2, NEW)*** — spawn the accountant against a real (temp-file) SQLite, feed synthetic network readings simulating 2 ticks + a rollover, assert the DB has the expected current_cycle + history rows. Verifies the full accumulate→flush→archive→reset→flush cycle.

### 7.3 Performance gate (Windows CI runner only)

- **`benches/poll_cost.rs`** — uses `criterion` to measure per-provider CPU time over a 5-minute simulated poll window. Fails CI if any `Lightweight` or `Watch` source exceeds the NFR-1 threshold.

### 7.4 GUI / E2E (manual smoke checklist)

egui immediate-mode is awkward to drive programmatically; egui_kittest exists (2026) but our transparency/AppBar semantics need a real desktop. Manual checklist (run in sdd-verify):

- [ ] Transparent on light wallpaper (no black box).
- [ ] Transparent on dark wallpaper.
- [ ] Always-on-top survives Win+D.
- [ ] Docked to left edge reserves space (no overlapping windows).
- [ ] Docked to right, top, bottom — all four work.
- [ ] Multi-monitor: sidebar appears on chosen monitor at correct DPI.
- [ ] Status pill shows BASIC on clean unprivileged launch, tooltip accurate.
- [ ] Click pill → UAC prompt → OHM launches → pill flips to FULL within ~5s.
- [ ] Ctrl+Shift+S toggles click-through.
- [ ] Closing sidebar terminates OHM if sidebar started it; leaves OHM running if user started it.
- ***(v2)*** [ ] Network throughput row shows per-NIC RX/TX in Mbps, updates every tick.
- ***(v2)*** [ ] Bandwidth panel shows current-cycle RX/TX/total in GB per-NIC + days-until-reset countdown.
- ***(v2)*** [ ] Changing `cycle_start_day` in Settings takes effect at next rollover (does not retroactively re-split current cycle).
- ***(v2)*** [ ] Bandwidth totals survive: app close/reopen; system reboot; system sleep/wake. (Manual: note totals, reboot, relaunch, verify totals persisted with at most ~60s of data lost.)
- ***(v2)*** [ ] Rollover: set `cycle_start_day` to today, wait past midnight (or simulate via injected clock in test mode), verify current cycle archived to history, new cycle starts at 0.
- ***(v2)*** [ ] NFR-8: all displayed values are human-readable (GHz, GB, Mbps, °C, V, RPM, W) by default.
- ***(v2)*** [ ] NFR-8: "raw values" toggle in Settings switches all displays to Hz/bytes/bps.
- ***(v2)*** [ ] NFR-8: °C/°F toggle in Settings switches all temperature displays app-wide.
- ***(v2)*** [ ] NFR-8: decimal/binary toggle switches byte values between GB and GiB.

---

## 8. File Changes (implementation snapshot and planned deliverables)

The workspace now contains the implementation files listed below. The table
retains `sdd-apply` labels for deliverables that were planned in the original
design; entries marked `implemented` or `provisioned locally` reflect the
current worktree, while release artifacts remain pending.

| Path | Purpose | Created in phase |
|---|---|---|
| `Cargo.toml` (workspace root) | workspace manifest, shared `[workspace.dependencies]` | sdd-apply |
| `crates/sidebar-domain/{Cargo.toml,src/**}` | pure domain logic + types (incl. *(v2)* `billing.rs`, expanded `format.rs`) | sdd-apply |
| `crates/sidebar-sensor/{Cargo.toml,src/**}` | trait + classifier (incl. *(v2)* `Tier::Both` filtering) | sdd-apply |
| `crates/sidebar-adapter-sysinfo/...` | sysinfo adapter | sdd-apply |
| `crates/sidebar-adapter-nvml/...` | nvml-wrapper adapter | sdd-apply |
| `crates/sidebar-adapter-battery/...` | starship-battery adapter | sdd-apply |
| `crates/sidebar-adapter-ohm/...` | LHM HTTP adapter (was WMI; revised 2026-07-08) | sdd-apply |
| `crates/sidebar-adapter-pdh/...` | PDH disk throughput adapter | sdd-apply |
| `crates/sidebar-adapter-net/...` *(v2)* | Per-NIC throughput via `GetIfTable2` (windows crate) | implemented |
| `crates/sidebar-persistence/...` *(v2)* | SQLite bandwidth state store (rusqlite, WAL) | sdd-apply |
| `crates/sidebar-bandwidth/...` *(v2)* | BandwidthAccountant: subscribe, accumulate, flush, rollover | sdd-apply |
| `crates/sidebar-platform/...` | Win32 (AppBar, DWM, DPI, OhmSupervisor) | sdd-apply |
| `crates/sidebar-app/...` | binary: gui + poller + wiring (incl. *(v2)* `bandwidth_panel.rs`) | sdd-apply |
| `benches/poll_cost.rs` | NFR-1 perf gate (now incl. *(v2)* network adapter poller in the measured set) | sdd-apply |
| `resources/LibreHardwareMonitor.exe` | bundled LHM (MPL-2.0, **unsigned** — see AD-14) | provisioned locally; release packaging pending |
| `.github/workflows/ci.yml` | Windows-latest runner, cargo test + bench + clippy + fmt | sdd-apply |
| `.github/workflows/release.yml` *(v2)* | Build → SignPath sign → publish GitHub Release + winget manifest PR (+ optional Store MSIX) | sdd-apply |
| `winget/manifest.yaml` *(v2)* | winget manifest template for `winget-create` submission | sdd-apply |
| `signpath/code-signing-policy.md` *(v2)* | SignPath-required code signing policy doc (linked from README/homepage) | sdd-apply |

**Migration:** The current workspace is no longer greenfield. Config schema
starts at `config_version = 1` (now includes `[bandwidth]`); SQLite schema is
`user_version = 2`, and `sidebar-persistence::migrate` applies the additive
v0→v1→v2 chain (including `current_cycle_metadata`) for existing databases.

---

## 9. Open Questions

### OQ-1 — Distribution format — **RESOLVED (v2 amendment)** *(cross-ref PRD §9)*

**Previously:** architecture was distribution-agnostic; the only distribution-coupled artifact was the bundled LHM binary. **Plan resolved, implementation pending.** The zero-cost-first stack is SignPath (free OSS signing) + GitHub Releases + winget + optional Microsoft Store. See AD-14 and §11; the workflow and policy files are still release-story deliverables.

### OQ-2 — Rust edition (cross-ref PRD §9)

Edition 2021 recorded initially; re-evaluate 2024 once transitive-dep MSRVs align. `sysinfo` 0.39.3 requires MSRV 1.95 (retrieved 2026-07-07) which is itself a 2024-edition-capable toolchain. **Tentative: edition 2021 for v1.** No action in this phase.

### OQ-3 — egui 0.35 vs 0.34 pinning

egui 0.34 (Mar 2026) introduced the "More Ui less Context" refactor where `App::update` was deprecated in favor of `App::ui`. egui 0.35.0 is current latest. **Tentative: pin egui = 0.35 in the workspace `Cargo.toml`.** Adapter code targets the new `App::ui` signature. Validated via docs.rs/egui retrieved 2026-07-07.

### OQ-4 — "Per port" interpretation *(v2, cross-ref PRD §9)*

Architecture implements "per port" as per-NIC (network interface), keyed on LUID. If the user meant TCP port, the `sidebar-adapter-net` crate and `BandwidthAccountant` design would need significant rework (ETW packet capture instead of `GetIfTable2`). Current implementation assumes per-NIC.

### OQ-5 — Locale-aware number formatting *(v2, cross-ref PRD §9)*

`sidebar-domain::format` is locale-stable in v1 (`.` decimal separator, no thousands separator). The function signatures accept a future `Locale` parameter without API breakage. Deferred to v1.1.

---

## 10. Skill Resolution Notes

The `sdd-design` SKILL.md includes a default "Design artifact MUST be under 800 words" guideline. The user's explicit task brief overrides this: it mandates comprehensive PRD and architecture docs with specific named sections (telemetry coverage matrix, NFR-1 through NFR-7, architecture decisions with Choice/Alternatives/Rationale, data flow diagram, crate layout, trait sketches, threading model, testing strategy, file changes, open questions). The user's instruction takes precedence per the skill's own "user override" clause. Both documents are deliberately thorough; the word-count guideline is intentionally not applied. *(v2: the brief further overrides the 800-word guideline by mandating UPDATE-pass amendments that add four new sections worth of content — network, bandwidth, format, distribution. The override stands.)*

---

## 11. Build & Release Pipeline *(v2 amendment — planned; Epic 9 pending)*

The target pipeline implements AD-14 (zero-cost distribution) and will be
triggered on git tag `v*` (semantic version) or manual workflow dispatch. The
`release.yml`, SignPath policy, and package manifests are not in the current
tree; Epic 9 owns those deliverables.

### 11.1 Pipeline stages

```
git tag v1.0.0  ──▶  .github/workflows/release.yml
                          │
                          ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │  STAGE 1 — Build (Windows runner, windows-latest)                │
   │   cargo build --release --target x86_64-pc-windows-msvc          │
   │   → target/release/sidebar.exe (unsigned Rust binary)            │
   │   + resources/LibreHardwareMonitor.exe (bundled, upstream build)  │
   │     by its own maintainers; we do NOT re-sign it)                │
   └─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │  STAGE 2 — Sign sidebar.exe via SignPath Foundation (free OSS)   │
   │   uses: signpath/github-action@v1                                │
   │   with: project-slug, signing-policy-slug,                       │
   │         artifact: target/release/sidebar.exe                     │
   │   → signed-sidebar.exe (OV cert issued to SignPath Foundation)   │
   │   Requires: SignPath project setup, code-signing-policy.md in    │
   │             repo, MFA on approvers, GitHub Actions as trusted CI │
   │   Fallback (if SignPath denied): skip signing, ship unsigned     │
   └─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │  STAGE 3 — Package (matrix of artifacts)                         │
   │   (a) Portable ZIP:  signed-sidebar.exe + LibreHardwareMonitor.exe │
   │                        example → sidebar-v1.0.0-portable.zip     │
   │   (b) MSIX (optional, for Store): makeappx pack from layout      │
   │                        → sidebar-v1.0.0.msix (signed by Microsoft│
   │                        on Store submission, not by us)           │
   └─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
   ┌─────────────────────────────────────────────────────────────────┐
   │  STAGE 4 — Publish                                                │
   │   - GitHub Release: upload portable ZIP, generate release notes  │
   │   - winget: run winget-create to generate manifest, open PR to   │
   │             microsoft/winget-pkgs (auto-validates, ~1hr merge)   │
   │   - Scoop/Chocolatey: community manifests (out of our CI; power  │
   │             users submit these; we document the install command) │
   │   - Microsoft Store (optional, v1.1): Partner Center submission  │
   │             of the MSIX; needs sandbox-compat verification first │
   └─────────────────────────────────────────────────────────────────┘
```

### 11.2 Signing specifics (SignPath Foundation)

- **Project setup (one-time).** Apply at signpath.org; submit the sidebar repo (OSI license, public, free downloads). Foundation issues a free cert in their name; our GitHub Actions is registered as a trusted build system. Approvers (team members) must use MFA.
- **Code signing policy (one-time, in-repo).** `signpath/code-signing-policy.md` — SignPath requires this document, linked from the README, describing what we sign, who approves, how releases are tagged. Required for OSS subscription.
- **Per-release flow.** Tag → Stage 1 builds → Stage 2 submits `sidebar.exe` to SignPath via the GitHub Action → an Approver clicks "Approve" in the SignPath dashboard (or auto-approve if policy permits) → SignPath returns the signed binary. The bundled `LibreHardwareMonitor.exe` is **not** submitted (it's upstream OSS; we redistribute the maintainer's own build).
- **What gets signed.** Only `sidebar.exe` (our Rust binary). The OHM subprocess, any DLLs we ship, and the config files remain unsigned. This matches every other OHM consumer.
- **SmartScreen.** SignPath's cert is shared across their OSS projects, so reputation accrues faster than a brand-new cert. Expect diminishing SmartScreen warnings over the first weeks. The Microsoft Store path (if taken) sidesteps SmartScreen entirely.

### 11.3 Channel-specific notes

| Channel | Signing | Auto-update | Cost | v1 status |
|---|---|---|---|---|
| **GitHub Releases (portable ZIP)** | SignPath (our binary); LHM unsigned | None (user replaces files; future: in-app "new version" link) | $0 | **v1 primary (planned)** |
| **winget** | Uses the GitHub Release binary (SignPath-signed) | `winget upgrade sidebar` (manual/scriptable) | $0 | **v1 primary (planned)** |
| **Scoop / Chocolatey** | Community manifests; uses our signed binary | Package-manager-native | $0 | v1 best-effort |
| **Microsoft Store (MSIX)** | Signed by Microsoft on submission | Store-native auto-update | $0 (Partner Center new flow) | **v1.1** (pending sandbox-compat sdd-verify, see R13) |

### 11.4 CI vs Release separation

- `ci.yml` runs on every push/PR: `cargo test`, `cargo bench --bench poll_cost` (NFR-1 gate, Windows runner), `cargo clippy -- -D warnings`, `cargo fmt --check`. No signing, no publishing.
- `release.yml` runs only on tags / manual dispatch: the full Stage 1–4 pipeline above. Gated on a `release-approver` environment (GitHub Environment with required reviewers) so an accidental tag doesn't auto-publish.

### 11.5 What this section does NOT cover

- **Notarization (macOS).** Out of scope — sidebar is Windows-only (NFR-5).
- **Linux builds.** Out of scope (NFR-5: Win11 only).
- **Auto-update for the portable ZIP path.** v1 ships without; v1.1 may add an in-app GitHub-API "latest release" check that links the user to the download page. The winget and Store paths already have auto-update.
- **Telemetry / crash reporting from end users.** Out of scope for v1 (privacy-friendly default); the SignPath terms and our own privacy posture mean we ship no analytics.

---

## 12. Development Environment

See `CONTRIBUTING.md` for the contributor setup guide and `docs/backlog/nfr-thresholds.md` T-44 for the prerequisite contract. Summary below.

The dev environment is intentionally **relocatable** — most tooling lives under
`tools/` in the workspace, so the folder can be moved between Win11 machines.

**System prerequisites** (pre-existing, not folder-relocatable): Rust ≥1.95, `llvm-tools-preview` rustup component, MSVC Build Tools + Windows SDK, PowerShell 7+, Git.

**Project-local tooling** (under `tools/`, relocatable): `cargo-deny`, `cargo-audit`, **`cargo-llvm-cov`** (Windows-native coverage; NOT `cargo-tarpaulin` which is Linux-only — see T-43), `cargo-nextest`, `actionlint`, `winget-create`, `sqlite3`.

**Activation & verification scripts** (Story 0.7): `scripts/env.ps1` (PATH prepend), `scripts/verify-dev-env.ps1` (prerequisite assertion, CI pre-flight), `scripts/fetch_ohm.ps1` (Story 6.5 LHM binary acquisition, SHA-256-verified).

**Reference hardware (T-31) is generalized** to "any modern 8+ core x86_64 CPU, ≥16 GB RAM, Win11 24H2/25H2" with a per-machine calibration constant for the NFR-1 bench.

---

## 13. Epic 0–8 gap-closure evidence (2026-07-11)

The `fix-epic8-gaps` remediation is implemented through the PR4 integration
slice. The following statements are limited to behavior covered by the current
workspace tests and checks; Win11/UAC/capture behavior that requires a desktop
remains explicitly manual.

### Implemented contracts

- **Runtime and shutdown:** a `TierChanged(Full)` event rebuilds the provider
  registry and poller before the next tick; shutdown cancellation emits one
  `Event::Shutdown`, force-flushes, tears down sidebar-owned LHM, and joins
  the poller, accountant, event-coalescer, and Ctrl+C signal handler
  idempotently. Poller, event-channel, GUI-close, and shutdown tests cover the
  transition and join paths.
- **System-theme bridge:** the runtime `Event::ThemeChanged(String)` contract
  carries the resolved lowercase mode (`"dark"` or `"light"`); `"system"` is
  configuration-only. The Win32 bridge reads `AppsUseLightTheme`, publishes
  the resolved event on `WM_SETTINGCHANGE/ImmersiveColorSet`, and the GUI
  repaints so `ThemePreference::System` applies the live OS palette.
- **Capture exclusion:** `[display] hide_from_capture = false` is the default
  and leaves capture enabled. When explicitly enabled, the GUI obtains the live
  eframe root HWND and calls `SetWindowDisplayAffinity` with
  `WDA_EXCLUDEFROMCAPTURE`; missing/invalid HWNDs and API errors are logged as
  a non-success. The real Win11 visual capture smoke is still manual.
- **Bandwidth precision:** network readings are filtered by `MetricKind` before
  LUID grouping, and `ReadingValue::Counter(u64)` preserves values above
  `2^53` through serialization and accumulation.
- **LHM ownership:** every launched child is owned by an RAII guard until Job
  Object setup succeeds; setup failure terminates, reaps, and closes handles.
  Real UAC/Job Object process-reap smoke remains manual.
- **G16 exception:** production HTTP accepts only literal `http://127.0.0.0/8`
  or `http://[::1]` authorities, rejects hostnames/remote targets before
  transport, and disables automatic redirects so a remote `Location` cannot
  escape the loopback boundary. Rejection and redirect-regression tests pass.

### Deferred scope (not implemented by this change)

The following remain pending and must not be described as completed by this
remediation: **3.2b acquisition/benchmark decision, 6.5 LHM acquisition,
6.6 capture-cloak visual validation, and the uncompleted portions of the 9.x,
10.x, and 11.x release/CI stories**. Use `docs/backlog/PROGRESS.md` for the
authoritative per-story status; merged 11.x stories remain merged.

### Validation recorded for this slice

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo test --workspace --all-targets` | 625 passed, 13 ignored, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo deny check` | pass (Windows QA, 2026-07-12); existing duplicate-dependency and unmatched-license warnings remain |
| `cargo check --workspace --target x86_64-pc-windows-msvc` | pass |
| `cargo build -p sidebar-app --release --target x86_64-pc-windows-msvc` | verified snapshot (2026-07-12T02:26:38Z); `target/x86_64-pc-windows-msvc/release/sidebar-app.exe` (17,688,064 bytes, SHA-256 `68B9F8AC43F56DC789D3C9DAD7A1BA87055B675889262A2C202A049C049FA087`) |

---

## 14. Current implementation state and known gaps (2026-07-12)

The current worktree closes the previously documented integration wiring gaps;
release acceptance is still partial until commit/review and manual Windows
gates complete:

- **Runtime hooks:** `SidebarApp::run` preserves the launch callback,
  `BandwidthView` receiver, and child-liveness probe when eframe creates the
  native app instance.
- **Full-mode launch:** BASIC clicks send a request to the supervisor-owner
  thread, which invokes `OhmSupervisor::launch_elevated`; UAC remains explicit.
- **Bandwidth bridge:** `BandwidthAccountant` publishes snapshots over a watch
  channel after loading retained `bandwidth_history` rows; the GUI drains this
  derived DTO and never opens SQLite.
- **OHM monitoring:** the supervisor-owner thread polls child liveness. The GUI
  emits one Full→Basic degradation event only for a child the sidebar launched;
  externally running LHM is intentionally not owned or degraded.
- **Theme/header:** missing `AppsUseLightTheme` defaults to Dark, and the 12.1
  header renders the local `HH:MM` clock and ISO date.
- **Remaining gates:** 6.6 manual Win32 smoke, 9.x SignPath/release/winget,
  10.1–10.2 NFR/smoke, 11.x regression/coverage, and real UAC/LHM, Job Object,
  capture, multi-monitor, and release-walker checks remain pending. The §13
  test command is current evidence only; it is not a substitute for HITL.

These are wiring/closure gaps, not a change to the product boundary: the
project remains Win11-only, Basic mode remains no-admin, LHM remains a local
HTTP sidecar, and monthly bandwidth remains per-NIC (not per TCP port).

---

## Citations (retrieved 2026-07-07; LHM HTTP revised 2026-07-08)

1. **egui** — https://docs.rs/egui — v0.35.0 latest; `ViewportBuilder::with_transparent` confirmed; `eframe::App` trait.
2. **sysinfo** — https://docs.rs/sysinfo — v0.39.3; MSRV 1.95; Windows CPU temp returns empty.
3. **nvml-wrapper** — https://docs.rs/nvml-wrapper — v0.12.0 (Mar 2026).
4. **ureq** — https://docs.rs/ureq — v2.x (sync HTTP client; replaces `wmi` crate as of 2026-07-08 LHM HTTP migration). Used by `sidebar-adapter-ohm` for `GET /data.json`.
5. **starship-battery** — https://docs.rs/starship-battery — Windows 7+ supported.
6. **windows crate** — https://docs.rs/windows — v0.62.2.
7. **MetricsHub LibreHardwareMonitor connector** — https://www.metricshub.com/docs/latest/connectors/librehardwaremonitor — historical WMI integration reference; the WMI path was removed in LHM v0.9.5 (Jan 2026). sidebar now uses the LHM HTTP `/data.json` endpoint (AD-2, revised 2026-07-08). See also [LHM issue #2143](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/2143) for the maintainer's confirmation.
7a. ***(2026-07-08)* LHM HttpServer.cs** — https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/master/LibreHardwareMonitor.Windows.Forms/Utilities/HttpServer.cs — source of the `/data.json`, `/metrics`, and `/Sensor?action=Get` routes; configurable `ListenerPort` / `ListenerIp`.
8. **Sentry Hardware Connectors v41** — https://www.sentrysoftware.com/docs/hardware-connectors/latest/connectors/MS_HW_LibreHardwareMonitor.html — as of 2026-03-04.
9. **Microsoft Learn — Code signing options** — https://github.com/MicrosoftDocs/windows-dev-docs/blob/docs/hub/apps/package-and-deploy/code-signing-options.md — ms.date 2026-04-20.
10. **Microsoft Learn — MSIX AppInstaller auto-update** — https://learn.microsoft.com/en-us/windows/msix/app-installer/auto-update-and-repair--overview — ms.date 2026-04-10.
11. ***(v2)* rusqlite crate** — https://docs.rs/rusqlite — bundled SQLite bindings for Rust; `bundled` feature compiles sqlite3.dll into the binary (~1 MB).
12. ***(v2)* SignPath Foundation — OSS conditions** — https://signpath.org/terms.html — free OSS code signing eligibility (OSI license, public repo, free downloads, MFA, no PUPs/hacking tools).
13. ***(v2)* SignPath — GitHub trusted build system** — https://docs.signpath.io/trusted-build-systems/github — GitHub Actions integration for OSS signing.
14. ***(v2)* Microsoft Learn — Open a Store developer account** — https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account — new onboarding flow: **$0 for Individual and Company accounts** (India 2026 included).
15. ***(v2)* Microsoft Learn — winget repository submission** — https://learn.microsoft.com/en-us/windows/package-manager/package/repository + https://github.com/microsoft/winget-pkgs — PR-based manifest submission; `winget-create` tool.
16. ***(v2)* Azure Trusted Signing pricing** — https://azure.microsoft.com/en-us/pricing/details/artifact-signing/ — Basic $9.99/mo, no free tier in 2026; rejected by user on cost grounds.
17. ***(v2)* Microsoft Learn — GetIfEntry2 (netioapi.h)** — https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getifentry2 — single-row per-adapter fill; lighter than `GetIfTable2`.
18. ***(v2)* Microsoft Learn — GetIfTable2 (netioapi.h)** — https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getiftable2 — full-table allocation + `FreeMibTable`; not used per-adapter.
19. ***(v2)* SQLite for time-series in Rust (guide)** — https://medium.com/rustaceans/harnessing-the-power-of-sqlite-for-time-series-data-storage-in-rust-a-comprehensive-guide-321612470836 — rationale for SQLite over TOML for accumulated state.
20. ***(v2)* HN: SQLite append-only audit-log suitability** — https://news.ycombinator.com/item?id=17855045 — community consensus on SQLite for append-only patterns.

---

**End of architecture.** Companion document: `PRD.md`.
