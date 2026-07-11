# NFR Thresholds — sidebar-v1

**Single source of truth for every numeric boundary in the system.** Every test that asserts a numeric NFR MUST cite this file as `nfr-thresholds.md#T-<id>` in a doc-comment. This prevents threshold drift across stories and makes the swarm's tests self-validating.

Cross-references: PRD §6 (NFR statements), architecture.md §7 (testing strategy), guardrails.md G14 (resource bounds), G17 (generation bounds).

---

## Compute & Polling

### T-1 — Per-source CPU budget (NFR-1, CRITICAL)
- **Value:** `0.5%` CPU average over a 5-minute window on reference hardware.
- **Measurement:** `criterion` bench `poll_cost`, normalized to reference machine.
- **Scope:** Per `SensorProvider` adapter. Sum across all adapters in a tier MUST also stay under `T-2`.
- **Failure action:** Build fails; offending source moved to `Deferred` per OQ-2.
- **Cited by:** Story 2.3, Story 10.1, every adapter in Epic 3.

### T-2 — Aggregate poller CPU budget
- **Value:** `2.0%` CPU average across all active providers over a 5-minute window.
- **Measurement:** Same bench, summed.
- **Rationale:** T-1 alone is insufficient — 10 × 0.4% sources = 4% aggregate, violating user intent.
- **Cited by:** Story 7.2, Story 10.1.

### T-3 — Default poll interval
- **Value:** `10s` default; range `1s`–`60s` inclusive.
- **Clamping rule:** Out-of-range values are clamped to nearest bound AND logged via `tracing::warn!` with the original + clamped value.
- **Cited by:** Story 1.5, Story 7.2.

---

## Memory

### T-4 — Steady-state RSS, Basic mode (NFR-4)
- **Value:** `≤ 80 MiB` resident set, measured via `GetProcessMemoryInfo(WorkingSetSize)`.
- **Measurement window:** 5 minutes after cold start, p95 over 60 samples at 5s cadence.
- **Cited by:** Story 10.1.

### T-5 — Steady-state RSS, Full mode (NFR-4)
- **Value:** `≤ 120 MiB` (host process only; OHM is separate).
- **Cited by:** Story 10.1.

### T-6 — SQLite working-set contribution
- **Value:** `≤ 3 MiB` incremental RSS attributable to bundled SQLite + WAL cache.
- **Rationale:** AD-11 budget headroom; T-4/T-5 were set pre-SQLite.
- **Cited by:** Story 4.1, Story 10.1.

---

## Latency

### T-7 — Cold-start, Basic mode (NFR-3)
- **Value:** `≤ 2000 ms` from process start to first complete egui frame, p95 over 20 launches.
- **Measurement:** Inject a tracer that records `Instant` at `main()` entry and at the first `eframe::App::ui` call.
- **Cited by:** Story 10.1.

### T-8 — Cold-start, Full mode (NFR-3)
- **Value:** `≤ 6000 ms` (includes LHM subprocess launch + first HTTP `/data.json` round-trip).
- **Note:** OHM's own startup dominates; we do not control it.

### T-9 — GUI frame budget
- **Value:** `≤ 16 ms` per egui repaint (60 FPS vsync).
- **Cited by:** Story 8.1.

---

## Timeouts

### T-10 — LHM HTTP probe timeout (AD-7, revised 2026-07-08 — was WMI)
- **Value:** `500 ms` hard timeout on `GET http://127.0.0.1:<port>/data.json` via `ureq::Agent::timeout(Duration::from_millis(500))`.
- **Failure action:** Treat as unreachable → Basic mode. NO retry within the same launch (except the T-45 port-fallback chain during launch).
- **Cited by:** Story 3.6, Story 6.4, Story 7.3.

### T-11 — LHM subprocess launch timeout
- **Value:** `5000 ms` from `ShellExecuteW("runas")` return to first successful HTTP probe on the chosen port.
- **Failure action:** Status pill shows "LHM launch failed", tier remains Basic.
- **Cited by:** Story 6.4.

### T-12 — SQLite busy-retry ceiling (AD-11)
- **Value:** Max `5` retries with exponential backoff `[10ms, 20ms, 40ms, 80ms, 160ms]`. Total wait `≤ 310 ms`.
- **Failure action:** After ceiling, return `Err(SqliteFailure(SQLITE_BUSY))` to caller. NO infinite retry.
- **Cited by:** Story 4.2.

### T-13 — NVML call timeout (defensive)
- **Value:** NVML has no native timeout; wrap each call in `tokio::time::timeout(100ms, spawn_blocking(...))`.
- **Failure action:** Treat timeout as NVML error, return empty readings, log.
- **Cited by:** Story 3.2.

---

## Resource Bounds

### T-14 — Broadcast channel capacity
- **Value:** `8` messages.
- **Behavior on overflow:** Oldest dropped (standard `tokio::broadcast` semantics); each drop emits `tracing::warn!`.
- **Cited by:** Story 7.2, guardrails.md G14.

### T-15 — Bandwidth flush debounce
- **Value:** `60 s` between debounced flushes; immediate flush on shutdown + rollover + config change.
- **Cited by:** Story 5.2.

### T-16 — History retention (v1)
- **Value:** `current + previous` cycle (i.e. `keep_cycles = 1` in `prune_history`). Older rows deleted on rollover.
- **Cited by:** Story 4.2.

### T-17 — WAL checkpoint interval
- **Value:** SQLite default (`PRAGMA wal_autocheckpoint = 1000` pages). Do not override without profiling evidence.
- **Cited by:** Story 4.1.

### T-18 — Tokio runtime size
- **Value:** `2` worker threads (per AD-6). Multi-threaded flavor.
- **Cited by:** Story 7.2.

### T-19 — Tokio shutdown grace
- **Value:** `3000 ms` from shutdown signal to forced runtime drop. Within this window: poller stops, BandwidthAccountant force-flushes, OHM supervisor terminates sidebar-owned child.
- **Cited by:** Story 7.x wiring, guardrails.md G14.

---

## Data Bounds

### T-20 — Reading value range
- **Value:** All `Reading::value: f64`. Must be finite (`f64::is_finite()`). `NaN`/`±Inf` are forbidden at the trait boundary; adapters that cannot produce a value MUST omit the reading, not emit NaN.
- **Failure action:** `format_*` functions render `"--"` for non-finite; poller logs an `error!` if a NaN slips through (defensive, post-hoc).
- **Cited by:** Story 1.1, Story 1.3, Story 3.x adapters.

### T-21 — Process list top-N
- **Value:** Default `N = 5`. Configurable `1 ≤ N ≤ 50`. Out-of-range clamped + logged.
- **Cited by:** Story 1.6, Story 8.1.

### T-22 — Sparkline rolling window
- **Value:** Default `60` samples (= 10 minutes at default poll). Configurable `10 ≤ window ≤ 600`.
- **Cited by:** Story 1.6.

---

## Network Adapter Counter Semantics

### T-23 — Counter wraparound contract
- **Value:** If `current_counter < previous_counter`, treat as counter reset: `delta = current_counter`. (64-bit Win11 counters don't wrap in practice, but we defend.)
- **Cited by:** Story 5.1.

### T-24 — LUID stability expectation
- **Value:** `MIB_IF_ROW2.InterfaceLuid` MUST be stable across reboots per the Microsoft IP Helper contract.
- **Verification:** Integration test on Windows CI asserts LUID is identical across two reads within one process AND (manually) across a reboot.
- **Failure action:** If disproved in sdd-verify, fall back to MAC address (R10).
- **Cited by:** Story 3.5, guardrails.md G9.

---

## Billing Cycle Arithmetic

### T-25 — Cycle length invariant
- **Value:** For any valid `(CycleStartDay, year, month)`, `cycle_end - cycle_start ∈ [27, 31] days` inclusive.
- **Cited by:** Story 1.4.

### T-26 — `CycleStartDay::Day(u8)` invariant
- **Value:** `Day(d)` requires `1 ≤ d ≤ 28`. Construction with `d = 0` or `d > 28` MUST panic in debug / clamp + log in release.
- **Rationale:** Day 29–31 don't exist in February; allowing them invites ambiguity. Users wanting month-end use `LastDayOfMonth`.
- **Cited by:** Story 1.4.

### T-27 — Timezone contract for billing
- **Value:** All cycle arithmetic uses `chrono::NaiveDate` / `NaiveDateTime` (no timezone). "Today" is `Local::now().date_naive()`. Rollover fires at `00:00:00` local time on the cycle start day.
- **Cited by:** Story 1.4, Story 5.2.

---

## GUI Display (NFR-8)

### T-28 — Decimal vs binary byte base
- **Value:** Default `Base::Decimal` (10⁹). Binary (`Base::Binary`, 2³⁰) only behind explicit toggle.
- **Cited by:** Story 1.3.

### T-29 — Temperature unit
- **Value:** Default `TempUnit::Celsius`. Fahrenheit toggle affects ALL temp readings app-wide.
- **Cited by:** Story 1.3.

### T-30 — Format precision
- **Value:** Hz = 3 sig figs; bytes = 3 sig figs; bps = 3 sig figs; voltage = 3 decimals; power = 2 decimals; percent = integer; rpm = integer.
- **Cited by:** Story 1.3 (exact expected strings in test cases).

---

## Reference Hardware (NFR baseline)

### T-31 — Reference machine (generalized per dev-env inventory 2026-07-07)
- **Spec:** Any modern 8+ core x86_64 CPU, ≥ 16 GB RAM, Win11 24H2 (build 26100) OR 25H2 (build 26200).
- **Calibration:** Because reference hardware varies, the NFR-1 bench (`poll_cost`) reports a **calibration constant** per machine — the idle baseline CPU% measured over 60s before the bench runs. The T-1/T-2 thresholds are then evaluated as (measured − calibration) deltas, not absolutes. Documented in `benches/poll_cost.rs` header.
- **Original spec (deprecated):** Intel i5-1240P / 16 GB / 24H2. Retained for historical context; do NOT use for v1 acceptance.
- **This dev machine (LAPTOP-PLN56DNU):** AMD Ryzen AI 7 350 (8+8 cores), 24 GB RAM, Win11 25H2 build 26200. AMD Radeon 860M iGPU (no NVIDIA). Used as the primary local acceptance machine; NVIDIA-only paths (Story 3.2) are validated on a separate CI runner or deferred.
- **CI runner delta:** `windows-latest` differs from any specific dev machine; the calibration-constant approach (above) normalizes results across machines.
- **Cited by:** Story 10.1, all perf-sensitive stories, `docs/dev-env.md` §1.1.

---

## Supply Chain

### T-32 — License allowlist
- **Allowed:** MIT, Apache-2.0, MPL-2.0, BSD-3-Clause, ISC, Zlib, Unicode-DFS-2016, CC0-1.0.
- **Forbidden:** GPL, AGPL, LGPL, LGPL-2.0+, SSPL, any proprietary, "unlicensed", unknown.
- **Cited by:** Story 0.3 (deny.toml), guardrails.md G3/G18.

### T-33 — RUSTSEC advisory policy
- **Value:** `cargo audit` MUST pass with zero unmuted advisories. Muting an advisory requires a `#[allow]` comment in `deny.toml` with rationale + expiry date.
- **Cited by:** Story 0.3, guardrails.md G18.

### T-43 — Coverage tool (Windows-corrected per dev-env inventory 2026-07-07)
- **Value:** `cargo-llvm-cov` (NOT `cargo-tarpaulin`). Tarpaulin is Linux-only (uses ptrace) and does not run on Windows.
- **Prerequisite:** `rustup component add llvm-tools-preview`.
- **Invocation:** `cargo llvm-cov --workspace --lcov --output-path coverage/lcov.info`.
- **Cited by:** Story 0.2 (CI), Story 11.2 (regression gate), Story 10.1 (NFR verification), T-42 (coverage floor). `docs/dev-env.md` §3.2/§3.3.

### T-44 — Dev environment prerequisites (per dev-env inventory 2026-07-07)
- **System prerequisites (must pre-exist on the machine, cannot be folder-relocated):**
  1. Rust ≥ 1.95 (MSRV forced by sysinfo 0.39.3).
  2. `rustup component add llvm-tools-preview` (for cargo-llvm-cov).
  3. MSVC Build Tools + Windows SDK (for the `windows` crate FFI link).
  4. PowerShell 7+ (for `scripts/env.ps1`, `fetch_ohm.ps1`).
  5. Git for Windows (for `cargo` and CI scripts).
- **Project-local tooling (under `D:\dev\sidebar\tools\`, relocatable):**
  - `tools/cargo-bin/` — cargo-deny, cargo-audit, cargo-llvm-cov, cargo-nextest (installed via `cargo binstall --install-root`).
  - `tools/ci/` — actionlint, winget-create (installed via scoop).
  - `tools/sqlite/` — sqlite3.exe (for debugging bandwidth.db).
- **Activation:** `scripts/env.ps1` prepends the `tools/` subdirectories to PATH.
- **Verification:** `scripts/verify-dev-env.ps1` (Story 0.6 deliverable) asserts all prerequisites + tools; exits non-zero on any failure.
- **Cited by:** Story 0.1 (workspace), Story 0.2 (CI mirrors this locally), Story 6.5 (LHM fetch script). `docs/dev-env.md`.

### T-45 — LHM HTTP port + fallback chain (added 2026-07-08 with AD-2 revision)
- **Default port:** `17127`. Chosen because it is (a) above the IANA registered-and-reserved ranges (0–1023, plus Windows dynamic-excluded ranges), (b) below the ephemeral range Windows uses by default (49152–65535), (c) free on this dev machine (verified 2026-07-08 via `Get-NetTCPConnection` + `netsh interface ipv4 show excludedportrange`), (d) not a well-known application port.
- **Fallback chain:** On launch, `OhmSupervisor` probes 17127; if occupied by a non-LHM service (HTTP response doesn't match LHM JSON signature) OR if the bind fails, it tries 17128, 17129, ... 17137 (10 candidates) and picks the first free one.
- **Persistence:** The chosen port is written into `resources/LibreHardwareMonitor.exe.config` BEFORE launching LHM (`runWebServerMenuItem=true`, lowercase `listenerPort=<port>`). The current launch path does not rewrite `config.toml`; the configured `[ohm] http_port` remains the initial probe preference.
- **Out of fallback chain:** If all 10 candidates are occupied → Full mode is unavailable for this session; status pill shows "LHM port unavailable", tier = Basic, logged at `warn!`.
- **Cited by:** AD-2 (revised), AD-7 (revised), Story 6.4, Story 7.3, Story 3.6 (consumes the resolved port), Story 1.5 (`[ohm] http_port` config).

---

## UX Behavior (audit pass 3)

### T-34 — Global hotkey defaults
- **Value:** `Ctrl+Shift+S` toggles click-through (NFR-7). Configurable in `[hotkeys] click_through = "Ctrl+Shift+S"`.
- **Registration:** `RegisterHotKey` per HWND; hotkey parsed via `global-hotkey` crate (MIT/Apache-2.0, T-32-allowed) OR direct Win32 `RegisterHotKey`.
- **Conflict behavior:** If the hotkey is already registered by another app, sidebar logs `warn!` and the toggle is unavailable until the conflict resolves. NO silent fallback to a different key.
- **Cited by:** Story 6.6, Story 1.5 (`[hotkeys]` config section).

### T-35 — Theme defaults
- **Value:** Default theme = `Dark`. Accent color = `#4CAF50` (green) for the FULL-mode pill, alerts, and active settings rows. Configurable in `[theme] mode = "Dark" | "Light" | "System"` and `[theme] accent = "#RRGGBB"`.
- **System theme tracking:** When `mode = "System"`, sidebar follows Windows dark/light via `RegQueryValueEx(HKCU\...\Personalization\AppsUseLightTheme)`. Re-checks on `WM_SETTINGCHANGE` broadcast.
- **egui mapping:** `Dark` → `egui::Visuals::dark()`; `Light` → `egui::Visuals::light()`; accent injected via `ctx.style().visuals.selection.bg_fill`.
- **Cited by:** Story 8.6, Story 1.5.

### T-36 — Multi-monitor target selection
- **Value:** Default = primary monitor. User-selectable per-monitor in `[dock] monitor_id` (stored as the monitor's `DeviceID` from `EnumDisplayDevices`, stable across reboots per Win32 contract).
- **Behavior on monitor disconnect:** sidebar re-docks to the primary monitor + emits a `warn!`. On reconnect of the configured monitor, sidebar does NOT auto-move back (avoid surprising the user); user re-selects in Settings.
- **DPI:** per-monitor v2 (T-31 hardware; NFR-6) — sidebar re-renders at the target monitor's DPI without restarting.
- **Cited by:** Story 6.2 (AppBar param), Story 6.6 (monitor picker UI), Story 1.5 (`[dock]` section).

### T-37 — First-run wizard required fields
- **Value:** On first launch (detected via absence of `%APPDATA%\sidebar\config.toml`), sidebar presents a wizard collecting:
  1. Docked edge (left/right/top/bottom) — default right.
  2. Target monitor — default primary.
  3. Billing-cycle start day — default 1.
  4. Theme — default Dark.
- **Skip behavior:** Wizard is skippable; defaults are applied on skip. A "completed" flag (`config.first_run_complete = true`) prevents re-prompting.
- **Cited by:** Story 8.10, Story 1.5.

### T-38 — Tier change event coalescing
- **Value:** When `OhmSupervisor` detects OHM crash (transition Full → degraded-Basic) or recovery (Basic → Full after user re-enables), the tier change is broadcast on the `Event` channel. Multiple tier transitions within a 500ms window are coalesced to the latest; the GUI repaints at most once per coalesce window.
- **Cited by:** Story 7.4, Story 6.4.

### T-39 — Graceful shutdown timeout hierarchy
- **Value:** On `Ctrl+C` / `SIGTERM` / `WM_CLOSE`:
  1. `t=0ms`: poller cancels via `CancellationToken`.
  2. `t=0–500ms`: BandwidthAccountant force-flushes to SQLite (synchronous).
  3. `t=500–2000ms`: OhmSupervisor terminates sidebar-owned OHM child.
  4. `t=2000–3000ms`: tokio runtime drops; eframe exits.
  5. `t=3000ms`: forced process exit (`std::process::exit(0)`) if anything is stuck.
- **Cited by:** Story 7.5, guardrails.md G14 (T-19 alignment).

---

## Regression Harness Budgets (audit pass 4)

### T-40 — Per-layer test runtime budgets
- **L0 unit (`cargo test --lib`):** ≤ 60 s total across all crates.
- **L1 integration (`cargo test --tests`):** ≤ 60 s total (excludes `#[ignore]`).
- **L2 UI snapshots (`cargo test --test ui_snapshots`):** ≤ 30 s total.
- **L3 bench (`cargo bench`):** ≤ 600 s total.
- **L4 smoke:** manual; scriptable subset ≤ 5 min on the release runner.
- **Hard rule:** If a story's tests would push any layer over its budget, the swarm MUST split the story or optimize — never silently exceed. See G27.

### T-41 — Aggregate PR regression budget
- **Value:** The full L0+L1+L2+L3 matrix (the "regression run") MUST complete in ≤ 750 s on the Windows CI runner (60 + 60 + 30 + 600 = 750).
- **Cache:** `Swatinem/rust-cache@v2` MUST be used; cache hit brings cold-build time under the budget.
- **Failure action:** If the regression run exceeds 750s, CI fails with `regression-budget-exceeded`. The swarm MUST split the offending story or mark it for orchestrator review.

### T-42 — Coverage delta floor
- **Value:** For every PR touching crate(s) C, the line coverage of C MUST NOT decrease.
- **Measurement:** `cargo llvm-cov --workspace --lcov --output-path coverage/lcov.info` (per T-43; NOT `cargo tarpaulin` which is Linux-only) on the PR vs `main`; diff per crate.
- **Tolerance:** ±0.0% (zero regression). An intentional decrease (e.g. removing dead code) requires a PR-description justification + HITL sign-off per G19.
- **Target:** `sidebar-domain` and `sidebar-sensor` ≥ 80% line coverage; adapter/platform crates ≥ 60% (Win32 FFI is hard to cover fully); `sidebar-app` ≥ 40% (GUI).
