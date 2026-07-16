# Epics & Stories — sidebar-v1 (Audited, Hardened)

**Status:** Two-pass audit complete. Schema per Gentle-AI SDD protocol. Every story carries: TDD contract (Happy Path + Boundary, citing fixtures `F-*` and thresholds `T-*`), cold-start setup/teardown, panic-safety, resource bounds, and explicit HITL gates. Cross-cutting guardrails in `guardrails.md` apply to all.

---

## EPIC 0 — Foundation Workspace & Scaffolding
- **System Objective:** Bootstrap the Cargo workspace, supply-chain policy, repo hygiene, CI scaffolding, and shared dependency pinning so downstream stories have a hardened compile target.
- **Swarm Mapping:** Platform-Foundation Agent.

### STORY 0.1: Workspace Skeleton + Pinned Dependency Manifest
- **User Story:** As the Architect, I want a Cargo workspace with all 11 library crates + 1 binary crate stubbed (12 packages total), dependencies split between `[workspace.dependencies]` (shared) and per-crate `[dependencies]`, so every subsequent story compiles in isolation without version drift.
- **Technical Context:** architecture.md §4. Crates: `sidebar-domain`, `sidebar-sensor`, `sidebar-adapter-{sysinfo,nvml,battery,ohm,pdh,net}`, `sidebar-persistence`, `sidebar-bandwidth`, `sidebar-platform`, `sidebar-app` (bin). Workspace-level pins (retrieved 2026-07-07; LHM HTTP migration 2026-07-08): `sysinfo = 0.39.3  # MIT`, `nvml-wrapper = 0.12.0  # MIT/Apache-2.0`, `ureq = 2.12  # MIT/Apache-2.0` (replaces `wmi = 0.18.4` — LHM dropped WMI in v0.9.5+, see AD-2 revised), `serde_json = 1  # MIT/Apache-2.0` (for LHM `/data.json` parsing), `starship-battery = 0.10  # MIT/Apache-2.0`, `windows = 0.62.2  # MIT/Apache-2.0`, `egui = 0.35  # MIT`, `eframe = 0.35  # MIT`, `tokio = 1  # MIT`, `rusqlite = 0.32  # MIT (bundled feature)`, `toml = 0.8  # MIT/Apache-2.0`, `serde = 1  # MIT/Apache-2.0`, `mockall = 0.12  # MIT/Apache-2.0`, `criterion = 0.5  # MIT/Apache-2.0`, `proptest = 1  # MIT/Apache-2.0`, `tracing = 0.1  # MIT`, `chrono = 0.4  # MIT/Apache-2.0`, `tempfile = 3  # MIT/Apache-2.0` (dev-dep). Edition 2021 (OQ-2/OQ-3). MSRV 1.95 (forced by sysinfo). Workspace `[lints]` workspace-wide clippy gates.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm crate list against architecture §4 (11 libs + 1 bin, 12 packages). Decide which deps are shared (workspace) vs local. Confirm `rust-toolchain.toml` channel matches MSRV.
  2. [ ] **Implement:** Root `Cargo.toml` with `[workspace] members`, `[workspace.dependencies]` with commented licenses, 12 stub `Cargo.toml` + `src/lib.rs`/`src/main.rs` each containing a real smoke test (NOT an empty stub — see G17). Workspace `[lints.clippy]` with `undocumented_unsafe_blocks = "deny"`.
  3. [ ] **Validate:** `cargo check --workspace` passes; `cargo test --workspace` runs all smokes green; `cargo fmt --check`; `cargo clippy --workspace -- -D warnings`. Use fixture F6 to verify each crate's `lib.rs` is loadable twice in one process (idempotency sanity).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Each crate's `lib.rs` exposes `pub fn crate_present() -> bool { true }`; test asserts it returns `true`. (Real assertion, not `assert!(true)`.)
    2. Workspace test asserts `cargo_metadata::MetadataCommand::new().exec()` returns exactly 12 packages with the expected names.
  - **Boundary & Edge Case Test Cases (cite T-* and F-*):**
    1. MSRV violation: set `rust-version = "99.0.0"` in one crate temporarily; `cargo build` must error with `rustc` MSRV diagnostic (not a generic compile error).
    2. Dependency conflict: introduce two crates pinning different major versions of `tokio`; `cargo tree --duplicates` MUST list the conflict (CI gate). Fixture F6.
    3. Empty workspace member: remove `src/lib.rs` from one crate; `cargo check` MUST fail with `error[E0761]: --crate-type bin requires a main.rs` or analogous precise diagnostic — no silent skip.
- **Explicit Swarm Guardrails:** HITL approval required on pinned versions (G3/T-32). Shell permission gate on `cargo` invocation (G19). NO dependency may be added without the license-comment (CI denies via Story 0.3).

### STORY 0.2: CI Workflow — `ci.yml` (Windows runner, test+bench+clippy+fmt+deny+audit)
- **User Story:** As the Architect, I want a GitHub Actions workflow on `windows-latest` that runs the full test suite, NFR-1 perf bench, clippy with `-D warnings`, fmt check, `cargo deny`, and `cargo audit` on every PR.
- **Technical Context:** architecture.md §8 + §11.4 + guardrails.md G18/G22. Runner: `windows-latest`. Toolchain pinned via Story 0.4. Steps in order: `cargo fmt --check` (T-22 budget: 60s) → `cargo clippy --workspace -- -D warnings` (180s) → `cargo deny check bans licenses advisories sources` → `cargo audit` → `cargo test --workspace` (120s, T-22) → `cargo bench --bench poll_cost` (600s, T-22) → bench-threshold parser. Caching via `Swatinem/rust-cache@v2`. Egress allowlist per G16.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `windows-latest` ships Win11 24H2+ in 2026-07. Verify `actionlint`. Confirm criterion JSON parseable for T-1 threshold.
  2. [ ] **Implement:** `.github/workflows/ci.yml` + `benches/parse_threshold.rs` (parses criterion JSON, exits non-zero if any group's mean CPU% > T-1).
  3. [ ] **Validate:** Trigger on a no-op PR; all gates green.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `actionlint` parses `ci.yml` clean.
    2. `parse_threshold.rs` unit-tested with synthetic criterion JSON at 0.3% (pass) and 0.6% (fail).
  - **Boundary & Edge Case Test Cases:**
    1. Bench breach: inject a fake bench reporting >0.5%; `parse_threshold` MUST exit non-zero with `NFR-1 violation: provider X exceeded 0.5% (got 0.6%)`. Threshold T-1.
    2. Clippy drift: inject `clippy::needless_borrow`; CI fails at the clippy gate.
    3. fmt drift: inject unformatted code; CI fails at fmt gate.
    4. `cargo deny` finds a forbidden license (inject GPL dep in a test branch); CI fails at deny gate.
    5. Test budget breach (T-22): if `cargo test` exceeds 120s, CI fails with explicit message (use `timeout 120` wrapper).
- **Explicit Swarm Guardrails:** HITL on runner-OS or toolchain changes (G19). Shell gate on `gh workflow run`. Egress allowlist enforced (G16).

### STORY 0.3: Supply-Chain Policy (`deny.toml` + `cargo audit`)
- **User Story:** As the Architect, I want `deny.toml` encoding the T-32 license allowlist + the T-33 RUSTSEC advisory policy, plus `cargo audit` integration, so no forbidden license or known-vulnerable dep can merge (G3/G18).
- **Technical Context:** guardrails.md G3/G18 + nfr-thresholds.md T-32/T-33. `deny.toml` fields: `[licenses] allow = [...]` (T-32 list), `confidence-threshold = 0.93`, `[bans] multiple-versions = "warn"`, `[advisories] db-urls = ["https://github.com/rustsec/advisory-db"]`, `vulnerability = "deny"`, `unmaintained = "warn"`. `cargo audit` runs separately (different DB).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Pull current license list from each pinned crate's `Cargo.toml` (Story 0.1) and confirm against T-32.
  2. [ ] **Implement:** `deny.toml`. Add `cargo-audit` and `cargo-deny` to CI (Story 0.2 dependency).
  3. [ ] **Validate:** Run `cargo deny check bans licenses advisories sources` against the workspace; zero failures.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `cargo deny check licenses` on the v1 dep set returns zero findings.
    2. `cargo audit` on the v1 dep set returns zero unmuted advisories.
  - **Boundary & Edge Case Test Cases:**
    1. Inject a GPL-licensed crate (`gpl-test-crate` placeholder in a test branch); `cargo deny check licenses` MUST fail naming the offending crate.
    2. Inject a known-vulnerable version of a real dep (e.g. an old `chrono` with a RUSTSEC advisory); `cargo audit` MUST fail.
    3. An advisory muted in `deny.toml` with no expiry date → CI fails with "muted advisory missing expiry" (T-33 enforcement).
- **Explicit Swarm Guardrails:** HITL mandatory (G11/G19) — initial supply-chain policy is a project-owner decision. Any future modification to `deny.toml` also requires HITL.

### STORY 0.4: Toolchain Pin (`rust-toolchain.toml`) + Release Profile
- **User Story:** As the Architect, I want the toolchain pinned to MSRV 1.95 + a `[profile.release]` tuned for NFR-3 (cold start) and NFR-4 (RSS), so every contributor and CI uses the same compiler with the same optimizations.
- **Technical Context:** MSRV 1.95 (forced by sysinfo 0.39.3). `[profile.release]`: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` (smaller binary, faster cold start — but REQUIRES G15 panic-safety because `catch_unwind` is no-op under `panic=abort`), `strip = "symbols"`. Documented tradeoff: `panic=abort` means a panicking adapter tears down the process UNLESS the poller spawns each provider call in a subprocess — rejected as too heavy. **Final decision: `panic = "unwind"`** (default) to preserve G15 panic-safety. `lto = "fat"` + `codegen-units = 1` for NFR-3/NFR-4.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `panic = "abort"` is incompatible with G15 panic-catching. Decide `panic = "unwind"` (default).
  2. [ ] **Implement:** `rust-toolchain.toml` (channel = "1.95.0", components = ["clippy", "rustfmt"]). Root `Cargo.toml` `[profile.release]` block.
  3. [ ] **Validate:** `cargo +1.95.0 build --release` produces a binary; `rustc -V` matches.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `rust-toolchain.toml` parses; `rustup show` reports `1.95.0`.
    2. `[profile.release]` round-trips through `cargo build --release` without warnings.
  - **Boundary & Edge Case Test Cases:**
    1. Toolchain mismatch: temporarily bump channel to `2.0.0`; `cargo build` fails with MSRV/distribution error.
    2. `panic = "abort"` regression test: a `catch_unwind` test under default profile (unwind) MUST succeed; document that switching to abort would break G15.
    3. Binary size with `lto = "fat"` is measurably smaller than `lto = false` (assert via `cargo build --release` then `wc -c`; record baseline).
- **Explicit Swarm Guardrails:** HITL on toolchain bump (G19). HITL on `[profile.release]` changes affecting panic strategy (G15 interlock).

### STORY 0.5: Repo Hygiene (`LICENSE`, `README.md`, `SECURITY.md`, `.gitignore`, `Cargo.lock` policy)
- **User Story:** As the Architect, I want the standard OSS repo hygiene files in place so the project is publishable and SignPath-eligible (Story 9.1 depends on `LICENSE` + `README.md`).
- **Technical Context:** `LICENSE` = MPL-2.0 text (matches OHM, compatibility with citation 3 in PRD). `README.md` = project description, build instructions, link to `docs/`. `SECURITY.md` = responsible disclosure + the G16 no-runtime-egress policy. `.gitignore` = `/target/`, `/Cargo.lock` is NOT ignored (binary workspace, G18). `Cargo.lock` committed.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm MPL-2.0 choice vs MIT (PRD says either; MPL-2.0 matches OHM for symbolic alignment but MIT is more permissive — flag as decision).
  3. [ ] **Validate:** Markdown lint clean. SignPath eligibility self-check passes.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `LICENSE` matches the OSI MPL-2.0 text byte-for-byte (fixture: download from opensource.org, hash compare).
    2. `README.md` contains links to all four `docs/` files (grep assertion).
  - **Boundary & Edge Case Test Cases:**
    1. `Cargo.lock` is committed (`.gitignore` does NOT exclude it); test asserts `git ls-files Cargo.lock` returns the file.
    2. `SECURITY.md` explicitly states the G16 no-runtime-egress policy (grep).
    3. `LICENSE` typo injection (replace one word); hash differs from canonical — fixture test catches.
- **Explicit Swarm Guardrails:** HITL on LICENSE choice (MPL-2.0 vs MIT) — affects downstream compatibility. License decision is project-owner.

### STORY 0.6: Workspace Lints + Common Error Type
- **User Story:** As the Architect, I want workspace-wide `[lints.rust]` + `[lints.clippy]` plus a shared `sidebar_error::Error` type so every crate uses uniform error handling and the same lint gates.
- **Technical Context:** Workspace `[lints]`: `unsafe_op_in_unsafe_fn = "deny"`, `missing_docs = "warn"`, `rust_2018_idioms = "deny"`, `clippy::undocumented_unsafe_blocks = "deny"`, `clippy::dbg_macro = "deny"`, `clippy::todo = "deny"`. Error type: `thiserror::Error` enum in a new shared crate OR in `sidebar-domain::error`. Decision: put in `sidebar-domain::error` (no new crate; keeps workspace at 11 packages).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Decide whether to add `thiserror` as a workspace dep (yes; MIT/Apache-2.0, T-32-allowed).
  2. [ ] **Implement:** `[lints]` block in root `Cargo.toml`. `crates/sidebar-domain/src/error.rs` with `Error` enum + `Result<T>` alias.
  3. [ ] **Validate:** `cargo clippy --workspace -- -D warnings` clean. Doctest examples for each error variant.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `Error::Io(io::Error::from(...))` constructs and formats.
    2. `Result::<()>::Err(Error::Config(...))` returns through `?` operator in a test fn.
  - **Boundary & Edge Case Test Cases:**
    1. Inject `dbg!()` in one crate; clippy MUST fail with `clippy::dbg_macro`.
    2. Inject `unsafe` without SAFETY comment; clippy MUST fail with `undocumented_unsafe_blocks`.
    3. `Error` enum exhaustiveness: adding a variant forces all `match` sites to update (compile-time).
- **Explicit Swarm Guardrails:** HITL on adding new workspace lints (could block legitimate patterns).

### STORY 0.7: Dev Environment Scripts (activation + verification + OHM fetch)
- **User Story:** As the Architect, I want `scripts/env.ps1`, `scripts/verify-dev-env.ps1`, and `scripts/fetch_ohm.ps1` so any contributor (human or agentic) can activate the relocatable dev env, verify all prerequisites are present, and acquire the bundled OHM binary deterministically (T-44, `docs/dev-env.md`).
- **Technical Context:** T-44 + `docs/dev-env.md` + Story 6.5. Three PowerShell 7 scripts at `C:\dev\hobby\sidebar\scripts\` in the current workspace:
  1. **`env.ps1`** — Prepends `tools/cargo-bin`, `tools/ci`, `tools/sqlite` to PATH. Verifies system prerequisites (Rust ≥1.95, MSVC linker reachable, PowerShell 7). Dot-source from `$PROFILE` or run per-session.
  2. **`verify-dev-env.ps1`** — Asserts every tool from `docs/dev-env.md` §1 is present and prints a green/red table. Exits non-zero on any failure. Used as a pre-flight by CI and by Story 11.2's regression gate. Checks: rustc ≥1.95, cargo, clippy, rustfmt, `llvm-tools-preview` component, cargo-deny, cargo-audit, cargo-llvm-cov, cargo-nextest, actionlint, sqlite3, gh, scoop, git, MSVC linker (via `cl` or rustc link test), LibreHardwareMonitor.exe + hash match in `resources/`.
  3. **`fetch_ohm.ps1`** — Story 6.5 implementation. Downloads pinned LHM GUI release, SHA-256 verifies against `resources/ohm.sha256`, extracts to `resources/`. Idempotent (skip if hash already matches).
- **Wiring:**
  - **Layer:** integration (the scripts themselves are integration-tested by running them)
  - **Depends-On:** [0.1] (workspace must exist for `env.ps1` to point at)
  - **Blocks:** [6.5] (`fetch_ohm.ps1` IS Story 6.5's implementation), [11.2] (regression gate calls `verify-dev-env.ps1`)
  - **Next:** 1.1
  - **Parallel-With:** [0.3, 0.4, 0.5, 0.6]
  - **DoD:** Running `verify-dev-env.ps1` on this machine (LAPTOP-PLN56DNU) after the user performs dev-env.md §3.1+§3.2 returns all-green; running `env.ps1` puts cargo-deny/llvm-cov/nextest/actionlint on PATH for the shell session.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm PowerShell 7 path resolution (`$PSScriptRoot`, `Split-Path`). Decide idempotency semantics for `fetch_ohm.ps1`.
  2. [ ] **Implement:** Three scripts under `scripts/`. Plus `scripts/README.md` documenting usage.
  3. [ ] **Validate:** Run `verify-dev-env.ps1` on this machine — capture output, iterate until all-green (after user does §3.1+§3.2).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `env.ps1` invoked in a fresh pwsh session → `$env:PATH` contains `tools\cargo-bin`.
    2. `verify-dev-env.ps1` on a correctly-configured machine → exit 0, all rows green.
    3. `fetch_ohm.ps1` (idempotent) — second invocation skips download, logs `"already present, hash matches"`.
  - **Boundary & Edge Case Test Cases (cite T-44):**
    1. `verify-dev-env.ps1` with Rust 1.94 (below MSRV 1.95) → red row, exit non-zero, message names the required version.
    2. `verify-dev-env.ps1` with `llvm-tools-preview` missing → red row naming the component.
    3. `verify-dev-env.ps1` with `LibreHardwareMonitor.exe` missing → red row; suggests running `fetch_ohm.ps1`.
    4. `fetch_ohm.ps1` with hash mismatch (corrupted download) → deletes the bad file, exits non-zero, no partial state.
    5. `fetch_ohm.ps1` with no network (CI sandbox) → times out cleanly within 30s per G16, no hang.
    6. `env.ps1` invoked from bash (not pwsh) → graceful error message ("must run from PowerShell 7"), no partial PATH mutation.
- **Explicit Swarm Guardrails:** HITL on the OHM version pin (R7/G19 — external upstream trust). HITL on any change to `verify-dev-env.ps1`'s prerequisite list (T-44 is a contract).

---

## EPIC 1 — Domain Core (Pure Types & Logic)
- **System Objective:** Implement the pure, no-IO domain layer (`sidebar-domain`) that every adapter, formatter, and the bandwidth accountant depends on.
- **Swarm Mapping:** Domain-Logic Agent.

### STORY 1.1: Core Reading Types (`MetricKind`, `Unit`, `SensorId`, `Reading`, `BatteryState`)
- **User Story:** As the Domain Agent, I want the canonical types defined exactly per architecture §5.1 so all downstream code shares one vocabulary.
- **Technical Context:** architecture.md §5.1. `MetricKind` enum: 35 variants (incl. v2 network + bandwidth). `Unit`: 14 variants (incl. `BitsPerSec`, `PacketsPerSec`). `SensorId { category: &'static str, instance: String }`. `Reading { sensor, kind, value: f64, unit, timestamp: Instant }`. `BatteryState { Charging, Discharging, Idle, Unknown }` (forward-referenced by Story 1.3; MUST be defined here). All `#[derive(Debug, Clone, Copy/PartialEq/Eq/Hash)]` per spec. **Audit hardening:** exhaustive `match` (NOT count assertion) so adding/removing a variant is a compile error everywhere.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Cross-check every `MetricKind` variant against PRD §7 telemetry matrix — every matrix row maps to a variant.
  2. [ ] **Implement:** `crates/sidebar-domain/src/reading.rs`.
  3. [ ] **Validate:** `cargo test -p sidebar-domain reading::`. Use exhaustive `match` in tests (no brittle count).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `SensorId::new("cpu", "package")` round-trips through `Debug`/`PartialEq`/`Hash`.
    2. `Reading { value: 62.0, unit: Celsius, kind: CpuTemperature, ... }` constructs and clones.
    3. Exhaustive match over `MetricKind` returns a documented `&'static str` per variant (compile-time exhaustiveness proof).
  - **Boundary & Edge Case Test Cases (cite T-20, fixture F11 not applicable — pure types):**
    1. `Reading` with `value: f64::NAN`: `PartialEq` MUST NOT equate it to itself (NaN semantics). Document that adapters MUST NOT emit NaN per T-20.
    2. `SensorId` with empty `instance` constructs (legal for global sensors).
    3. `Reading::value` accepts `f64::INFINITY` at construction (no panic) but `format_*` (Story 1.3) MUST render `"--"` per T-20.
- **Explicit Swarm Guardrails:** Commit gate: `cargo clippy -p sidebar-domain -- -D warnings`. No HITL (pure types).

### STORY 1.2: Snapshot + EWMA Smoother + Alert Threshold
- **User Story:** As the Domain Agent, I want `Snapshot` (timestamped `Vec<Reading>` + tier) plus pure smoothing/alerting functions so the GUI renders calm values with threshold alerts.
- **Technical Context:** architecture.md §4 + §7.1. EWMA: pure `fn ewma(history: &[f64], alpha: f64) -> Option<f64>`. Alert: pure `fn check_threshold(value, threshold, hysteresis_band, prev_state) -> AlertState` with hysteresis. `Tier { Basic, Full }` runtime enum. `AlertState { Normal, Warning, Critical }`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `Option<f64>` return for `ewma` (empty history). Define hysteresis state machine.
  2. [ ] **Implement:** `snapshot.rs`, `smooth.rs`, `alert.rs`.
  3. [ ] **Validate:** All-platform unit tests (no Windows deps). 100% line coverage in these modules.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `ewma(&[10.0, 10.0, 10.0], 0.5) == Some(10.0)` within `1e-9` tolerance.
    2. `check_threshold(95.0, threshold=90.0, hysteresis=5.0, prev=Normal) == Warning`.
  - **Boundary & Edge Case Test Cases:**
    1. `ewma(&[], 0.5) == None` (T-20 alignment — no NaN).
    2. Hysteresis flap: oscillation 88→92→88 with threshold 90, hysteresis 5 MUST NOT return to Normal until value < 85.
    3. `check_threshold(f64::NAN, ...)` returns `Normal` (graceful, no panic) per G15.
    4. `check_threshold(f64::INFINITY, threshold=90, ...) == Critical` (mathematically sensible).
- **Explicit Swarm Guardrails:** Coverage gate.

### STORY 1.3: NFR-8 Format Module (Human-Readable Defaults)
- **User Story:** As the Domain Agent, I want the `format` module per architecture §5.4 (AD-13) so every UI value defaults to human-readable output.
- **Technical Context:** architecture.md AD-13 + nfr-thresholds.md T-28/T-29/T-30. Functions: `format_hz(u64) -> String`, `format_bytes(u64, Base) -> String`, `format_bps(u64) -> String`, `format_temp(f64, TempUnit) -> String`, `format_voltage(f64)`, `format_rpm(u32)`, `format_power(f64)`, `format_percent(f64)`, `format_battery(u8, BatteryState)`. Enums `Base { Decimal, Binary }`, `TempUnit { Celsius, Fahrenheit }`. Locale-stable v1. Exact expected outputs from §7.1.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm decimal-GB (10⁹) vs binary-GiB (2³⁰). Confirm °F formula `(c × 9/5) + 32`. Precision rules T-30.
  2. [ ] **Implement:** `crates/sidebar-domain/src/format.rs`. Each function documented with T-30 precision.
  3. [ ] **Validate:** Byte-exact match on every §7.1 case.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. All 10 exact-match assertions from architecture §7.1 (`format_hz(3_840_000_000) == "3.84 GHz"` etc.).
    2. Decimal/binary toggle: `format_bytes(N, Decimal)` vs `format_bytes(N, Binary)` produce correctly-ratioed outputs.
  - **Boundary & Edge Case Test Cases (cite T-20, T-28, T-29, T-30):**
    1. `format_bytes(0, Decimal) == "0 GB"` (T-20: no NaN, no negative).
    2. `format_bytes(u64::MAX, Decimal)` scales to EB without overflow.
    3. `format_temp(f64::NAN, Celsius) == "-- °C"` (T-20, G15).
    4. `format_hz(0) == "0 Hz"` (not `"0 GHz"`).
    5. `format_battery(78, BatteryState::Charging) == "78% (Charging)"`.
    6. `format_battery(255, BatteryState::Unknown) == "-- (Unknown)"` (T-20 sentinel handling).
- **Explicit Swarm Guardrails:** Coverage gate: 100% of public functions.

### STORY 1.4: Billing Pure Functions (`cycle_end`, `next_cycle_start`, `CycleStartDay`)
- **User Story:** As the Domain Agent, I want pure date-arithmetic functions for the bandwidth billing cycle, with the `CycleStartDay` invariant (T-26) enforced at construction, so rollover is fully unit-testable (R9 mitigation).
- **Technical Context:** PRD §5.5.2 + architecture §7.1 + nfr-thresholds.md T-25/T-26/T-27. `CycleStartDay::Day(u8)` (1–28, T-26) + `CycleStartDay::LastDayOfMonth`. `cycle_end(start: CycleStartDay, year: i32, month: u32) -> Option<NaiveDate>`. `next_cycle_start(current_end: NaiveDate) -> NaiveDate`. Timezone contract T-27 (NaiveDate only, "today" = `Local::now().date_naive()`). chrono crate (MIT/Apache-2.0).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Enumerate every edge case as a RED test BEFORE impl (true TDD).
  2. [ ] **Implement:** `crates/sidebar-domain/src/billing.rs`. `Day(u8)` constructor asserts `1 ≤ d ≤ 28` (T-26).
  3. [ ] **Validate:** Unit + proptest (fixture F7). T-25 invariant asserted.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `cycle_end(Day(7), 2026, 7) == Some(2026-08-06)`.
    2. `next_cycle_start(2026-08-06) == 2026-08-07`.
  - **Boundary & Edge Case Test Cases (cite T-25, T-26, T-27, fixture F7):**
    1. Leap year: `cycle_end(Day(29), 2024, 2)` is `Some(...)` (2024 leap); `cycle_end(Day(29), 2023, 2)` returns `None` OR clamps per documented contract (DECIDE in plan: panic vs None vs clamp — PRD §5.5.2 implies clamp; pick clamp and assert).
    2. `LastDayOfMonth`: `cycle_end(LastDayOfMonth, 2026, 1)` → cycle is Jan 31 through Feb 27 (2026 non-leap).
    3. Year boundary: `cycle_end(Day(15), 2026, 12) == Some(2027-01-14)`.
    4. `CycleStartDay::Day(0)` constructor: panics in debug (T-26), clamps + logs in release.
    5. `CycleStartDay::Day(29)` constructor: same — Day variant rejects 29+.
    6. Proptest (F7): for `d ∈ 1..=28`, `year ∈ 2020..=2100`, `month ∈ 1..=12`, `cycle_end - cycle_start ∈ [27, 31]` (T-25).
- **Explicit Swarm Guardrails:** HITL on the Day(29+) rejection contract (G11) — this is marquee-feature policy.

### STORY 1.5: Config Schema + Migration (TOML, `config_version = 1`)
- **User Story:** As the Domain Agent, I want a versioned `Config` struct covering all PRD §3 UX features + v2 bandwidth + theme + dock + monitors + thresholds + hotkeys + first-run flag.
- **Technical Context:** architecture.md AD-9 + §7.1 + nfr-thresholds.md T-3/T-21/T-22/T-34/T-35/T-36/T-37. File: `%APPDATA%\sidebar\config.toml`. Full schema:
  ```toml
  config_version = 1
  first_run_complete = false              # T-37 wizard flag
  poll_interval_seconds = 10              # T-3 (1–60)

  [display]
  temp_unit = "Celsius"                   # T-29
  raw_values = false                      # T-28 toggle
  decimal_base = true                     # T-28 (true=Decimal GB)

  [bandwidth]
  cycle_start_day = { Day = 1 }           # T-26 (Day(1..=28) | LastDayOfMonth)
  tracked_luids = []                      # empty = all non-loopback

  [process]
  top_n = 5                               # T-21 (1–50)

  [graph]
  window = 60                             # T-22 (10–600)

  [theme]
  mode = "Dark"                           # T-35 (Dark|Light|System)
  accent = "#4CAF50"                      # T-35

  [dock]
  edge = "Right"                          # Left|Right|Top|Bottom
  monitor_id = "primary"                  # T-36 (DeviceID or "primary")
  offset_px = 0

  [ohm]                                   # LibreHardwareMonitor subprocess (revised 2026-07-08)
  http_port = 17127                       # T-45 default; OhmSupervisor falls back 17128..17137
  enabled = false                         # Full-mode opt-in; auto-detect may flip this on launch

  [thresholds]                            # PRD §3 UX row "configurable thresholds"
  cpu_temp_warn = 80.0                    # °C
  cpu_temp_critical = 95.0
  gpu_temp_warn = 80.0
  gpu_temp_critical = 95.0

  [hotkeys]                               # T-34
  click_through = "Ctrl+Shift+S"

  [metrics]                               # PRD §3 UX row "per-metric enable/disable + reorder"
  enabled = ["CpuUtilization", "CpuFrequency", "MemoryUsed", ...]  # MetricKind names
  order = ["CpuUtilization", "CpuFrequency", ...]                  # display order
  ```
  Migration: `migrate(raw: toml::Value) -> Config`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm clamping rules T-3/T-21/T-22. Confirm `[theme] mode = System` requires runtime reg-query (Story 8.6 implements; Config just stores the enum). Confirm `[dock] monitor_id = "primary"` is the sentinel for primary monitor.
  2. [ ] **Implement:** `crates/sidebar-domain/src/config.rs`. All sections above. Default impl returns the documented defaults.
  3. [ ] **Validate:** Round-trip + migration tests. Fixture F1 (TempDir).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `Config::default()`: all fields match documented defaults (poll=10, temp=Celsius, raw=false, decimal=true, cycle=Day(1), top_n=5, window=60, theme=Dark, accent=#4CAF50, edge=Right, monitor=primary, first_run=false).
    2. Round-trip: `Config` → TOML → parse → equals original.
    3. All `[metrics] enabled` entries are valid `MetricKind` names (cross-check with Story 1.1 exhaustive list).
  - **Boundary & Edge Case Test Cases (cite T-3/T-21/T-22/T-26/T-34/T-35/T-36/T-37):**
    1. `poll_interval_seconds = 0` → clamped to 1 + `warn!` (T-3).
    2. `poll_interval_seconds = 999` → clamped to 60.
    3. Missing `[bandwidth]` in v0 config → migrated to default `Day(1)`.
    4. `top_n = 0` → 1; `top_n = 999` → 50 (T-21).
    5. `cycle_start_day = { Day = 29 }` → rejected, clamped to `Day(28)` + `warn!` (T-26).
    6. `[theme] accent = "not-a-color"` → fallback to default `#4CAF50` + `warn!`.
    7. `[dock] edge = "Sideways"` → invalid, fallback to `Right` + `warn!`.
    8. `[hotkeys] click_through = ""` → empty string treated as "hotkey disabled" (documented).
    9. `[metrics] enabled` contains unknown MetricKind name → that entry dropped + `warn!`.
    10. `first_run_complete` missing → treated as `false` (wizard runs).
- **Explicit Swarm Guardrails:** Coverage gate. HITL if any new section is added (config schema is a contract).

### STORY 1.6: Aggregate (top-N) + Graph (rolling window)
- **User Story:** As the Domain Agent, I want pure top-N process selection and rolling-window sparkline functions.
- **Technical Context:** architecture.md §4 + §7.1 + T-21/T-22. `top_n(readings: &[Reading], kind: MetricKind, n: usize) -> Vec<&Reading>`. `RollingWindow::new(max_len: usize)`, `push_and_evict(&mut self, value: f64)`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Stable-sort requirement (ties broken by insertion order).
  2. [ ] **Implement:** `aggregate.rs`, `graph.rs`.
  3. [ ] **Validate:** Edge cases n=0, n > len, empty window.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `top_n` with 5 process-CPU readings, n=3 → top 3 descending.
    2. `RollingWindow::new(10)` with 12 pushes → first 2 evicted (T-22).
  - **Boundary & Edge Case Test Cases:**
    1. `top_n([], ..., 3)` → empty (no panic).
    2. `top_n(readings, ..., 0)` → empty.
    3. Ties: stable order preserved.
    4. `RollingWindow` push `f64::NAN` → behavior documented (recommend: store but mark; sparkline renders gap).
    5. `RollingWindow::new(0)` → constructor panics OR accepts and always evicts immediately (DECIDE; recommend panic, violates T-22 lower bound).
- **Explicit Swarm Guardrails:** Coverage gate.

---

## EPIC 2 — Sensor Abstraction & Cost Classifier
- **System Objective:** Define `SensorProvider`, `SensorDescriptor`, `CostClass`, `Tier`, and the compile-time `classify_for_v1` gate (NFR-1 enforcement).
- **Swarm Mapping:** Sensor-Framework Agent.

### STORY 2.1: SensorProvider Trait + Mockall Auto-Mock
- **User Story:** As the Sensor Agent, I want the `SensorProvider` trait with `mockall::automock` so every adapter implements one contract and domain logic can be tested against canned readings (AD-4).
- **Technical Context:** architecture.md §5.2. `trait SensorProvider: Send + Sync { fn descriptor(&self) -> &SensorDescriptor; fn read_all(&self) -> Vec<Reading>; }`. `#[cfg_attr(test, mockall::automock)]`. Counter-vs-gauge semantics (§5.2 v2 note).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `read_all` is sync; adapters use `spawn_blocking` if blocking. Document in trait doc-comment.
  2. [ ] **Implement:** `crates/sidebar-sensor/src/provider.rs`.
  3. [ ] **Validate:** `MockSensorProvider` returns canned readings (fixture F4).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F4:**
    1. `MockSensorProvider` returns `vec![Reading {...}]`; caller receives it.
    2. Multiple mocks register independent canned data.
  - **Boundary & Edge Case Test Cases:**
    1. Mock returns empty `Vec<Reading>` — caller handles.
    2. Mock panics on second call — caller does not double-poll (assert call count = 1).
    3. `Arc<dyn SensorProvider>` crosses threads — `Send + Sync` proven via `static_assertions::assert_impl_all`.
- **Explicit Swarm Guardrails:** HITL architectural review (keystone trait).

### STORY 2.2: SensorDescriptor + CostClass + Tier Enums
- **User Story:** As the Sensor Agent, I want descriptor/cost-class/tier types so adapters self-declare cost and tier (AD-5, AD-7).
- **Technical Context:** architecture.md §5.3. `CostClass { Lightweight, Watch, Heavy, Deferred }`. `SensorDescriptor { name: &'static str, cost_class, metrics: &'static [MetricKind], requires_tier: Tier }`. `Tier { Basic, Full, Both }`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Per-adapter expected (CostClass, Tier) oracle as a test.
  2. [ ] **Implement:** `crates/sidebar-sensor/src/descriptor.rs`.
  3. [ ] **Validate:** Type-level + unit tests.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Construct `SensorDescriptor::new("sysinfo-cpu", CostClass::Lightweight, &[CpuUtilization, CpuFrequency], Tier::Basic)`.
    2. Exhaustive `match` over `CostClass` (compile-time exhaustiveness).
  - **Boundary & Edge Case Test Cases:**
    1. `metrics: &[]` — legal but documented as suspicious.
    2. `Tier::Both` semantics documented (tier-agnostic; runs in both modes).
- **Explicit Swarm Guardrails:** None.

### STORY 2.3: `classify_for_v1` — Compile-Time Cost + Tier Gate
- **User Story:** As the Sensor Agent, I want `classify_for_v1` filtering Heavy/Deferred + tier-incompatible sources, so NFR-1 is enforced at registry-build time.
- **Technical Context:** architecture.md §5.4 + T-1. Filter: accept Lightweight + Watch; reject Heavy + Deferred with `tracing::warn!` containing structured fields `{ source: name, cost_class: X, reason: Y }` for orchestrator audit. Tier filter: Both runs both; Basic runs both; Full runs only when `active_tier=Full`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Structured log field names.
  2. [ ] **Implement:** `crates/sidebar-sensor/src/classifier.rs`.
  3. [ ] **Validate:** All 4 cost-class × 3 tier combos tested.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `[Lightweight/Basic, Watch/Full]` + `active_tier=Full` → both retained.
    2. `[Heavy/Basic, Deferred/Both]` → both rejected, two `warn!` emitted with structured fields.
  - **Boundary & Edge Case Test Cases:**
    1. `Tier::Full` descriptor + `active_tier=Basic` → rejected silently (no warn).
    2. `Tier::Both` runs in both modes (parametrized).
    3. Empty input → empty output.
    4. Duplicate descriptors (same name) → both retained (document; dedup is NOT the classifier's job).
    5. `warn!` fields verified via `tracing_subscriber::layer()` capture in test.
- **Explicit Swarm Guardrails:** HITL on `warn!` field schema (G11). CI gate: no `Heavy`/`Deferred` in v1 registry without waiver comment.

---

## EPIC 3 — Adapter Implementations
- **System Objective:** Implement every concrete `SensorProvider` adapter.
- **Swarm Mapping:** Adapter Agent (per-crate), LHM HTTP Agent for `/data.json`.

### STORY 3.1: `sidebar-adapter-sysinfo` (CPU/RAM/disk/processes/uptime)
- **User Story:** As the Adapter Agent, I want a sysinfo-backed provider for CPU util, freq, RAM, disk capacity, processes, uptime.
- **Technical Context:** AD-3 + §7.2. `sysinfo = 0.39.3`. **State container:** `Mutex<System>` (sysinfo requires `&mut self` to refresh). `Tier::Basic`, `CostClass::Lightweight`. Refresh strategy: `refresh_cpu()` + `refresh_memory()` + `refresh_processes()` per tick. Emits per-core + aggregate CPU util, freq, RAM used/total, disk used/total, processes, uptime.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm sysinfo API. Confirm refresh cost (NFR-1, T-1).
  2. [ ] **Implement:** `crates/sidebar-adapter-sysinfo/src/lib.rs` with `Mutex<System>`.
  3. [ ] **Validate:** Integration on Windows CI; unit-test Reading construction with mock sysinfo trait.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock sysinfo 8 cores → 8 + 1 aggregate `CpuUtilization` readings.
    2. Mock sysinfo RAM 8/16 GB → 2 readings.
  - **Boundary & Edge Case Test Cases (cite T-20):**
    1. 0 processes → empty process readings.
    2. Empty disk list → no DiskUsed.
    3. CPU usage exactly 100.0 → reading value 100.0.
    4. Two rapid `read_all` calls → `Mutex<System>` allows both; second reflects refreshed data.
    5. sysinfo returns NaN-typed value (cannot, but if it did) → adapter skips that reading (T-20).
- **Explicit Swarm Guardrails:** Windows-only integration. Shell gate on sysinfo version bump.

### STORY 3.2: `sidebar-adapter-nvml` (NVIDIA GPU)
- **User Story:** As the Adapter Agent, I want an nvml-wrapper-backed NVIDIA GPU provider.
- **Technical Context:** AD-3. `nvml-wrapper = 0.12.0`. NVML init failure → empty readings + `NvmlUnavailable` flag. `Tier::Basic`, `CostClass::Lightweight`. Per T-13, each NVML call wrapped in `tokio::time::timeout(100ms, spawn_blocking(...))`.
- **⚠️ Local-test caveat (per `docs/dev-env.md` §1.1/§6.2):** This dev machine (LAPTOP-PLN56DNU) has **no NVIDIA GPU** — only an AMD Radeon 860M iGPU. NVML integration tests for this story are `#[ignore]`'d locally and MUST run on a CI runner (or a different dev machine) with NVIDIA hardware. The unit tests (mock NVML via `Nvml::init()` test stubs) still run everywhere. AMD GPU coverage on this machine is via Story 3.6 (OHM Full mode) only. R5 in PRD §8 already documents the AMD-coverage-via-OHM-only design choice.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** NVML lifecycle (NVML::init? lazy?).
  2. [ ] **Implement:** `crates/sidebar-adapter-nvml/src/lib.rs`.
  3. [ ] **Validate:** Windows CI `#[ignore]` if no NVIDIA.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock NVML 42% util, 65°C → 2 readings.
    2. NVML init fails → empty vec, `debug!`, no panic.
  - **Boundary & Edge Case Test Cases (cite T-13):**
    1. 0 GPUs → empty.
    2. 2 GPUs → `instance "0"` and `"1"`.
    3. NVML call exceeds 100ms (T-13) → returns empty, logs.
    4. NVML error mid-poll → partial readings, logged.
- **Explicit Swarm Guardrails:** HITL on NVML error taxonomy. `#[ignore]` runnable via `--ignored`.

### STORY 3.2b: `sidebar-adapter-nvml` Process-GPU (Watch — conditional)
- **User Story:** As the Adapter Agent, I want per-process GPU% via NVML, classified Watch, conditional on bench result (OQ-2).
- **Technical Context:** PRD §3 Tier 3 + §7.3. Behind `feature = "proc-gpu"`. Bench result gates inclusion in default build per T-1.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Write bench FIRST.
  2. [ ] **Implement:** Process-GPU path behind feature flag.
  3. [ ] **Validate:** Bench on reference HW. >0.5% → feature off in default.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock NVML 3 processes 10/20/30% → 3 `ProcessGpuPercent` readings.
  - **Boundary & Edge Case Test Cases (cite T-1):**
    1. 0 processes → empty.
    2. Bench: 5-min simulated poll measures adapter CPU% — MUST be ≤0.5% (T-1).
    3. Feature off → no `ProcessGpuPercent` readings.
- **Explicit Swarm Guardrails:** HITL on OQ-2 ship/defer decision (G11).

### STORY 3.3: `sidebar-adapter-battery` (starship-battery)
- **User Story:** As the Adapter Agent, I want a battery provider.
- **Technical Context:** AD-3. `starship-battery = 0.10`. Emits `BatteryPercent`, `BatteryState`, `BatteryPowerRate`. `Tier::Basic`, `CostClass::Lightweight`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Handle no-battery desktops.
  2. [ ] **Implement:** `crates/sidebar-adapter-battery/src/lib.rs`.
  3. [ ] **Validate:** Windows CI gated on battery presence.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock battery 78% charging → 3 readings; `format_battery(78, Charging) == "78% (Charging)"` end-to-end (cross-check with Story 1.3).
  - **Boundary & Edge Case Test Cases:**
    1. No battery → empty.
    2. 100% idle → `BatteryState::Idle`.
    3. Rate sign convention documented (negative on AC = charging).
- **Explicit Swarm Guardrails:** None.

### STORY 3.4: `sidebar-adapter-pdh` (Per-drive R/W throughput)
- **User Story:** As the Adapter Agent, I want a PDH-backed per-drive R/W bytes/sec provider.
- **Technical Context:** AD-3 + §7.2. `windows = 0.62.2` PDH. `\PhysicalDisk(*)\Disk Read Bytes/sec`, `\Disk Write Bytes/sec`. `Tier::Basic`, `CostClass::Lightweight`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** PDH counter path on Win11 24H2.
  2. [ ] **Implement:** `crates/sidebar-adapter-pdh/src/lib.rs`. **All `unsafe` PDH calls get SAFETY comments (G2); tests use fixture F11.**
  3. [ ] **Validate:** Integration under synthetic disk load.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock PDH C: read 1 MB/s, write 2 MB/s → 2 readings.
  - **Boundary & Edge Case Test Cases:**
    1. PDH unavailable → empty, `debug!`.
    2. Zero-activity drive → value 0.0 (not omitted).
    3. Hot-plugged USB drive → picked up next tick.
    4. SAFETY: every `unsafe` block has `// SAFETY:` comment (CI lint G2).
- **Explicit Swarm Guardrails:** Shell gate on Windows-only test. HITL on any new `unsafe`.

### STORY 3.5: `sidebar-adapter-net` (Per-NIC via GetIfTable2) — v2 MARQUEE
- **User Story:** As the Adapter Agent, I want a per-NIC throughput provider using `GetIfTable2` so live RX/TX counters are surfaced.
- **Technical Context:** AD-12 + §5.2 + §7.2 + T-23/T-24. `windows` crate `GetIfTable2` snapshots `MIB_IF_TABLE2`; implementation filters live non-loopback rows and frees the table. **Adapter emits RAW cumulative counters** (§5.2 v2 note, G9); delta-and-divide downstream. `Tier::Both`, `CostClass::Lightweight`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm LUID stability T-24. Decide tracked-NIC discovery.
  2. [ ] **Implement:** `crates/sidebar-adapter-net/src/lib.rs`. **All `unsafe` per F11 with SAFETY comments.**
  3. [ ] **Validate:** Integration asserts monotonic non-decreasing counters; LUID stable within process (T-24).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Two ticks: `InOctets=1000` → `2000` → domain-layer delta = 1000 bytes (cross-checked in accountant test).
    2. Adapter emits per-LUID readings with `SensorId.instance` = LUID-as-string.
  - **Boundary & Edge Case Test Cases (cite T-23/T-24):**
    1. Counter wraparound T-23 → domain-layer treats as reset (test in Story 5.1).
    2. NIC disappears → adapter skips, no panic.
    3. NIC reappears → resumes; LUID matches.
    4. Zero NICs → empty.
    5. SAFETY: every `unsafe` FFI block documented (CI lint).
- **Explicit Swarm Guardrails:** HITL on LUID stability assumption (G11) — R10 fallback to MAC if sdd-verify disproves. HITL on any new `unsafe`.

### STORY 3.6: `sidebar-adapter-ohm` (LHM HTTP bridge) — Full mode *(revised 2026-07-08 — was WMI)*
- **User Story:** As the OHM Agent, I want an HTTP-backed provider that `GET`s `http://127.0.0.1:17127/data.json` from the bundled LibreHardwareMonitor subprocess, parsing the JSON sensor tree for CPU temp/power/fan/voltage, AMD/Intel GPU, SSD SMART/temp.
- **Technical Context:** AD-2 (revised) + AD-7 (revised) + §7.2 + T-10 + T-45. **`ureq = 2.x`** (sync HTTP client; replaces the `wmi` crate). **HTTP probe contract** (verified from LHM `HttpServer.cs` on master, retrieved 2026-07-08):
  1. `GET /data.json` → JSON array of `LhmNode { id, text, children, min, value, max, imageindex, type }` where `type ∈ {"Node","Sensor"}`. The root is a Node array; sensor leaves carry `value` + parent path.
  2. Port = 17127 default (T-45), configurable. Adapter receives the resolved port from `OhmSupervisor` (Story 6.4) — does not pick its own.
  3. One GET per tick (NFR-1 Lightweight — one HTTP roundtrip + one `serde_json` deserialization; sub-millisecond on localhost).
  500ms timeout T-10. **No COM init** (was required for WMI; no longer needed — HTTP has no apartment semantics). `Tier::Full`, `CostClass::Lightweight`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Map LHM node types (`Sensor` leaves with parent `Node` paths) → `MetricKind` + `SensorId`. Define `LhmNode` `serde` struct. Verify `/data.json` JSON shape against a real LHM v0.9.6 capture (saved fixture under `tests/fixtures/lhm_data.json`).
  2. [ ] **Implement:** `crates/sidebar-adapter-ohm/src/lib.rs`. **`ureq` sync GET inside `spawn_blocking`** (per architecture AD-6 — `read_all` is sync but blocking; the poller wraps it). Tests `#[ignore]` without LHM running.
  3. [ ] **Validate:** Integration gated on LHM installed + running on port 17127 (manually launch `resources/LibreHardwareMonitor.exe` for `--ignored` tests).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture: saved `tests/fixtures/lhm_data.json`:**
    1. Parse the saved fixture; assert it yields expected `CpuTemperature`, `FanSpeed`, `Voltage` readings with correct `SensorId.category/instance`.
    2. Adapter uses a mock `ureq`-trait (`trait HttpClient { fn get(&self, url: &str) -> Result<String>; }`) so unit tests don't hit the network.
  - **Boundary & Edge Case Test Cases (cite T-10, T-45):**
    1. HTTP connection-refused (LHM not running) → empty readings, `debug!`, no panic.
    2. HTTP timeout 500ms (T-10) → empty, `debug!`, no hang.
    3. Non-LHM service on port 17127 (returns HTML 404) → JSON parse fails → empty, `warn!`.
    4. Malformed LHM JSON (missing `value` field on a sensor) → that node skipped, others returned.
    5. Two CPUs (dual-socket) → `SensorId.instance = "cpu/0"` and `"cpu/1"` derived from LHM node `id`.
    6. LHM v0.9.6 vs v0.9.7 schema drift (new field added) → `serde(default)` tolerance, no fail.
- **Explicit Swarm Guardrails:** HITL on HTTP timeout T-10 (G11). HITL on `ureq` version (R2 — prefer maintained sync client). Shell gate. **Local-test note:** This dev machine (LAPTOP-PLN56DNU, AMD Ryzen AI 7 350) is the IDEAL LHM test target — v0.9.6 has Ryzen AI 300-series support. `#[ignore]`'d integration tests run cleanly here after `scripts/fetch_ohm.ps1` + manual LHM launch.

---

## EPIC 4 — Persistence Layer (SQLite)
- **System Objective:** Implement the SQLite-backed bandwidth state store (AD-11).
- **Swarm Mapping:** DB Agent.

### STORY 4.1: Schema Init + PRAGMAs
- **User Story:** As the DB Agent, I want the `current_cycle`, `bandwidth_history`, and `current_cycle_metadata` tables + WAL/`user_version` PRAGMAs.
- **Technical Context:** AD-11 + T-6/T-12/T-17/T-26 + G21. Tables per architecture.md AD-11 SQL block. PRAGMAs: `user_version = 2`, `journal_mode = WAL`, `foreign_keys = ON`, default `wal_autocheckpoint`. `adapter_luid` stored as `INTEGER` (SQLite 64-bit signed; LUID is 64-bit — confirm no overflow, T-26 adjacent).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `rusqlite` `bundled` feature compiles sqlite3 (~1 MB, T-6). Confirm INTEGER width.
  2. [ ] **Implement:** `crates/sidebar-persistence/src/schema.rs`. `init(conn: &Connection) -> Result<()>`. **Connection acquisition pattern:** prod opens `%APPDATA%\sidebar\bandwidth.db`; tests use TempDir (F1).
  3. [ ] **Validate:** Round-trip on TempDir SQLite (F1, F6).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixtures F1, F6:**
    1. Fresh TempDir DB → `init()` → `PRAGMA user_version == 2`.
    2. `PRAGMA journal_mode == "wal"`.
    3. `init()` called twice → idempotent (F6).
  - **Boundary & Edge Case Test Cases:**
    1. Corrupt/non-SQLite file at path → `init()` returns Err, no overwrite.
    2. Read-only FS → Err with clear message.
    3. `adapter_luid` insert of `u64::MAX` → round-trips as `i64` (LUID is 64-bit; verify no sign issues — DECIDE: store as `i64` reinterpret-cast).
- **Explicit Swarm Guardrails:** HITL on `adapter_luid` integer-width (G11). Shell gate on `rusqlite` version. G21 (all SQLite via `sidebar-persistence`).

### STORY 4.2: Bandwidth Repo (save / load / archive / prune)
- **User Story:** As the DB Agent, I want repo functions for the rollover lifecycle.
- **Technical Context:** §7.1 + T-12/T-16 + G21 + R11. `save_accumulator`, `load_current_cycle`, `archive_cycle`, `prune_history(keep=1)` (T-16). Busy-retry per T-12 (5 attempts, ≤310ms). Each archive = one transaction. **Crash recovery on next launch (R11 mitigation):** On startup, `load_current_cycle()` reads the existing accumulator state. If the DB is in a dirty state (WAL not checkpointed due to prior crash), SQLite's journal-rollback recovers automatically on `Connection::open`. If `updated_at` on a `current_cycle` row is older than the cycle_start of the *current* date, the row is stale (sidebar was down across a rollover boundary) → the accountant's first tick detects this and archives the stale cycle before accumulating fresh data.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Txn boundaries. Busy-retry wrapper.
  2. [ ] **Implement:** `crates/sidebar-persistence/src/bandwidth_repo.rs`.
  3. [ ] **Validate:** Round-trip + archive + prune on TempDir (F1).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Save `{luid, rx: 1_000_000, tx: 2_000_000, cycle_start}` → reload → byte-equal.
    2. Archive → history gains row with `cycle_end`; current reset.
  - **Boundary & Edge Case Test Cases (cite T-12, T-16):**
    1. `prune_history(keep=1)` with 5 historical rows → most recent 1 retained.
    2. Save new LUID → INSERT (upsert).
    3. Save existing LUID → UPDATE.
    4. Concurrent save (two threads) → SQLite busy; T-12 retry ceiling (5 attempts) respected, then Err if still busy.
- **Explicit Swarm Guardrails:** G21.

### STORY 4.3: Migration (`v0_to_v2`)
- **User Story:** As the DB Agent, I want a migration module tracking schema via `user_version`.
- **Technical Context:** §7.1 + AD-11 + G21. `migrate(conn) -> Result<()>` reads `user_version`, applies sequential migrations in a single transaction each.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Migration registry pattern.
  2. [ ] **Implement:** `crates/sidebar-persistence/src/migrate.rs`.
  3. [ ] **Validate:** v0→v1→v2 and legacy-v1→v2 tests (F1, F6).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Empty DB (`user_version = 0`) → migrate → `user_version = 2`, including `current_cycle_metadata`.
    2. Existing v1 DB → migrate → `user_version = 2`, creating `current_cycle_metadata`.
    3. Already-v2 → migrate → no-op.
  - **Boundary & Edge Case Test Cases:**
    1. `user_version = 99` → Err "unknown future schema".
    2. Migration fails mid-way (inject fault) → txn rolls back, `user_version` unchanged.
- **Explicit Swarm Guardrails:** G21.

---

## EPIC 5 — Bandwidth Accountant
- **System Objective:** Implement the `BandwidthAccountant` task (architecture §6, flows F/G/H/I).
- **Swarm Mapping:** Async-Orchestration Agent.

### STORY 5.1: MonthlyAccumulator (in-memory)
- **User Story:** As the Domain Agent, I want an in-memory `MonthlyAccumulator` keyed on LUID with wraparound handling (T-23).
- **Technical Context:** `crates/sidebar-bandwidth/src/accumulator.rs`. `MonthlyAccumulator { by_luid: HashMap<u64, AccEntry> }`. `AccEntry { cycle_start: NaiveDate, rx_bytes: u64, tx_bytes: u64, prev_rx_counter: Option<u64>, prev_tx_counter: Option<u64> }`. `add_delta` per T-23.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Wraparound contract T-23.
  2. [ ] **Implement:** `accumulator.rs`.
  3. [ ] **Validate:** Wraparound + multi-LUID tests (F7 proptest).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `add_delta(luid=1, rx=100, tx=50)` then `add_delta(luid=1, rx=150, tx=70)` → rx_bytes=50, tx_bytes=20.
    2. Two LUIDs accumulate independently.
  - **Boundary & Edge Case Test Cases (cite T-23, fixture F7):**
    1. Wraparound T-23: prev=2e9, current=1000 → delta=1000.
    2. First call (prev=None) → delta=0 (baseline).
    3. rx=0, tx=0 → no accumulation, no panic.
    4. Proptest (F7): random valid counter sequences; cumulative rx_bytes always equals sum of deltas.
- **Explicit Swarm Guardrails:** None (pure).

### STORY 5.2: Accountant Task (subscribe + accumulate + flush + rollover)
- **User Story:** As the Async Agent, I want the `BandwidthAccountant` tokio task (architecture §6, flows F/G).
- **Technical Context:** §6 + T-15/T-19/T-27 + G15. Holds `broadcast::Receiver<Vec<Reading>>`. Filters NetRxBytes/NetTxBytes. Flush debounce 60s (T-15) + on shutdown + on rollover. **Injectable `Clock`** (fixture F3) — signature: `pub trait Clock: Send + Sync { fn now(&self) -> chrono::NaiveDateTime; }`. Production uses `SystemClock`; tests use `FakeClock`. Rollover check `clock.now().date_naive() >= cycle_end` (T-27). Uses `sidebar-persistence::bandwidth_repo`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Clock trait (F3). Flush error handling per G15 (catch, log, continue).
  2. [ ] **Implement:** `crates/sidebar-bandwidth/src/accountant.rs`.
  3. [ ] **Validate:** E2E with mock broadcast + TempDir SQLite + FakeClock (F1, F2, F3).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixtures F1, F2, F3:**
    1. 2 ticks of NetRxBytes → accumulator totals correct → flush → DB `current_cycle` rows match.
    2. Tick contains non-network readings → ignored.
  - **Boundary & Edge Case Test Cases (cite T-15, T-19, T-23, T-27; G15):**
    1. Rollover: FakeClock advances past `cycle_end` (T-27) → archive called, new cycle starts at 0, force-flush.
    2. Two rollovers in sequence → history has 2 rows.
    3. Broadcast sender drops (poller crash) → accountant exits with final flush (G15).
    4. Flush fails (Simulate SQLite disk full via TempDir permission flip) → error logged, accountant continues (G15).
    5. Rapid 100 ticks within 60s debounce (T-15) → only 1 flush.
    6. Shutdown signal mid-flush → graceful within T-19 (3000ms) grace.
- **Explicit Swarm Guardrails:** HITL on Clock trait contract (G11). Shell gate on tokio version. G15 panic-safety.

### STORY 5.3: BandwidthView DTO + Builder
- **User Story:** As the Domain Agent, I want a `BandwidthView` DTO so the GUI renders without touching SQLite (flow H).
- **Technical Context:** §4 (`view.rs`). `BandwidthView { current: Vec<NICtotals>, history: Vec<NICtotals>, days_until_reset: u32, next_reset_date: NaiveDate }`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `friendly_name` from `MIB_IF_ROW2.InterfaceAlias` cached by LUID.
  2. [ ] **Implement:** `crates/sidebar-bandwidth/src/view.rs`.
  3. [ ] **Validate:** Builder test with synthetic data.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Build view from accumulator {luid=1, rx=1GB, tx=2GB} + history [{luid=1, rx=5GB, tx=6GB}] → 1 current + 1 history entry, `days_until_reset` computed via FakeClock (F3).
  - **Boundary & Edge Case Test Cases:**
    1. Empty accumulator + empty history → empty vecs, `days_until_reset` = full cycle.
    2. `days_until_reset` when today == cycle_end-1 → 1.
    3. `days_until_reset` when today == cycle_end → 0.
    4. NIC in history not in current (disconnected) → history retained.
- **Explicit Swarm Guardrails:** None.

---

## EPIC 6 — Platform Layer (Win32)
- **System Objective:** Win32 integration: transparent topmost window, AppBar, DWM, DPI v2, OhmSupervisor.
- **Swarm Mapping:** Win32-Native Agent.

### STORY 6.1: Transparent Borderless Topmost Viewport + Peek Exclusion + Capture Exclusion
- **User Story:** As the Win32 Agent, I want an egui/eframe viewport that is transparent, borderless, always-on-top, DWM-peek-excluded, and optionally excluded from supported capture APIs for streamers (AD-1, NFR-7, R4).
- **Technical Context:** AD-1 + NFR-7 + §7.4 + T-9 + T-31. `eframe::ViewportBuilder::with_transparent(true)`, `clear_color([0,0,0,0])`, `Frame::none()`. `SetWindowPos(HWND_TOPMOST)`. **Two distinct APIs:**
  1. `DwmSetWindowAttribute(DWMWA_EXCLUDED_FROM_PEEK, TRUE)` — sidebar doesn't disappear during Aero Peek (Win+Tab, hover-show-desktop).
  2. `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` — optional capture exclusion; **default OFF** (`[display] hide_from_capture = false`).
  egui 0.35 (OQ-3).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Verify `ViewportBuilder::with_transparent` in 0.35 and `WDA_EXCLUDEFROMCAPTURE` on Win11 24H2. Manual smoke on Win11 24H2 mandatory (R4).
  2. [ ] **Implement:** `crates/sidebar-platform/src/window.rs` + `dwm.rs`. **All `unsafe` FFI per F11 with SAFETY comments.** `dwm::exclude_from_peek(hwnd)` + `dwm::set_capture_cloak(hwnd, bool)`.
  3. [ ] **Validate:** Manual smoke items 1–4 + "capture exclusion: verify sidebar NOT visible in OBS preview when toggle on".
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `ViewportBuilder` construction returns `transparent == true` (mockable).
    2. `dwm::exclude_from_peek(hwnd)` calls `DwmSetWindowAttribute` with `DWMWA_EXCLUDED_FROM_PEEK` (mock verify).
    3. `dwm::set_capture_cloak(hwnd, true)` calls `SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE` (mock/shape verify).
  - **Boundary & Edge Case Test Cases:**
    1. DWM unavailable → graceful no-op, logged.
    2. `SetWindowPos` fails → logged, app continues (non-fatal per G15).
    3. Manual smoke: transparency fails on specific GPU driver → R4 materialized; document workaround.
    4. SAFETY comment presence (CI lint G2).
    5. Capture exclusion unsupported (older Windows/invalid HWND) → non-fatal warning and continue.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11) — manual smoke on real Win11, capture exclusion is a streamer-privacy feature that needs visual review. HITL on any `unsafe`. Shell gate on egui version.

### STORY 6.2: AppBar Dock Registration (SHAppBarMessage)
- **User Story:** As the Win32 Agent, I want AppBar registration so the sidebar reserves edge space.
- **Technical Context:** §4 (`appbar.rs`) + NFR-6. `SHAppBarMessage` `ABM_NEW/QUERYPOS/SETPOS/REMOVE`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Unregister on shutdown (don't leak). Monitor-change re-dock.
  2. [ ] **Implement:** `crates/sidebar-platform/src/appbar.rs`. **`unsafe` per F11.**
  3. [ ] **Validate:** Manual smoke 4–5.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `appbar::register(hwnd, Left, primary)` → `ABM_NEW` + `ABM_SETPOS` (mock).
    2. `appbar::unregister(hwnd)` → `ABM_REMOVE`.
  - **Boundary & Edge Case Test Cases:**
    1. Non-primary monitor → correct `rc`.
    2. Double-register → no-op or returns existing.
    3. Unregister without register → no-op.
    4. Monitor disconnect → re-dock to primary or hide (documented).
- **Explicit Swarm Guardrails:** HITL smoke on multi-monitor. HITL on `unsafe`.

### STORY 6.3: Per-Monitor DPI Awareness v2
- **User Story:** As the Win32 Agent, I want per-monitor DPI v2 (NFR-6).
- **Technical Context:** NFR-6. `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` before window creation.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** API exists on Win11 24H2.
  2. [ ] **Implement:** `crates/sidebar-platform/src/dpi.rs`. **`unsafe` per F11.**
  3. [ ] **Validate:** Manual smoke 6.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `dpi::set_per_monitor_v2()` → Ok on Win11.
    2. `GetDpiForWindow` returns monitor DPI (mock).
  - **Boundary & Edge Case Test Cases:**
    1. API fails (older Windows) → fallback to system DPI, logged.
    2. Calling twice → no-op.
- **Explicit Swarm Guardrails:** HITL on `unsafe`.

### STORY 6.4: OhmSupervisor (subprocess launch + monitor + teardown) *(revised 2026-07-08 — was WMI)*
- **User Story:** As the Win32 Agent, I want the `OhmSupervisor` (AD-8, §3 flow D/E, G10) — probe the LHM HTTP endpoint, write the resolved port into LHM config, launch bundled LibreHardwareMonitor via `ShellExecuteW("runas")` on user action, monitor, tear down.
- **Technical Context:** AD-8 + §6 + AD-7 (revised) + T-10/T-11/T-45 + G10. **`OhmSupervisor::probe()` runs `GET http://127.0.0.1:17127/data.json` via `ureq` with 500ms timeout T-10.** If 200 + body looks like LHM JSON signature (top-level array, first element has `Text` and `Children`) → Full. If connection-refused/timeout → Basic. `launch_elevated()` picks a free port (17127..17137), patches `LibreHardwareMonitor.exe.config` (`runWebServerMenuItem=true`, `listenerPort=<chosen>`), invokes `ShellExecuteW("runas")` (5s launch timeout T-11), then re-probes. Job Object wrapping (G10) reaps sidebar-launched LHM on host crash; shutdown kills only sidebar-owned children. **The current integration slice wires the app-level monitor/degrade task; it gates degradation to children explicitly launched by the sidebar. Real UAC/LHM smoke remains HITL.**
- **⚠️ Verified-fact note (researched 2026-07-08 during dev-env certification):** LHM v0.9.6's HTTP server defaults to OFF (`runWebServerMenuItem=false` in `MainForm.cs`) and defaults to port 8085 (`_settings.GetValue("listenerPort", 8085)`). Both must be set in `LibreHardwareMonitor.exe.config`'s `<appSettings>`-equivalent section before launch. Without `runWebServerMenuItem=true`, LHM starts cleanly but listens on ZERO ports — the swarm WILL see connection-refused and conclude "Full mode unavailable" incorrectly. This is the #1 gotcha for Story 6.4.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** LHM binary path resolution. Child-handle ownership tracking (sidebar-owned vs user-owned). Job Object setup. Config keys are known and covered by patch tests; app-level child-monitor wiring is present in the current 12.8 worktree slice, with real UAC/LHM validation still pending.
  2. [ ] **Implement:** `crates/sidebar-platform/src/ohm_supervisor.rs`. **All `unsafe` per F11 with SAFETY comments.** No COM init needed (was required for WMI).
  3. [ ] **Validate:** Integration against bundled LHM v0.9.6 on this dev machine (LAPTOP-PLN56DNU, Ryzen AI 7 350 — LHM v0.9.6 has Ryzen AI 300-series support).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — `trait HttpClient` mock:**
    1. Mock HTTP probe returns LHM-shaped JSON → `probe()` returns `Tier::Full`.
    2. Mock HTTP probe returns connection-refused → `Tier::Basic`, no UAC.
  - **Boundary & Edge Case Test Cases (cite T-10, T-11, T-45, G10):**
    1. User clicks "Enable Full mode" → `launch_elevated()` writes port to LHM config + invokes `ShellExecuteW("runas")` (mock; real UAC manual).
    2. `ShellExecuteW` returns HINSTANCE error code ≤32 (e.g. ERROR_ACCESS_DENIED) → decoded, logged, Basic retained.
    3. LHM child launched by the sidebar exits mid-session → app monitor degrades once, broadcasts `Tier::Basic`, and the pill flips; an externally running LHM is not treated as sidebar-owned.
    4. Shutdown: sidebar-launched LHM → terminated; user-launched LHM → left running (G10).
    5. LHM binary missing → `launch_elevated()` Err with clear message.
    6. UAC declined → `ShellExecuteW` error; Basic retained, no retry loop.
    7. Host crash simulation (kill -9 the test supervisor) → Job Object reaps LHM child within ~1s (G10).
    8. Launch timeout T-11 (5s) exceeded without HTTP probe succeeding → "LHM launch failed", Basic retained.
    9. **Port fallback (T-45):** port 17127 occupied by a different service → write 17128 to LHM config, launch, probe succeeds on 17128.
    10. **Non-LHM discrimination:** something else returns HTTP 200 on 17127 but body isn't LHM JSON → treated as occupied → port fallback.
    11. **HTTP-server-not-enabled regression:** LHM config written but `runWebServerMenuItem` key missing or set to `false` → LHM launches, listens on zero ports → probe times out → Basic mode. Test asserts the config-write includes BOTH `runWebServerMenuItem=true` AND `listenerPort=<port>` (the #1 gotcha — see Verified-fact note in Technical Context).
    12. **Tier-change broadcast (T-38, F12):** LHM crash triggers `Event::TierChanged(Basic)` on the Event channel within 500ms; GUI status pill flips; coalescing prevents pill-flap if LHM restabilizes within 500ms.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11) — UAC + process-ownership + Job Object logic + port-write-to-LHM-config + tier-change-broadcast contract. HITL on `ShellExecuteW` invocation. Shell gate. G23 (Event channel discipline).

---

## EPIC 7 — Application Wiring
- **System Objective:** Wire the binary: tokio runtime, provider registry, poller, broadcast, two-tier probe.
- **Swarm Mapping:** Runtime-Wiring Agent.

### STORY 7.1: Provider Registry
- **User Story:** As the Wiring Agent, I want a registry building `Vec<Arc<dyn SensorProvider>>` filtered by `classify_for_v1(active_tier)` (AD-5, §5.4).
- **Technical Context:** §4 (`provider_registry.rs`) + §5.4. Hot tier switch rebuilds registry.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Hot rebuild on `Event::TierChanged`. Idempotency F6.
  2. [ ] **Implement:** `crates/sidebar-app/src/provider_registry.rs`.
  3. [ ] **Validate:** All cost-class × tier combos (F4).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F4:**
    1. `[sysinfo(LW/Basic), ohm(LW/Full), net(LW/Both)]` + active=Basic → sysinfo + net, drops ohm.
    2. Same + active=Full → all three.
  - **Boundary & Edge Case Test Cases:**
    1. `Heavy` descriptor → rejected with `warn!`.
    2. Hot tier switch Basic→Full mid-session → registry rebuilt; ohm added.
    3. Empty registry → empty vec, no panic.
    4. Idempotency (F6): rebuild twice produces identical registry.
- **Explicit Swarm Guardrails:** None.

### STORY 7.2: Poller Task (interval + broadcast publish)
- **User Story:** As the Wiring Agent, I want the poller task (AD-6, §6 flow A/B/C) — fires every `poll_interval_seconds`, runs providers on the blocking pool with a shared deadline, concatenates, timestamps, and publishes via broadcast.
- **Technical Context:** AD-6 + §6 + T-2/T-3/T-14/T-18 + G15. `tokio::time::interval` uses `MissedTickBehavior::Delay` to skip overlapping ticks. Each `provider.read_all()` runs in `spawn_blocking`, is wrapped in `catch_unwind(AssertUnwindSafe(...))`, and shares a 100 ms deadline; timed-out or panicking providers are skipped while survivors publish. Broadcast capacity 8 (T-14). 2 worker threads (T-18).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Broadcast capacity 8 (T-14). `catch_unwind` bounds (DECIDE: wrap each call in `AssertUnwindSafe` since `SensorProvider: Send + Sync` and we accept the unwind-safety caveat for poller resilience). Provider panic → log + skip (G15).
  2. [ ] **Implement:** `crates/sidebar-app/src/poller.rs`.
  3. [ ] **Validate:** F2 (mock broadcast), F10 (panic-catch).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixtures F2, F4, F10:**
    1. Two mock providers × 2 readings each → vec of 4 with single timestamp.
    2. Interval = 100ms (test); 3 ticks in 350ms → 3 messages.
  - **Boundary & Edge Case Test Cases (cite T-2, T-3, T-14, T-18, G15):**
    1. One provider panics (F10) → others' readings still published, panic logged (G15).
    2. One provider slow (500ms) with interval 100ms → shared deadline skips the provider; `MissedTickBehavior::Delay` prevents overlapping fan-outs.
    3. Receiver lags → oldest dropped (T-14), `warn!`.
    4. Interval = 0 → clamped to 1s (T-3).
    5. Aggregate CPU% over 5-min simulated window across all providers ≤ T-2 (2%).
- **Explicit Swarm Guardrails:** HITL on `catch_unwind`/`AssertUnwindSafe` decision (G11).

### STORY 7.3: Two-Tier Auto-Detect Probe (on every launch)
- **User Story:** As the Wiring Agent, I want the launch-time probe (AD-7, PRD §5.2).
- **Technical Context:** PRD §5.2 + AD-7 (revised) + T-10 + T-45. Delegates to `OhmSupervisor::probe()` (Story 6.4) which runs `GET http://127.0.0.1:<port>/data.json` via `ureq`. Sets `AppState.tier`. The port comes from config `[ohm] http_port = 17127` (Story 1.5), resolved through the T-45 fallback chain.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Sequence + AppState update. Tier-change broadcast (T-38) on transition.
  2. [ ] **Implement:** In `main.rs` launch sequence.
  3. [ ] **Validate:** Mock supervisor integration.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Mock probe Full → `AppState.tier = Full`, pill FULL.
    2. Mock probe Basic → Basic, no UAC.
  - **Boundary & Edge Case Test Cases (cite T-10, T-45):**
    1. Probe times out (500ms T-10) → Basic.
    2. Host elevated but LHM not installed → Basic + "install LHM" hint.
    3. Rapid relaunch (LHM running from previous session on port 17127) → probe succeeds immediately.
    4. LHM running but on fallback port 17128 (port 17127 was occupied at last launch) → probe tries 17127 (fails), then 17128 (succeeds) within 1s total.
- **Explicit Swarm Guardrails:** HITL — must verify "no UAC on default first launch" (G11, success metric).

---

## EPIC 8 — GUI (egui)
- **System Objective:** Render sidebar UI: status pill, metric rows (NFR-8), bandwidth panel, settings.
- **Swarm Mapping:** Frontend-UI Agent.

### STORY 8.1: AppState + egui::App + Repaint on Broadcast
- **User Story:** As the UI Agent, I want `AppState` wired to `eframe::App` repaint on broadcast (§6, T-9).
- **Technical Context:** §6 + T-9. egui 0.35 `App::ui` (OQ-3). Repaint via vsync + `ctx.request_repaint()` on broadcast drain.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `egui_kittest` for headless; manual for transparency/AppBar.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui.rs` + `main.rs`.
  3. [ ] **Validate:** F8 snapshot + manual.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. egui_kittest: AppState with one CPU reading → snapshot contains "42%" + "CPU".
    2. Broadcast receive triggers `request_repaint` (mock ctx).
  - **Boundary & Edge Case Test Cases (cite T-9, T-20, T-21, G15):**
    1. Empty readings → "Waiting for data..." placeholder, no panic.
    2. RwLock poisoned → GUI reads last good snapshot, logs (G15).
    3. 1000 readings → render within T-9 (16ms); document truncation if exceeded.
- **Explicit Swarm Guardrails:** HITL smoke on Win11.

### STORY 8.2: Status Pill
- **User Story:** As the UI Agent, I want the status pill (PRD §5.3).
- **Technical Context:** PRD §5.3. Pill BASIC (gray) / FULL (green). Tooltip per spec. Click → `OhmSupervisor::launch_elevated()`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** egui tooltip API in 0.35.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/status_pill.rs`.
  3. [ ] **Validate:** F8 + manual.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. `tier=Basic` → pill renders "BASIC" gray.
    2. Click pill in Basic → invokes launch-elevated callback (mock).
  - **Boundary & Edge Case Test Cases:**
    1. `tier=Full` → "FULL" green, click no-op or info.
    2. Tooltip text matches PRD §5.3 verbatim (snapshot).
- **Explicit Swarm Guardrails:** HITL — UAC trigger must be explicit user action only.

### STORY 8.3: Metric Row (NFR-8)
- **User Story:** As the UI Agent, I want a metric row component formatting each reading via `format` (NFR-8).
- **Technical Context:** §4 (`metric_row.rs`) + AD-13 + T-28/T-29/T-30. Maps `MetricKind × Unit` → `format_*`. Respects `config.display.{raw_values, temp_unit, decimal_base}`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Dispatch table.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/metric_row.rs`.
  3. [ ] **Validate:** F8 per MetricKind.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. CpuFrequency 3.84e9 → "3.84 GHz".
    2. `raw_values=true` → "3840000000 Hz".
  - **Boundary & Edge Case Test Cases (cite T-20, T-28, T-29):**
    1. CpuTemperature with `temp_unit=Fahrenheit` → "144 °F" (T-29).
    2. NaN reading → "--" (T-20).
    3. Unknown MetricKind → "unknown", logged.
- **Explicit Swarm Guardrails:** None.

### STORY 8.4: Bandwidth Panel — v2 MARQUEE
- **User Story:** As the UI Agent, I want the bandwidth panel (PRD §3 Tier 4, §5.5.8).
- **Technical Context:** §4 (`bandwidth_panel.rs`) + §6 flow H + T-28. Reads `Arc<RwLock<BandwidthView>>`. Per-NIC rows: friendly name, RX/TX/total GB, "X days until reset (YYYY-MM-DD)". History table below.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** egui grid in 0.35. Friendly name from LUID.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/bandwidth_panel.rs`.
  3. [ ] **Validate:** F8 with synthetic BandwidthView.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. BandwidthView 1 NIC (rx=50GB, tx=20GB, total=70GB, days_until_reset=12) → renders all four + "12 days until reset".
    2. History 1 prior cycle → renders below, smaller font.
  - **Boundary & Edge Case Test Cases:**
    1. Empty BandwidthView → "No network adapters tracked".
    2. `days_until_reset=0` → "Resets today" (document exact string).
    3. NIC in history not current → "(disconnected)" annotation.
- **Explicit Swarm Guardrails:** HITL — marquee feature, visual review (G11).
- **Integration note (2026-07-12):** the accountant now publishes `BandwidthView` snapshots over a watch channel, including retained SQLite history; the GUI bridge is tested, while visual Win11 review remains HITL.

### STORY 8.5: Settings Panel
- **User Story:** As the UI Agent, I want settings to edit `cycle_start_day`, temp unit, raw toggle, decimal/binary, poll interval, docked edge, theme.
- **Technical Context:** §4 (`settings_panel.rs`) + T-3/T-21/T-26/T-28/T-29. Edits write `Config`, persist to `config.toml` debounced.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `cycle_start_day` day picker (1–28 per T-26) + "Last day of month" radio.
  2. [ ] **Implement:** `settings_panel.rs`.
  3. [ ] **Validate:** F8 + config round-trip.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Change `cycle_start_day` 1→15 → Config updated, persists.
    2. Toggle `raw_values` on → metric rows re-render raw.
  - **Boundary & Edge Case Test Cases (cite T-3, T-21, T-26):**
    1. `cycle_start_day` change does NOT retroactively re-split current cycle (PRD §5.5.8 — next rollover only).
    2. `poll_interval=0` → clamped to 1 (T-3) with visible warning.
    3. `cycle_start_day=29` rejected at UI (T-26); user must pick ≤28 or "Last day".
    4. Settings closed without save → autosave vs revert (DECIDE: autosave debounced, no revert).
- **Explicit Swarm Guardrails:** HITL on no-retroactive-resplit rule (G11).

---

## EPIC 9 — Build & Release Pipeline (Zero-Cost)
- **System Objective:** SignPath + GitHub Releases + winget + optional Store (AD-14, §11).
- **Swarm Mapping:** Release-Engineering Agent.

### STORY 9.1: SignPath Project Setup + Code Signing Policy
- **User Story:** As the Release Agent, I want SignPath Foundation set up + `code-signing-policy.md` so sidebar.exe can be signed for free (AD-14, §11.2).
- **Technical Context:** §11.2 + PRD OQ-1 + R12. SignPath eligibility: OSI license (Story 0.5), public repo, free downloads, MFA approvers, `code-signing-policy.md` linked from README. **LHM acquisition strategy:** download `LibreHardwareMonitor.exe` from the pinned LHM release at build time (NOT committed to repo — too large + license-resale ambiguity), SHA-256 verify against `resources/ohm.sha256`, and package the local HTTP sidecar.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm LICENSE (Story 0.5). Draft policy. OHM acquisition URL + hash pinning.
  2. [ ] **Implement:** `signpath/code-signing-policy.md` + README link + `resources/ohm.sha256` + CI download step (gated behind SignPath-egress allowlist G16).
  3. [ ] **Validate:** Submit SignPath application (out-of-band).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `code-signing-policy.md` lints clean (markdown lint).
    2. Policy references OSI LICENSE.
    3. `resources/ohm.sha256` format valid (64-hex + two-spaces + filename).
  - **Boundary & Edge Case Test Cases:**
    1. SignPath rejects (R12) → fallback documented: unsigned via GitHub Releases + winget.
    2. Policy missing required section (approver MFA) → pre-commit hook flags.
    3. OHM download hash mismatch → CI fails fast, no packaging.
    4. OHM download URL 404 (release retired) → CI fails with actionable message.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — external trust submission. Egress to `github.com/ArcadeRenegade/SidebarDiagnostics`... wait — OHM is at `github.com/ArcadeRenegade/SidebarDiagnostics`? No — OHM is `github.com/ArcadeRenegade/SidebarDiagnostics` is the upstream we're cloning; LibreHardwareMonitor is at `github.com/LibreHardwareMonitor/LibreHardwareMonitor`. The CI egress allowlist MUST include `github.com/LibreHardwareMonitor`, `objects.githubusercontent.com` (G16).

### STORY 9.2: `release.yml` Workflow (Build → Sign → Publish)
- **User Story:** As the Release Agent, I want the release workflow (§11.1 Stages 1–4): build → SignPath sign → package ZIP + MSIX → GitHub Release + winget PR.
- **Technical Context:** §11.1 + G18 + T-31. Triggered on `v*` tag or manual dispatch. Stages per §11.1. **SignPath env vars:** `SIGNPATH_API_TOKEN` (secret), `SIGNPATH_PROJECT_SLUG=sidebar`, `SIGNPATH_SIGNING_POLICY_SLUG=release`. Gated on `release-approver` GitHub Environment (required reviewers). Reproducible build assertion: hash the binary twice across two runner invocations; warn (not fail) if differs (G18 best-effort).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** SignPath GitHub Action version (`signpath/github-action@v1`). winget-create manifest template.
  2. [ ] **Implement:** `.github/workflows/release.yml` + `winget/manifest.yaml` + `signpath/` policy link.
  3. [ ] **Validate:** Dry-run on `v0.0.0-test` tag.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `actionlint` passes on `release.yml`.
    2. Dry-run produces ZIP with `signed-sidebar.exe` (or unsigned fallback) + `OHM.exe` + `config.toml.example`.
  - **Boundary & Edge Case Test Cases:**
    1. SignPath fails (approval denied) → workflow continues unsigned (fallback §11.2).
    2. winget PR fails (rate limit) → workflow does NOT fail release; logs warning.
    3. Tag push without `release-approver` → blocks at env gate.
    4. Binary hash differs across two runs → warn (G18 best-effort reproducibility).
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — release-approver env. No auto-publish on tag.

---

## EPIC 10 — Acceptance & Verify
- **System Objective:** NFR verification harness: perf bench, smoke checklist, coverage gate, network-egress assertion.
- **Swarm Mapping:** QA Agent.

### STORY 10.1: `poll_cost` Criterion Bench + NFR-3/NFR-4 Executable Tests + Network Egress Assertion
- **User Story:** As the QA Agent, I want the criterion bench enforcing T-1/T-2, plus executable tests for cold-start (T-7) and RSS (T-4/T-5/T-6), plus a runtime network-egress assertion (G16), so NFRs are verified in CI not just manually.
- **Technical Context:** §7.3 + T-1/T-2/T-4/T-5/T-6/T-7 + T-31 + T-43 + G16. `benches/poll_cost.rs` (F9) per adapter + aggregate. **Reference hardware (T-31) is generalized** — the bench reports a calibration constant (idle baseline CPU% over 60s) and T-1/T-2 thresholds are evaluated as deltas from that baseline, so the bench is meaningful on any dev machine or CI runner. The current `--bench-cold-start` executable probe intentionally measures process-side egui setup without production config, sensor discovery, graphics, or LHM composition. It provides a deterministic host-probe smoke; full production cold-start/RSS/egress evidence remains a Windows CI/manual smoke gate. **Cold-start probe** (T-7): writes elapsed host-probe timing to a temp file. **RSS probe** (T-4/T-5): samples `GetProcessMemoryInfo` during the bounded host probe. **Egress probe** (G16): snapshots `netstat -ano` at startup and after 60s for the probe PID; short-lived sockets and production GUI/LHM behavior require the smoke gate. Coverage via `cargo-llvm-cov` (T-43).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Reference hardware T-31; CI normalization multiplier. Cold-start instrumentation hook (`--bench-cold-start` flag in `sidebar-app`). netstat parsing on Windows.
  2. [ ] **Implement:** `benches/poll_cost.rs` + `benches/parse_threshold.rs` + `tests/nfr_cold_start.rs` + `tests/nfr_rss.rs` + `tests/runtime_no_egress.rs`. **All `unsafe` (`GetProcessMemoryInfo`, `netstat` shell-out is safe via `Command`) per F11.**
  3. [ ] **Validate:** Run the host-probe checks on Windows CI; full production cold-start/RSS/egress evidence remains a separate manual/HITL smoke gate.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Bench all-Lightweight → green, prints report.
    2. `parse_threshold` identifies 0.6% provider as failure (T-1).
    3. Host-probe cold-start test on `--bench-cold-start` reports <2000ms (T-7); production startup remains manual evidence.
    4. Host-probe RSS test on a 30-second shortened run (CI budget) reports <80 MiB (T-4); production RSS remains manual evidence.
    5. Host-probe egress test checks startup and 60s snapshots for zero outbound sockets (G16); production egress remains manual evidence.
  - **Boundary & Edge Case Test Cases (cite T-1, T-2, T-4, T-5, T-6, T-7, T-31, G16):**
    1. proc-gpu (Watch) breaches T-1 → bench fails, feature auto-disabled (OQ-2).
    2. Aggregate CPU > T-2 (2%) → bench fails with aggregate report.
    3. Cold-start > T-7 (2000ms) → test fails with measured ms.
    4. RSS > T-4/T-5 → test fails with measured MiB.
    5. SQLite incremental RSS > T-6 (3 MiB) → test fails.
    6. CI runner noisier than reference T-31 → flaky; document tolerance band (e.g. ±20%).
    7. Egress test: if sidebar.exe opens ANY socket (regression) → test fails naming the destination IP.
- **Explicit Swarm Guardrails:** HITL — T-1 threshold + T-31 reference-hardware policy (G11). HITL on any new `unsafe`.

### STORY 10.2: Manual Smoke Checklist Automation (where feasible)
- **User Story:** As the QA Agent, I want the §7.4 manual smoke checklist codified so verify runs are reproducible.
- **Technical Context:** §7.4 (18 items incl. v2 additions).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Separate automatable vs manual.
  2. [ ] **Implement:** `verify/smoke-checklist.md` + scriptable harness.
  3. [ ] **Validate:** Dry-run on dev machine.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Checklist parses, all 18 items present.
    2. Scriptable items (config round-trip after reboot simulation) pass.
  - **Boundary & Edge Case Test Cases:**
    1. Manual item marked failed → verify run fails with item highlighted.
- **Explicit Swarm Guardrails:** HITL — manual smoke cannot be automated away (G11).

---

## STORIES ADDED IN AUDIT PASS 3 (correctness & completeness vs PRD/architecture)

The stories below close the gaps found in audit pass 3. Each maps to a specific PRD §3 UX row, §5 two-tier detail, §6 NFR, or §8 risk that had no owning story in passes 1–2.

---

### STORY 6.5: LHM Binary Acquisition + Version Pinning (R7)
- **Epic:** 6 (Platform)
- **User Story:** As the Win32 Agent, I want a deterministic, hash-verified, version-pinned acquisition of the bundled `LibreHardwareMonitor.exe` so the HTTP `/data.json` contract (R7) is stable and the binary is not committed to the repo.
- **Technical Context:** PRD §8 R7 + AD-2 + G18 + nfr-thresholds.md T-32. **Acquisition strategy:** `scripts/fetch_ohm.ps1` downloads the pinned LHM v0.9.6 release from `github.com/LibreHardwareMonitor/LibreHardwareMonitor`, verifies `resources/ohm.sha256`, and places `LibreHardwareMonitor.exe` plus supporting files under `resources/`. The script and local hash/license are provisioned; release CI packaging remains pending. LHM serves HTTP `/data.json`; no WMI namespace is required.
- **Gentle-AI SDD Phase Checklist:**
  1. [x] **Plan:** Pin LHM v0.9.6 and document the local HTTP `/data.json` endpoint plus config keys (`runWebServerMenuItem`, `listenerPort`).
  2. [x] **Implement:** `scripts/fetch_ohm.ps1` + `resources/ohm.sha256` + ignored `resources/LibreHardwareMonitor.exe`. The script validates the committed pin before replacing resources and supports offline `-CheckOnly`; CI `release.yml` stage 0 remains pending.
  3. [ ] **Validate:** Run the fetch on Windows CI; verify hash. `fetch_ohm.ps1 -CheckOnly` implements deterministic matching, malformed-pin, and corrupted-binary failure paths, but `crates/sidebar-app/tests/dev_env_scripts.rs` currently covers script presence/syntax and happy-path/idempotency only; add dedicated negative-path invocation coverage before marking validation complete. Full network fetch remains pending external egress and HITL approval.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `resources/ohm.sha256` format valid: 64-hex + two-spaces + filename (checked by `-CheckOnly`).
    2. Fetch script, when LHM v0.9.6 is available, downloads successfully; staged hash matches before extraction.
  - **Boundary & Edge Case Test Cases (cite R7, G18, T-32):**
    1. Staged hash mismatch → script exits non-zero before replacing the destination; offline `-CheckOnly` proves the negative path deterministically.
    2. Upstream release retired (404) → script exits non-zero with actionable message naming the missing tag.
    3. Network egress blocked (CI sandbox) → script times out cleanly within 30s (not infinite).
    4. License file alongside LHM (MPL-2.0) — fetched and placed at `resources/LibreHardwareMonitor.LICENSE.txt`; verify it exists post-fetch.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — OHM version choice is an upstream-trust decision. Egress to `github.com/LibreHardwareMonitor` + `objects.githubusercontent.com` required in G16 allowlist.

---

### STORY 6.6: Global Hotkey System + Multi-Monitor Picker + Theme Bridge
- **Epic:** 6 (Platform)
- **User Story:** As the Win32 Agent, I want a hotkey system (`Ctrl+Shift+S` toggle click-through per T-34), a multi-monitor picker (`EnumDisplayDevices` per T-36), and a system-theme bridge (reg-query for `AppsUseLightTheme` per T-35) so the UX features in PRD §3 land correctly.
- **Technical Context:** NFR-7 + T-34/T-35/T-36 + PRD §3 UX rows.
  - **Hotkey:** `RegisterHotKey` per HWND OR `global-hotkey` crate (MIT/Apache-2.0, T-32-allowed). Parsed from `[hotkeys] click_through = "Ctrl+Shift+S"` string. Conflict handling per T-34: log `warn!`, no silent fallback.
  - **Monitor picker:** `EnumDisplayDevices` + `EnumDisplaySettingsEx` enumerate monitors; expose `MonitorInfo { id: DeviceID, friendly_name, x, y, width, height, dpi }`. `[dock] monitor_id = <DeviceID>` (or `"primary"` sentinel). Re-dock on monitor disconnect per T-36.
  - **Theme bridge:** `RegQueryValueEx(HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize, AppsUseLightTheme)` for system theme. Listen for `WM_SETTINGCHANGE` with `lParam = "ImmersiveColorSet"` to detect live theme change → broadcast `Event::ThemeChanged("dark"|"light")` on the Event channel (F12). Missing or malformed registry values default to Dark. The current domain contract intentionally carries the resolved mode as a string; `"system"` is reserved for configuration, not runtime notifications.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Decide `global-hotkey` crate vs raw `RegisterHotKey` (recommend `global-hotkey` — cross-platform abstraction, less unsafe). Confirm `WM_SETTINGCHANGE` listening via eframe's `RawEvent` hook OR a custom `wndproc`.
  2. [ ] **Implement:** `crates/sidebar-platform/src/hotkey.rs` + `monitors.rs` + `theme_bridge.rs`. **All `unsafe` per F11.** Hotkey events broadcast as `Event::HotkeyPressed(name)`.
  3. [ ] **Validate:** Unit tests mockable; integration tests on Windows CI.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixtures F11, F12:**
    1. Parse `"Ctrl+Shift+S"` → `HotkeyCombo { ctrl: true, shift: true, key: S }`.
    2. `monitors::enumerate()` returns ≥1 monitor on Windows CI.
    3. `theme_bridge::is_system_dark()` returns bool (mock the reg query; missing value defaults Dark).
  - **Boundary & Edge Case Test Cases (cite T-34/T-35/T-36, G15):**
    1. Hotkey already registered by another app → `register()` returns Err; `warn!`; toggle unavailable until conflict resolves (T-34).
    2. Hotkey parse failure (`"Foo+Bar"`) → returns Err, config validation logs and treats as disabled.
    3. Configured `[dock] monitor_id` not present (unplugged) → re-dock to primary + `warn!` (T-36).
    4. System theme registry key missing → fallback to `Dark` default.
    5. `WM_SETTINGCHANGE` broadcast → `Event::ThemeChanged("dark"|"light")` published within 100ms.
- **Explicit Swarm Guardrails:** HITL on `RegisterHotKey` invocation (G19). HITL on capture cloak behavior — needs streamer review. HITL on any `unsafe`.

---

### STORY 7.4: Event Channel + Tier-Change Coalescing
- **Epic:** 7 (Wiring)
- **User Story:** As the Wiring Agent, I want the `Event` broadcast channel (separate from the readings broadcast) with tier-change coalescing so UI-affecting notifications don't mix with sensor data and OHM flap doesn't thrash the status pill (architecture §6, G23, T-38).
- **Technical Context:** architecture §6 + G23 + T-38 + F12. Two channels:
  1. `readings_tx: broadcast::Sender<Vec<Reading>>` (capacity 8, T-14).
  2. `event_tx: broadcast::Sender<Event>` (capacity 8, T-14).
  `Event` enum: `TierChanged(Tier)`, `ThemeChanged(String)`, `MonitorChanged(String)`, `HotkeyPressed(String)`, `Shutdown`. `ThemeChanged` carries the resolved lowercase mode (`"dark"` or `"light"`). **Coalescing (T-38):** tier-change events emitted by `OhmSupervisor` pass through a 500ms coalescer task; only the latest within the window is published. Other event types are not coalesced.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Event enum location (`sidebar-domain::event`). Coalescer implementation (`tokio::select!` with a debounce timer).
  2. [ ] **Implement:** `crates/sidebar-app/src/event_channel.rs` + coalescer task.
  3. [ ] **Validate:** Fixture F12.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F12:**
    1. Send `Event::TierChanged(Full)` then `Event::TierChanged(Basic)` within 500ms → subscribers receive only the latter (T-38 coalescing).
    2. Send `Event::ThemeChanged("dark")` → subscribers receive it (no coalescing for theme).
  - **Boundary & Edge Case Test Cases (cite T-14, T-38, G15):**
    1. 100 tier-change events in 1s → at most 2 published (start + end of window).
    2. Channel overflow (T-14 cap 8) → oldest dropped + `warn!`.
    3. Coalescer task panics → caught, logged, fallback: pass-through without coalescing (G15).
    4. `Event::Shutdown` published → all subscribers drain within their T-39 phase.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — Event enum + channel contract is an architectural keystone; modifications ripple.

---

### STORY 7.5: Graceful Shutdown Orchestrator
- **Epic:** 7 (Wiring)
- **User Story:** As the Wiring Agent, I want a shutdown orchestrator that handles `Ctrl+C` / `SIGTERM` / `WM_CLOSE` per the T-39 timeout hierarchy so the app exits cleanly with bandwidth data force-flushed and sidebar-owned OHM reaped.
- **Technical Context:** T-19/T-39 + G14/G15 + F13. Trigger sources: `tokio::signal::ctrl_c()`, `WM_CLOSE` from window (eframe close button), `Event::Shutdown` from any component. Sequence per T-39: cancel token → accountant force-flush (≤500ms) → OhmSupervisor teardown (≤2000ms) → runtime drop (≤3000ms) → forced exit. Uses `tokio_util::sync::CancellationToken`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** CancellationToken wiring. Force-exit via `std::process::exit(0)` if any phase exceeds its budget.
  2. [ ] **Implement:** `crates/sidebar-app/src/shutdown.rs`.
  3. [ ] **Validate:** Fixture F13.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F13:**
    1. Trigger Ctrl+C → accountant force-flush completes within 500ms (T-39 phase 2).
    2. Full shutdown completes within 3000ms (T-19).
  - **Boundary & Edge Case Test Cases (cite T-19, T-39, G15):**
    1. Accountant hangs (simulated) → phase 2 budget exceeded → forced transition to phase 3, log `error!`.
    2. OhmSupervisor hangs → phase 3 budget exceeded → Job Object (G10) reaps OHM via kernel, forced exit.
    3. Force-flush fails (SQLite disk full) → logged, shutdown continues (data loss accepted per R11).
    4. Double shutdown signal (Ctrl+C twice) → second is no-op; first is already in progress.
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — process-termination policy.

---

### STORY 8.6: Theme + Accent Color UI
- **Epic:** 8 (GUI)
- **User Story:** As the UI Agent, I want theme (Dark/Light/System) and accent-color support so the sidebar matches user preference (PRD §3 UX row, T-35).
- **Technical Context:** T-35 + F12. egui visuals mutated at startup + on `Event::ThemeChanged`. Accent injected via `ctx.style().visuals.selection.bg_fill`. Hex parsing via a small `accent` parser; invalid → fallback `#4CAF50`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** egui 0.35 visuals API. Hex parser edge cases (`#RGB`, `#RRGGBB`, `#RRGGBBAA`).
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/theme.rs`. Subscribe to Event channel for `ThemeChanged`.
  3. [ ] **Validate:** F8 snapshot in each theme.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. Theme=Dark → snapshot matches `tests/snapshots/theme_dark.snap`.
    2. Theme=Light → snapshot matches `tests/snapshots/theme_light.snap`.
    3. Accent `#FF0000` → selection bg_fill is red (assert via ctx readback).
  - **Boundary & Edge Case Test Cases (cite T-35):**
    1. Accent `"garbage"` → fallback `#4CAF50` + `warn!`.
    2. Accent `"#RGB"` (short form) → expanded to `#RRGGBB`.
    3. System theme changes (event received) → visuals update without restart.
- **Explicit Swarm Guardrails:** HITL on snapshot acceptance (`cargo insta accept`).

---

### STORY 8.7: Sparkline Widget
- **Epic:** 8 (GUI)
- **User Story:** As the UI Agent, I want a sparkline widget rendering the rolling window (Story 1.6) so users see history mini-graphs (PRD §3 UX row).
- **Technical Context:** Story 1.6 data + T-22 + F8. egui custom painter; reads `RollingWindow` from AppState; renders as a small inline line chart. NaN values render as gaps.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** egui `Painter::line_segment` / `Path::line`. Width param.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/sparkline.rs`.
  3. [ ] **Validate:** F8.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. RollingWindow `[10.0, 20.0, 30.0]` → sparkline renders 3 segments ascending.
    2. Empty window → renders placeholder "—".
  - **Boundary & Edge Case Test Cases (cite T-22, T-20):**
    1. NaN in window → gap in line (documented).
    2. Window larger than widget width → downsample (or document overflow behavior).
    3. All values identical → flat line at vertical center.
- **Explicit Swarm Guardrails:** None.

---

### STORY 8.8: Threshold Alert UI
- **Epic:** 8 (GUI)
- **User Story:** As the UI Agent, I want threshold alerts (Story 1.2 logic) surfaced visually so users see when CPU/GPU temps breach `[thresholds]` (PRD §3 UX row).
- **Technical Context:** Story 1.2 + T-35. Alert state from `check_threshold` drives row color: Normal=default, Warning=accent, Critical=red (`#F44336`). Blinking optional (off by default — calm UX).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Color mapping. Hysteresis already in 1.2.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/alert_indicator.rs`.
  3. [ ] **Validate:** F8.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. Reading 95°C, threshold critical 90°C → row color red.
    2. Reading 60°C, threshold warn 80°C → row color default.
  - **Boundary & Edge Case Test Cases:**
    1. Hysteresis: oscillation 88→92→88 with threshold 80/95, hysteresis 5 → color doesn't flap (1.2 contract).
    2. Threshold unset (None) → no alerting, default color.
    3. NaN reading → no alert, default color (T-20).
- **Explicit Swarm Guardrails:** None.

---

### STORY 8.9: Metric Enable/Disable + Drag-Reorder UI
- **Epic:** 8 (GUI)
- **User Story:** As the UI Agent, I want per-metric enable/disable + drag-reorder so users customize the sidebar layout (PRD §3 UX row, T-37 `[metrics]` config).
- **Technical Context:** `[metrics] enabled` + `[metrics] order` (Story 1.5). egui drag-and-drop via `egui_dnd` crate (MIT/Apache-2.0, T-32-allowed) OR native egui `Response::dnd_drop`. Persistence: every reorder writes back to config (debounced).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** egui_dnd vs native. Drag handle UX.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/metric_list.rs`.
  3. [ ] **Validate:** F8 + config round-trip.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Disable `CpuPower` → row disappears; config `[metrics] enabled` excludes it.
    2. Drag `CpuFrequency` above `CpuUtilization` → config `[metrics] order` updates.
  - **Boundary & Edge Case Test Cases:**
    1. Disable ALL metrics → sidebar shows "No metrics enabled" placeholder.
    2. Reorder persisted across restart (config round-trip via Story 1.5).
    3. Metric in `[metrics] order` but not in `enabled` → ignored (no crash).
- **Explicit Swarm Guardrails:** None.

---

### STORY 8.10: First-Run Wizard
- **Epic:** 8 (GUI)
- **User Story:** As the UI Agent, I want a first-run wizard collecting docked edge, target monitor, billing-cycle start day, and theme, so users get a sensible default-config experience on first launch (T-37, G24).
- **Technical Context:** T-37 + G24. Detect first-run via absence of `%APPDATA%\sidebar\config.toml` OR `first_run_complete = false`. Modal egui panel that blocks the rest of startup until dismissed. Writes `config.toml` on completion OR skip with `first_run_complete = true`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Modal egui panel that doesn't block GUI thread (G24). Wizard step sequence.
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/first_run.rs`.
  3. [ ] **Validate:** F8 + first-run-detection test.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixtures F1, F8:**
    1. Absent config file → wizard renders.
    2. Complete wizard → `config.toml` written with selected values + `first_run_complete = true`.
    3. Skip wizard → defaults written + `first_run_complete = true`.
  - **Boundary & Edge Case Test Cases (cite T-37, G24):**
    1. Existing config with `first_run_complete = true` → wizard does NOT render.
    2. Wizard completed but `config.toml` write fails (read-only FS) → wizard surfaces error, allows retry.
    3. Wizard closed via window-X → treated as skip (defaults applied).
    4. Poller does NOT start while wizard is showing (G24 — wizard is the gate).
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — first impression; visual review.

---

### STORY 9.3: Auto-Update Check (winget-aware, optional)
- **Epic:** 9 (Release)
- **User Story:** As the Release Agent, I want an OPTIONAL in-app auto-update check that respects the G16 no-runtime-egress rule by being OFF by default and limited to a GitHub-Releases-URL fetch when explicitly enabled, so users can opt into update notifications without violating the privacy posture.
- **Technical Context:** PRD OQ-1 (zero-cost distribution) + G16. **Default OFF.** When user enables `[updates] check_on_startup = true`, sidebar makes ONE outbound HTTPS GET to `api.github.com/repos/<owner>/sidebar/releases/latest` on startup, compares tag to local version, and surfaces a non-modal toast if newer. **Hard constraint:** only that one URL; no telemetry, no body, no auth token. Egress allowlist MUST be extended per G19.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Decide if this story ships in v1 or v1.1. Recommend v1.1 (deferral) unless user wants it now. If shipped, G16 egress allowlist must add `api.github.com`.
  2. [ ] **Implement:** `crates/sidebar-app/src/updater.rs` (off by default). `[updates] check_on_startup = false` in config.
  3. [ ] **Validate:** Egress test (Story 10.1) MUST fail when `check_on_startup = true` AND no update URL is hit (regression detection).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Default config → updater NOT invoked (T-39/G16 — zero egress).
    2. Mock HTTP returns newer tag → toast surfaced.
  - **Boundary & Edge Case Test Cases (cite G16):**
    1. `check_on_startup = true` + network failure → silent, logged.
    2. Mock HTTP returns 404 → silent, logged.
    3. Egress test asserts sidebar.exe opens ZERO sockets when `check_on_startup = false` (the default).
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — runtime network egress is a privacy policy decision. Recommend v1.1 deferral.

---

## Sequencing Summary (audit pass 3)

```
Epic 0 (Foundation) — 6 stories, all blocking.
   │
   ▼
Epic 1 (Domain) — pure types & logic.
   │
   ▼
Epic 2 (Sensor framework) — keystone.
   │
   ├──────────────┐
   ▼              ▼
Epic 3         Epic 4
(Adapters)    (Persistence)
   │              │
   │              ▼
   │           Epic 5 (Bandwidth Accountant)
   │              │
   ▼              ▼
Epic 6 (Platform) — incl. NEW 6.5 (OHM acquire), 6.6 (hotkey/monitor/theme)
   │
   ▼
Epic 7 (Wiring) — incl. NEW 7.4 (event channel), 7.5 (shutdown)
   │
   ▼
Epic 8 (GUI) — incl. NEW 8.6 (theme), 8.7 (sparkline), 8.8 (alert), 8.9 (dnd), 8.10 (wizard)
   │
   ▼
Epic 9 (Release) — incl. NEW 9.3 (auto-update, optional/v1.1)
   ║
Epic 10 (Verify)
```

**Audit pass 3 deltas:** 11 new stories (6.5, 6.6, 7.4, 7.5, 8.6, 8.7, 8.8, 8.9, 8.10, 9.3) + 9 in-place expansions (1.5, 3.6, 4.2, 6.1, 6.4). New thresholds T-34..T-39. New fixtures F12, F13. New guardrails G23, G24. Total: **11 Epics, 54 Stories**.

---


---

## EPIC 11 — Regression Harness & Story Wiring (Audit Pass 4)
- **System Objective:** Build the cumulative regression harness, story-progress tracker, and UI snapshot infrastructure that guarantee zero regressions as stories accumulate. Every downstream story's PR depends on this Epic being in place.
- **Swarm Mapping:** QA Infrastructure Agent.

### STORY 11.1: Test Layer Scaffold + `regression-harness.md` Reference
- **User Story:** As the QA Infra Agent, I want the L0–L4 layer scaffold formalized as the canonical test-runner model so every story's tests declare their layer and the harness runs them in strict order.
- **Technical Context:** `regression-harness.md` §1 + T-40. Layer convention: tests in `crates/*/src/**.rs` inline = L0; `crates/*/tests/*.rs` = L1; `crates/sidebar-app/tests/snapshots/` = L2; `benches/*` = L3; `verify/` = L4. Each test module declares its layer via a doc-comment header.
- **Wiring:**
  - **Layer:** unit + integration + ui + bench (this story establishes the layers; its own tests span all four)
  - **Depends-On:** [0.1, 0.2]
  - **Blocks:** [11.2, 11.3, 11.4] AND every story that uses the `Layer:` field (i.e. all of Epic 1+)
  - **Next:** 11.2
  - **Parallel-With:** [0.3, 0.4, 0.5, 0.6, 1.1] (Epic 0/1 bootstrap can proceed in parallel since they only USE the layer model, don't modify it)
  - **DoD:** `docs/backlog/regression-harness.md` exists (✓ created in pass 4); CI runner has distinct jobs for L0/L1/L2/L3; an end-to-end smoke proves each layer executes.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm layer folder convention. Confirm Windows-only gating for L1/L3.
  2. [ ] **Implement:** Restructure `ci.yml` (Story 0.2) into four jobs: `lint`, `unit (L0)`, `integration (L1)`, `bench (L3)`. UI snapshot job (L2) added by Story 11.3.
  3. [ ] **Validate:** Trigger CI; all four jobs run and report independently.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F14:**
    1. CI workflow has distinct jobs for L0, L1, L3; each job's `name` matches the layer.
    2. `regression-harness.md` parses cleanly as markdown (lint).
  - **Boundary & Edge Case Test Cases (cite T-40):**
    1. L0 job exceeds 60s budget (T-40) → fails with `layer-budget-exceeded: L0`.
    2. L1 job runs on non-Windows runner → fails (L1 is Windows-only).
    3. L3 job skipped because not a Windows runner → fails (L3 mandatory on Windows).
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — harness architecture is a keystone.

### STORY 11.2: CI Regression Gate (full matrix per PR)
- **User Story:** As the QA Infra Agent, I want CI to run the FULL L0+L1+L2+L3 matrix on every PR (not just the touched crate) so the cumulative-regression contract (G25) is enforced.
- **Technical Context:** G25 + T-41 + T-43 + T-44 + F14. Extends Story 0.2's `ci.yml`. Adds: `cargo test --workspace --all-targets` (L0+L1 combined), `cargo test --test ui_snapshots` (L2), `cargo bench --bench '*'` (L3), coverage via **`cargo-llvm-cov`** (NOT tarpaulin — tarpaulin is Linux-only per dev-env.md §6.3 / T-43). Generates `regression-report.md` artifact. **Dev env prerequisite (T-44):** `rustup component add llvm-tools-preview` must be run on every dev/CI machine before this story's coverage gate works.
- **Wiring:**
  - **Layer:** unit + integration + ui + bench
  - **Depends-On:** [0.2, 11.1]
  - **Blocks:** every downstream story (the gate is what makes their PRs trustworthy)
  - **Next:** 11.3
  - **Parallel-With:** [] (must complete before any code story can rely on the gate)
  - **DoD:** A PR with a deliberate regression (e.g. break Story 1.3's `format_hz`) fails CI with a clear message naming the regressed story.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `Swatinem/rust-cache@v2` brings the matrix under T-41 (750s).
  2. [ ] **Implement:** Update `ci.yml` to run the full matrix + generate `regression-report.md`.
  3. [ ] **Validate:** Inject a regression; verify CI catches it.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Full matrix on a clean PR completes in ≤ 750s (T-41).
    2. `regression-report.md` artifact uploaded; contains all four layer summaries.
  - **Boundary & Edge Case Test Cases (cite T-40, T-41, T-42, G25, G26):**
    1. Inject regression in Story 1.3 (`format_hz` returns "X MHz" instead of "GHz"); CI fails; report names the failing test + the story it belongs to (via test-module doc-comment convention).
    2. Coverage delta < 0 (T-42) → CI fails with `coverage-regression: crate sidebar-domain -2.3%`.
    3. Matrix exceeds T-41 budget → fails with `regression-budget-exceeded`.
    4. Cache miss (cold build) → still completes within budget OR fails gracefully with actionable message.
- **Explicit Swarm Guardrails:** HITL on any change to the regression contract (G19).

### STORY 11.3: UI Snapshot Harness (`insta` + `egui_kittest`)
- **User Story:** As the QA Infra Agent, I want the UI snapshot infrastructure (`insta` + `egui_kittest`) wired into L2 with a self-contained reference snapshot, so GUI stories (Epic 8) can add their own snapshots on top without a circular dependency on 8.1.
- **Technical Context:** `regression-harness.md` §6.3 + F8 + F14. `insta = 1.40  # MIT/Apache-2.0`, `egui_kittest = 0.35  # MIT`. Snapshots in `crates/sidebar-app/tests/snapshots/*.snap`. New snapshots trigger `requires-hitl-snapshot` label. **Self-contained bootstrap:** this story ships a *reference* snapshot that renders a trivial egui label (e.g. `ui.label("sidebar snapshot harness OK");`) — it does NOT depend on any GUI story being merged first. This breaks what would otherwise be a cycle (8.1 needs insta; 11.3 needs something to snapshot). Subsequent GUI stories (8.x) add their own snapshots on top of this harness.
- **Wiring:**
  - **Layer:** ui
  - **Depends-On:** [0.1, 11.1]
  - **Blocks:** [8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9, 8.10] (these stories rely on the snapshot infrastructure)
  - **Next:** 8.2
  - **Parallel-With:** [11.4]
  - **DoD:** `cargo insta test` passes against the self-contained reference snapshot; an intentional snapshot change requires HITL review (`cargo insta accept`).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Snapshot file naming convention (`<story-id>__<test-name>.snap`). Review workflow (`cargo insta review`).
  2. [ ] **Implement:** `crates/sidebar-app/tests/snapshots/` + helper module + a `story_11_3__harness_bootstrap.snap` reference (renders a single egui `Label`). CI L2 job.
  3. [ ] **Validate:** `cargo insta test` passes on the reference snapshot; a deliberate change requires `cargo insta accept`.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path) — fixture F8:**
    1. The bootstrap test renders `ui.label("sidebar snapshot harness OK")` and the resulting snapshot matches `story_11_3__harness_bootstrap.snap`.
    2. `cargo insta test` exits 0 on a clean tree with only the reference snapshot present.
  - **Boundary & Edge Case Test Cases (cite G19, G26):**
    1. Snapshot drift (intentional change to the label text) → `cargo insta test` fails with diff; HITL must run `cargo insta accept` and re-push.
    2. Snapshot drift (unintentional — egui version bump changes rendering) → CI fails; report shows the diff.
    3. New snapshot file added without HITL review → CI warns (the `requires-hitl-snapshot` label check).
- **Explicit Swarm Guardrails:** HITL on every snapshot acceptance (G19).

### STORY 11.4: Story Progress Tracker (`PROGRESS.md` auto-update)
- **User Story:** As the QA Infra Agent, I want `docs/backlog/PROGRESS.md` auto-updated on every merge so the swarm can read it at task-startup to know which stories are done and which to pick next.
- **Technical Context:** `regression-harness.md` §6.4 + G27. CI job on `main` branch (post-merge) parses merged PRs (via `git log` + PR-title convention `Story X.Y:`), updates `PROGRESS.md` table, commits back to `main`.
- **Wiring:**
  - **Layer:** unit (the updater logic is pure; the CI job is integration)
  - **Depends-On:** [0.2, 11.2]
  - **Blocks:** nothing strictly, but the swarm RECOMMENDS this be in place before Epic 1 stories merge (so progress is tracked from the start)
  - **Next:** (terminal — no Next; this story enables the loop)
  - **Parallel-With:** [11.3]
  - **DoD:** After a sample PR titled `Story 0.1: ...` merges, `PROGRESS.md` row for 0.1 shows `merged` within one CI run.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** PR-title convention `Story X.Y[: description]`. Parser in Python or Rust. Commit-back via `stefanzweifel/git-auto-commit-action` or equivalent.
  2. [ ] **Implement:** `.github/workflows/track-progress.yml` + `tools/parse_progress.py` (or `.rs`).
  3. [ ] **Validate:** Dry-run on a test PR.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. Parser reads `Story 1.3: format module` → emits table row `| 1.3 | merged | <ts> | <PR> |`.
    2. Parser ignores non-conforming PR titles (`Fix typo` → no row).
  - **Boundary & Edge Case Test Cases (cite G27):**
    1. Multiple stories in one PR (anti-pattern but possible) → parser emits multiple rows with the same PR number; warns.
    2. PR title missing story ID → parser skips, logs `warn!` to CI output.
    3. `PROGRESS.md` schema change → CI fails fast (schema is a contract per G19).
    4. Reverted PR → row status changes from `merged` to `reverted` (track via `git log --revert`).
- **Explicit Swarm Guardrails:** HITL **mandatory** (G11/G19) — the swarm reads this file; tampering or schema drift = silent story-skipping.

---

---

## EPIC 12 — SidebarDiagnostics Parity & Integration Closure (post-Epic 11)

- **System Objective:** Close the deliberate parity gaps that remain after the
  Win11-native Epic 0–8 slice, without weakening the product boundary (Basic
  mode remains no-admin; bandwidth remains per-NIC; LHM remains a local HTTP
  sidecar).
- **Status:** Planned. Stories are optional/deferred where the original
  SidebarDiagnostics behavior conflicts with the lightweight/privacy mandate.
- **Dependency convention:** Epic 12 is post-Epic 11. Unless a story is
  explicitly marked optional/deferred, it has an implicit dependency on `11.4`
  in addition to the dependencies listed below. Optional/deferred stories may
  remain skipped; this convention prevents `12.8` from appearing ready before
  the regression-harness terminal enabler is complete.

### STORY 12.1: Clock/date header (optional UX parity)
- **User Story:** As a user, I want a compact local clock/date header so the sidebar is glanceable without opening another app.
- **Scope:** Add a locale-stable local-time display; no network time source. Depends on [8.1]. Optional polish. **Verified:** header renders `HH:MM` plus ISO date and formatter/render tests pass.

### STORY 12.2: Per-metric graphs and history
- **User Story:** As a power user, I want a short per-metric history graph (not only the current sparkline) so I can spot trends and throttling.
- **Scope:** Reuse bounded `RollingWindow` state, cap memory/render cost, and keep the current sparkline as the default. Depends on [8.7, 11.3].

### STORY 12.3: Complete hotkey and reposition actions
- **User Story:** As a user, I want the configured hotkey to toggle click-through and repositioning reliably, including conflict feedback.
- **Scope:** Finish `RegisterHotKey`/drag/reposition event wiring and monitor re-dock behavior. Depends on [6.6, 8.5].

### STORY 12.4: Customization parity (deferred)
- **User Story:** As a user, I want the original app's customization breadth (layout, metric presets, and theme details) without sacrificing startup/RSS budgets.
- **Scope:** Audit each requested option against NFR-1/NFR-4; ship only low-cost settings. Plugin/scripting systems remain out of scope. Depends on [8.5, 8.6].

### STORY 12.5: Battery health + adapter identity/IP (deferred)
- **User Story:** As a laptop/network user, I want battery health and friendly adapter identity/IP context alongside current counters.
- **Scope:** Add health only when a supported Windows source is available; surface adapter name/IP as metadata without changing LUID accounting. Depends on [3.3, 3.5, 8.4].

### STORY 12.6: Alert scope and actions
- **User Story:** As a user, I want alerts scoped per metric/device with an explicit action (acknowledge, snooze, or open settings), not only row coloring.
- **Scope:** Preserve hysteresis and non-blocking rendering; no telemetry or notification egress. Depends on [1.2, 8.8].

### STORY 12.7: Localization (optional/deferred)
- **User Story:** As an international user, I want localized labels and number/date formats.
- **Scope:** Keep v1 locale-stable (`.` decimal, no thousands separator) and add a locale parameter only after the format API is proven. Depends on [1.3, 12.1].

### STORY 12.8: Epic 0–8 integration closure (status pill, BandwidthView, OHM monitor)
- **User Story:** As the integration owner, I want the currently documented wiring contracts to be true in the production launch path.
- **Scope:** Wire the status-pill callback to `OhmSupervisor::launch_elevated` and publish tier events; connect the accountant's `BandwidthView` to `AppState`/GUI; run an app-level child-liveness monitor that emits Full→Basic on unexpected exit of a sidebar-launched LHM child. Add integration tests for each path and preserve the 3 s shutdown/watchdog budget. Depends on [6.4, 7.2, 7.3, 7.4, 7.5, 8.2, 8.4].
- **Current evidence (2026-07-12):** working-tree implementation covers all three paths; 625 workspace tests pass with 13 ignored and formatting/diff checks pass.
- **Acceptance:** no-op callback removed; live bandwidth panel updates from persisted/accounted state; child exit degrades exactly once; all existing tests remain green. **Still pending:** commit/PR review and real UAC/LHM, Job Object, and Win11 smoke acceptance; do not mark this story merged before those gates.

---

## EPIC 13 — Hardening for Non-Technical Users (Audit Pass 5, 2026-07-13)
- **System Objective:** Close the gap between "the workspace tests pass" and "a non-technical user can run this without calling support." Five focused hardening stories that make the app self-healing on corrupt config/DB, prevent double-instance clobbering, explain every settings control in plain language, and bottle the reference-machine evidence into a single command.
- **Status:** Planned 2026-07-13. All five stories are code-fixable with no external dependencies beyond the reference machine for the final runner (Story 13.5).
- **Dependency convention:** Epic 13 is post-Epic 12. Stories 13.1–13.4 are independent of each other and may run in parallel (single-trunk PRs per G4). Story 13.5 depends on 13.1–13.4 landing first so the runner can exercise the hardened paths. No story in Epic 13 blocks the external SignPath/release gates (Epic 9), but Epic 13 SHOULD merge before the v1.0.0 tag so the hardened build is what gets signed.
- **LHM one-time-click policy (Path A, approved 2026-07-13):** The bundled LHM v0.9.6 binary does not auto-start its HTTP server from any config key. Rather than upgrade LHM (Path B, rejected — re-pin/re-hash/re-test risk), Epic 13 documents the one-time `View → Web Server` click in the first-run wizard (Story 13.4's About dialog) and in `verify/smoke-checklist.md` (Story 13.5). This is the only non-idiot-proof step in v1.0.0; a v1.1 story may revisit the LHM upgrade.

### STORY 13.1: Atomic config writes + corrupt-file backup
- **User Story:** As a non-technical user, I want my settings to survive a crash or a corrupted config file, so I don't silently lose my preferences.
- **Technical Context:** `crates/sidebar-app/src/main.rs:898-917` (`load_config`) + `crates/sidebar-app/src/gui/mod.rs:549-571` (`persist_config`). guardrails.md G15 (non-fatal recovery) + G28 (non-technical-user hardening). `persist_config` currently uses bare `std::fs::write` (non-atomic — a crash mid-write truncates the file). `load_config` returns `Config::default()` on malformed TOML but leaves the corrupt file on disk to be overwritten on the next settings edit (destroying forensic evidence). Fixture F15 (corrupt-file quarantine + atomic-write harness).
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [1.5, 8.5]
  - **Blocks:** [13.5]
  - **Next:** 13.2
  - **Parallel-With:** [13.2, 13.3, 13.4]
  - **DoD:** `persist_config` writes via `config.toml.tmp` + `std::fs::rename` (atomic on NTFS same-volume); `load_config` copies a malformed file to `config.toml.corrupt-<unix_timestamp>` before returning defaults; three tests (malformed recovery, backup-with-timestamp, atomic-write) pass; full workspace regression green.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm NTFS same-volume rename is atomic (it is, per Microsoft docs). Decide the timestamp format (`unix_timestamp` — sortable, no colons in filenames).
  2. [ ] **Implement:** Extract an `atomic_write(path, contents)` helper in `gui/mod.rs` (write-to-tmp + rename, returns `Result`). Extract `backup_corrupt(path)` in `main.rs` (copy to `<path>.corrupt-<ts>`, best-effort, logs on failure). Wire both into `persist_config` and `load_config`.
  3. [ ] **Validate:** `cargo test -p sidebar-app --lib` green; `cargo test --workspace --all-targets` green.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `load_config_recovers_from_malformed_toml` — write `b"not = a = valid = toml"` to a TempDir config path; `load_config` returns `Config::default()` and does not panic. Fixture F15.
    2. `load_config_backs_up_corrupt_file_with_timestamp` — after the above, the TempDir contains a file matching `config.toml.corrupt-*` whose content is the original garbage. Fixture F15.
    3. `persist_config_writes_atomically_via_temp_rename` — call `persist_config`; assert `config.toml` exists and no `config.toml.tmp` remains (rename succeeded). Fixture F15.
  - **Boundary & Edge Case Test Cases (cite G15, G28, F15):**
    1. Config dir is read-only — `persist_config` logs `warn!` and does not panic (G15 non-fatal); no `.tmp` left behind.
    2. Corrupt-file backup fails (disk full) — `load_config` still returns defaults; logs the backup failure at `warn!` but does not panic (G15).
    3. Concurrent writes from two threads — atomic rename guarantees the final file is one of the two writes, never a mix (assert via two threads + read-back).
- **Explicit Swarm Guardrails:** No new dependency (use `std::fs`). No `unsafe` (pure Rust). Cite G15 (non-fatal recovery) + G28 (hardening) in every `warn!` log line.

### STORY 13.2: SQLite corruption quarantine + auto-recreate
- **User Story:** As a non-technical user, I want bandwidth tracking to recover automatically if the database file gets corrupted, so I don't lose the feature forever with no way to fix it.
- **Technical Context:** `crates/sidebar-app/src/main.rs:549-597` (`run_accountant_on_thread`) + `crates/sidebar-persistence/src/schema.rs:40-104` (`init`) + `crates/sidebar-persistence/src/lib.rs`. guardrails.md G15 + G21 (SQLite discipline) + G28. The existing test `init_surfaces_error_on_corrupt_file` (`schema.rs:189-213`) locks in "init MUST NOT overwrite a corrupt file" — that contract stays. The gap: `run_accountant_on_thread` gives up entirely on `schema::init` failure, permanently disabling bandwidth tracking. Fixture F15.
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [4.1, 4.2]
  - **Blocks:** [13.5]
  - **Next:** 13.3
  - **Parallel-With:** [13.1, 13.3, 13.4]
  - **DoD:** New `pub fn quarantine_and_reopen(db_path: &Path) -> Result<Connection>` in `sidebar-persistence` renames `bandwidth.db` → `bandwidth.db.corrupt-<ts>`, opens a fresh connection, calls `schema::init`, returns the conn. `run_accountant_on_thread` calls it on `schema::init` failure instead of giving up. Three tests pass; full regression green.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `schema::init`'s "must not overwrite corrupt" contract is preserved (quarantine is a separate function, not a weakening of `init`). Decide the quarantine filename format (`bandwidth.db.corrupt-<unix_timestamp>`).
  2. [ ] **Implement:** Add `quarantine_and_reopen` to `sidebar-persistence/src/lib.rs` (or a new `quarantine.rs` module). Wire it into `run_accountant_on_thread`'s error path. Log the quarantine path at `warn!`.
  3. [ ] **Validate:** `cargo test -p sidebar-persistence` green; `cargo test -p sidebar-app --lib` green.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `quarantine_renames_corrupt_file_with_timestamp` — write garbage to a TempDir `bandwidth.db`; call `quarantine_and_reopen`; assert the original garbage file is now at `bandwidth.db.corrupt-*`. Fixture F15.
    2. `quarantine_reopens_fresh_connection_with_clean_schema` — after quarantine, the returned `Connection` passes `schema::init` (no error) and the schema tables exist. Fixture F15.
  - **Boundary & Edge Case Test Cases (cite G15, G21, G28, F15):**
    1. `quarantine_preserves_corrupt_file_for_forensics` — the renamed-aside file's bytes are byte-identical to the original garbage (no mutation).
    2. Quarantine rename fails (e.g. target path is read-only) — `quarantine_and_reopen` returns `Err(Error::Io(...))`; `run_accountant_on_thread` logs + disables the accountant (G15 — does not crash the host).
    3. `run_accountant_on_thread` recovers after quarantine — the accountant runs against the fresh DB and persists a sample reading without error.
- **Explicit Swarm Guardrails:** No new dependency. No `unsafe`. Do NOT weaken `schema::init`'s existing contract. Cite G15 + G21 + G28 in log lines.

### STORY 13.3: Single-instance named-mutex guard
- **User Story:** As a non-technical user who double-clicks the exe, I want the second click to do nothing (not launch a second instance that clobbers my settings and registers a second AppBar).
- **Technical Context:** `crates/sidebar-platform/src/lib.rs` (new `single_instance` module) + `crates/sidebar-app/src/main.rs:93-94` (top of `main`, after `init_tracing`). guardrails.md G2 (unsafe policy) + G10 (ownership — analog) + G28. Win32 `CreateMutexW` + `GetLastError() == ERROR_ALREADY_EXISTS`. The `windows` crate already has `Win32_System_Threading` + `Win32_Foundation` + `Win32_Security` features enabled (`sidebar-platform/Cargo.toml:16-28`). Fixture F11 (unsafe FFI test).
- **Wiring:**
  - **Layer:** unit + integration (the integration test spawns a child process)
  - **Depends-On:** [0.1, 6.4]
  - **Blocks:** [13.5]
  - **Next:** 13.4
  - **Parallel-With:** [13.1, 13.2, 13.4]
  - **DoD:** New `sidebar_platform::single_instance::claim_or_exit()` calls `CreateMutexW` with `Global\sidebar-app-single-instance`; if `GetLastError` returns `ERROR_ALREADY_EXISTS`, logs + `std::process::exit(0)`. `main.rs` calls it right after `init_tracing()`. Two tests pass (constant pin + child-process second-launch exits 0). Full regression green.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Confirm `Global\` prefix (cross-session) vs `Local\` (per-session) — use `Global\` so a second user session also gets the guard. Confirm the mutex handle must outlive `claim_or_exit` (leak it via `Box::leak` — lives until process exit, which is the intent).
  2. [ ] **Implement:** `crates/sidebar-platform/src/single_instance.rs` — `claim_or_exit() -> !` with `CreateMutexW` + `GetLastError` + `std::process::exit(0)`. Add `pub mod single_instance;` to `lib.rs`. Wire into `main.rs` after `init_tracing()`.
  3. [ ] **Validate:** `cargo test -p sidebar-platform` green; `cargo test -p sidebar-app --lib` green; `actionlint` clean.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `mutex_name_is_global_sidebar_app_single_instance` — pin the constant string (compile-time guard against accidental rename). Fixture F11.
    2. `claim_succeeds_on_first_call` — spawn the binary as a child process (mirroring `e2e_launch_smoke.rs`); assert it exits 0 within 5s (did not trip the mutex on first launch).
  - **Boundary & Edge Case Test Cases (cite G2, G10, G28, F11):**
    1. Second launch detects the first — spawn two child processes back-to-back; the second exits 0 within 2s (tripped the mutex) while the first keeps running. (May require a `--hold-open` test flag to keep the first alive; document if so.)
    2. Mutex creation fails (extremely unlikely — kernel out of handles) — `claim_or_exit` logs `error!` and falls through (does not block the launch — better to risk a double-instance than to block the user from the app entirely). Document this tradeoff in the SAFETY comment.
- **Explicit Swarm Guardrails:** HITL on the `unsafe` block per G2/G19 (reviewer confirms the SAFETY invariant holds on Win11 24H2 + 25H2). Every `unsafe` block has a `// SAFETY:` comment (workspace lint `clippy::undocumented_unsafe_blocks = "deny"`). No new dependency.

### STORY 13.4: Settings tooltips + jargon cleanup + About dialog
- **User Story:** As a non-technical user, I want every setting explained in plain language and a way to see what this app is + how to use Full mode, so I don't have to Google "GB vs GiB" or wonder how to get temperature readings.
- **Technical Context:** `crates/sidebar-app/src/gui/settings_panel.rs:66-192` (8 sections, only 1 has a tooltip) + `crates/sidebar-app/src/gui/mod.rs:1282-1298` (header gear). guardrails.md G28 + nfr-thresholds.md T-37 (first-run wizard). New `about.rs` module mirroring `first_run.rs:99-197`. Version string via `env!("CARGO_PKG_VERSION")` (already used at `main.rs:393`).
- **Wiring:**
  - **Layer:** ui (kittest) + unit
  - **Depends-On:** [8.5, 8.6]
  - **Blocks:** [13.5]
  - **Next:** 13.5
  - **Parallel-With:** [13.1, 13.2, 13.3]
  - **DoD:** Every settings section has an `on_hover_text(...)` with a plain-language explanation. Three jargon labels renamed ("Show raw values (Hz/bytes)" → "Show technical units"; "Byte base" → "Size units"; "Poll interval (seconds)" → "Refresh rate (seconds)"). New About dialog (ⓘ button next to the gear) shows version + LHM credit + privacy-policy link + GitHub issues link + the LHM one-time-click instructions. Kittest assertions for every tooltip + the About dialog content.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Draft the 8 tooltip strings + 3 renamed labels + About dialog content. Review for plain-language register (target: a user who doesn't know what "binary" means).
  2. [ ] **Implement:** Add tooltip constants to `settings_panel.rs` (mirror `NO_RESPLIT_TOOLTIP` at line 52). Add `on_hover_text` calls. New `gui/about.rs` with `render_about(ui, open)`. Wire `about_open` into `SidebarView` + the header.
  3. [ ] **Validate:** `cargo test -p sidebar-app --lib` green; kittest assertions for each tooltip + About dialog pass.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `settings_panel_renders_tooltip_for_every_section` — kittest harness; assert each section's tooltip text is queryable. Fixture F8.
    2. `settings_panel_renamed_jargon_labels` — assert "Show technical units", "Size units", "Refresh rate" are present; "raw values", "Byte base", "Poll interval" are NOT (the old jargon is gone). Fixture F8.
    3. `about_dialog_renders_version_lhm_credit_privacy_link` — open the About dialog; assert `env!("CARGO_PKG_VERSION")`, "LibreHardwareMonitor", "Privacy Policy", "GitHub" appear. Fixture F8.
  - **Boundary & Edge Case Test Cases (cite G28, T-37, F8):**
    1. About dialog closes when the user clicks the close button (kittest click + assert `open == false`).
    2. About dialog Full-mode instructions contain the literal phrase "View → Web Server" (the Path A one-time-click documentation).
    3. Tooltips render on hover (kittest `on_hover_text` registers the text in the access tree).
- **Explicit Swarm Guardrails:** No new dependency. No `unsafe`. HITL on the tooltip wording (G19 — first-impression UX review, analog to Story 8.10). Cite G28 + T-37 in doc comments.

### STORY 13.5: Reference-machine runner script + LHM one-time-click docs
- **User Story:** As the release engineer, I want a single command that runs every evidence gate on the designated reference machine (T-31) and produces a bundle I can attach to the release, so the v1.0.0 tag is backed by reproducible proof rather than ad-hoc notes.
- **Technical Context:** New `verify/reference-machine.ps1` + new `verify/evidence/` directory. nfr-thresholds.md T-46 (new). guardrails.md G25 (cumulative regression) + G28. Mirrors `verify/smoke-checklist.ps1` (the `Invoke-SmokeItem` pattern) + `scripts/env.ps1` (the `$PSScriptRoot` root derivation). The script runs: pre-flight → build → full L0-L3 matrix → all 13 `#[ignore]`'d tests → NFR-1 bench → scriptable smoke → exe SHA-256 → 12 manual items (prompted) → verdict + evidence bundle under `verify/evidence/<date>/`.
- **Wiring:**
  - **Layer:** smoke (L4) + integration (the structural test)
  - **Depends-On:** [13.1, 13.2, 13.3, 13.4, 10.2]
  - **Blocks:** — (terminal for Epic 13)
  - **Next:** — (terminal — Epic 13 closure; the next gate is the external SignPath submission, Epic 9)
  - **Parallel-With:** —
  - **DoD:** `verify/reference-machine.ps1` exists, is actionlint-clean (well, pwsh-syntax-clean), runs end-to-end on the reference machine producing `verify/evidence/<date>/{workspace-tests.txt, ignored-suite.txt, poll_cost.txt, scriptable-smoke.txt, sha256.txt, manual-smoke.md}`, and exits 0 on full PASS / 1 on any failure. A structural Rust test asserts the script exists + contains the required sections.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Inventory the 13 `#[ignore]`'d tests (already done in the audit). Decide the manual-item prompt UX (Read-Host per item, write PASS/FAIL to `manual-smoke.md`).
  2. [ ] **Implement:** `verify/reference-machine.ps1` + `verify/evidence/.gitkeep`. Structural test in `crates/sidebar-app/tests/reference_machine_runner.rs`.
  3. [ ] **Validate:** Dry-run the script's pre-flight + build + test stages on the reference machine (skip the manual-item prompts). Structural test passes.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):**
    1. `reference_machine_script_exists_and_is_well_formed` — assert `verify/reference-machine.ps1` exists, contains `#Requires -Version 7.0`, contains section markers for each of the 10 stages, and uses the `exit 0`/`exit 1` convention. Fixture F14.
    2. `evidence_directory_exists_with_gitkeep` — assert `verify/evidence/.gitkeep` exists (so the directory is tracked by git).
  - **Boundary & Edge Case Test Cases (cite T-46, G25, G28, F14):**
    1. Script pre-flight fails on missing Rust → exits 1 with a clear message (assert via a mocked `cargo --version` failure path, or by inspecting the script's pre-flight block).
    2. Script exits non-zero if any automated stage fails (assert by inspecting the `$ErrorActionPreference = 'Stop'` + `throw` convention).
- **Explicit Swarm Guardrails:** No new Rust dependency. HITL on the manual-item wording (the 12 prompts are the human-walker UX). Cite T-46 + G25 + G28 in the script header.

---

## EPIC 14 — Silent-Failure Elimination (Productization Pass, 2026-07-16)
- **System Objective:** Eliminate every place where a non-technical user does something and nothing visibly happens. Five silent-failure traps were identified in the 2026-07-16 productization audit: wizard dead-end, launch-failure silence, per-sensor staleness invisibility, config-corruption silence, DB-corruption silence. Each is a trust-killer. This epic closes all five.
- **Status:** Planned 2026-07-16. All five stories MUST land before the v1.0.0 tag (per maintainer directive 2026-07-16: "all of these before v1, not v1.1").
- **Dependency convention:** Epic 14 is post-Epic 13. Stories 14.1-14.5 are independent and parallelizable (disjoint file ownership). No story in Epic 14 blocks the external SignPath/release gates (Epic 9), but Epic 14 SHOULD merge before the v1.0.0 tag so the hardened build is what gets signed.

### STORY 14.1: Launch-failure visibility (launch_result_rx → user banner)
- **User Story:** As a non-technical user, when I click the BASIC pill and accept (or decline) the UAC prompt, I want to SEE whether it worked — not a silent gray pill with no explanation.
- **Technical Context:** `crates/sidebar-app/src/main.rs:215-225` (supervisor thread swallows `launch_elevated` Err into `tracing::warn!`). `crates/sidebar-platform/src/ohm_supervisor.rs:440-448` (T-11 timeout returns Err). `crates/sidebar-app/src/gui/mod.rs:1173` (degraded_message only fires for child-EXIT, not launch-FAILURE). guardrails.md G28 (non-technical-user hardening) + G29 (silent-failure surfaces).
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [12.8]
  - **Blocks:** [14.5]
  - **Next:** 14.2
  - **Parallel-With:** [14.2, 14.3, 14.4, 14.5]
  - **DoD:** A `LaunchOutcome` enum (Success/UacDeclined/Timeout/BinaryMissing/PortChainExhausted) is sent from the supervisor thread to the GUI via a new `watch::Receiver<Option<LaunchOutcome>>`. The GUI renders actionable banners: UacDeclined → "You declined the permission prompt. Click the pill to try again."; Timeout → "The hardware monitor didn't respond. Click again, or restart sidebar."; BinaryMissing → "Bundled monitor binary is missing. Reinstall sidebar."
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Define `LaunchOutcome` in sidebar-domain (pure enum, no deps). Wire the watch channel in main.rs alongside the existing `bandwidth_view_rx`.
  2. [ ] **Implement:** Supervisor thread sends outcome on launch completion/failure. GUI drains it in `logic()` + sets a `launch_message: Option<String>` field rendered as a banner above the metric rows. Auto-clear on first successful Full broadcast.
  3. [ ] **Validate:** Unit test the outcome→message mapping; kittest that the banner renders; integration test the watch channel plumbing.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) UacDeclined maps to the "declined" message string. (2) Success maps to None (no banner). (3) The banner auto-clears when tier flips to Full with a non-empty broadcast.
  - **Boundary & Edge Case Test Cases (cite G29):** (1) A launch that times out after T-11 surfaces the Timeout message, not a silent gray pill. (2) BinaryMissing surfaces the reinstall message. (3) The banner does NOT auto-clear if the user just dismisses it manually (needs a dismiss button).
- **Explicit Swarm Guardrails:** No new dependency. Cite G29 (silent-failure surfaces) in every message string's doc comment.

### STORY 14.2: Wizard hot-start (no restart required)
- **User Story:** As a non-technical user completing the first-run wizard, I want sensors to appear immediately — not a dead "Restart sidebar" string with no restart button.
- **Technical Context:** `crates/sidebar-app/src/gui/mod.rs:1221-1232` (wizard completion shows dead string). `crates/sidebar-app/src/main.rs:443-454` (poller/accountant/supervisor gated on `wizard_active` for the whole process lifetime). guardrails.md G24 (first-run gate) + G29.
- **Wiring:**
  - **Layer:** integration
  - **Depends-On:** [8.10, 7.2, 7.5]
  - **Blocks:** [14.5]
  - **Next:** 14.3
  - **Parallel-With:** [14.1, 14.3, 14.4, 14.5]
  - **DoD:** After wizard Continue/Skip, a `tokio::sync::Notify` signal hot-starts the poller, accountant, and supervisor in-session. The user sees a "Starting sensors…" interim banner until the first non-empty broadcast. No restart required.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** The building blocks exist (channels, `with_*` builders, `drain_broadcast`). The hard part is wiring background-task handles into AppState post-construction.
  2. [ ] **Implement:** Add `state.attach_readings_rx(...)` + `state.attach_bandwidth_view_rx(...)` setters. On wizard completion, fire the Notify signal → main.rs spawns the poller/accountant/supervisor inline + attaches the receivers.
  3. [ ] **Validate:** Integration test that wizard completion → first broadcast arrives within 5s without a restart.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) Wizard Continue fires the Notify signal. (2) The poller spawns within 2s of the signal.
  - **Boundary & Edge Case Test Cases (cite G24, G29):** (1) If the poller fails to spawn (e.g. runtime error), the user sees an error banner, not a silent hang. (2) The wizard gate (G24) still holds — the poller does NOT start until the user clicks Continue/Skip.
- **Explicit Swarm Guardrails:** HITL on the hot-start UX (G19 — first impression). Cite G24 (the poller is still gated on wizard completion; this story removes the RESTART requirement, not the gate).

### STORY 14.3: Per-sensor staleness detection + visual indicator
- **User Story:** As a non-technical user, if one sensor (e.g. GPU temp) hangs while others keep working, I want to see that it's stale — not a frozen number that looks plausible.
- **Technical Context:** `Reading.timestamp` already exists per-reading (`reading.rs:330`, stamped in `poller.rs:492-494`). The render loop (`gui/mod.rs:1540-1601`) iterates readings but never checks timestamps. The poller-level stale badge (`gui/mod.rs:1251-1263`) only fires on a TOTAL blackout. guardrails.md G29.
- **Wiring:**
  - **Layer:** ui + unit
  - **Depends-On:** [8.3]
  - **Blocks:** [14.5]
  - **Next:** 14.4
  - **Parallel-With:** [14.1, 14.2, 14.4, 14.5]
  - **DoD:** In the render loop, each reading's `timestamp` is compared to `Instant::now()`. Past the threshold (3× poll interval, clamped [15s, 120s]), the row renders dimmed + a `⏱` glyph + tooltip "This sensor hasn't updated in X seconds."
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** No domain change — `Reading.timestamp` is already there. Pure render logic.
  2. [ ] **Implement:** In `render_sidebar_mut`, compute `is_stale = reading.timestamp.elapsed() > threshold`. Pass a `stale: bool` flag to the row renderer. Reuse `metric_row::render` with a dimmed color.
  3. [ ] **Validate:** Kittest that a stale reading renders dimmed + the glyph; a fresh reading renders normally.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) A reading stamped `now` renders as fresh (no dim). (2) A reading stamped 60s ago (poll=10s → threshold=30s) renders as stale (dimmed + glyph).
  - **Boundary & Edge Case Test Cases (cite G29):** (1) The threshold scales with poll interval (poll=1s → threshold=3s, clamped to 15s min). (2) An empty readings vec shows WAITING_TEXT, not a stale badge.
- **Explicit Swarm Guardrails:** No new dependency. Cite G29.

### STORY 14.4: Config-corruption + DB-corruption user banners
- **User Story:** As a non-technical user whose settings or bandwidth history got corrupted, I want to SEE that it happened + where my backup is — not a silent reset to defaults that makes me think I lost everything.
- **Technical Context:** `crates/sidebar-app/src/main.rs:930-978` (`load_config` + `backup_corrupt_file` — both `tracing::warn!`-only). `crates/sidebar-app/src/main.rs:581-602` (`quarantine_and_reopen` — warn-only). Story 13.1/13.2 added the quarantine + recovery; this story surfaces it to the user. guardrails.md G29.
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [13.1, 13.2]
  - **Blocks:** [14.5]
  - **Next:** 14.5
  - **Parallel-With:** [14.1, 14.2, 14.3, 14.5]
  - **DoD:** Two one-shot flags: `config_recovered: Option<PathBuf>` (backup path) + `db_quarantined: Option<PathBuf>`. Rendered as dismissible banners: "Your settings file was unreadable and we reset to defaults. Your old file was backed up at `<path>`." / "Bandwidth history was corrupted; starting fresh. Old data was backed up at `<path>`."
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Mirror the `degraded_message` pattern. The backup paths are already computed in `backup_corrupt_file` + `quarantine_and_reopen` — propagate them to the GUI.
  2. [ ] **Implement:** Return `Option<PathBuf>` from the recovery functions. Pass to SidebarApp. Render banners with a dismiss (X) button.
  3. [ ] **Validate:** Unit test the flag is set on corrupt input; kittest the banner renders.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) A corrupt config.toml sets `config_recovered` to the backup path. (2) A corrupt bandwidth.db sets `db_quarantined` to the backup path.
  - **Boundary & Edge Case Test Cases (cite G29):** (1) The banner dismiss button clears the flag (one-shot). (2) If the backup itself fails, the banner still fires but without a path ("…back up failed; settings were reset.").
- **Explicit Swarm Guardrails:** Cite G29 + G15 (non-fatal recovery).

### STORY 14.5: Generalized user-message stack (Vec<UserMessage> framework)
- **User Story:** As the product owner, I want a single general framework for surfacing user-facing messages (info/warning/error), so stories 14.1-14.4 + future conditions all feed into one dismissible stack rather than ad-hoc fields.
- **Technical Context:** Currently `degraded_message: Option<&'static str>` (`gui/mod.rs:387`) is the only user-facing surface. Stories 14.1-14.4 each need their own message. This story promotes the ad-hoc field to a `Vec<UserMessage>` with severity + dismiss semantics. guardrails.md G29.
- **Wiring:**
  - **Layer:** ui + unit
  - **Depends-On:** [14.1, 14.2, 14.3, 14.4]
  - **Blocks:** — (terminal for Epic 14)
  - **Next:** 15.1
  - **Parallel-With:** —
  - **DoD:** `UserMessage { severity: Severity, text: String, dismissable: bool, id: MessageId }` struct. Rendered as a vertical stack of banners above the metric rows. Each of 14.1-14.4 feeds into it. Existing `degraded_message` migrates to a `UserMessage` with `Severity::Warning`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Define `Severity { Info, Warning, Error }` + `MessageId` (enum for dedup). The stack lives on SidebarApp.
  2. [ ] **Implement:** Replace `degraded_message` with `push_message()`. Migrate 14.1-14.4 producers. Render stack in `ui()`.
  3. [ ] **Validate:** Kittest that multiple messages stack vertically; dismiss removes one, not all.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) Three messages stack in insertion order. (2) Dismissing by MessageId removes only that one. (3) Auto-clear rules (e.g. degraded clears on Full broadcast) still fire.
  - **Boundary & Edge Case Test Cases (cite G29):** (1) Duplicate MessageId is deduped (no message spam). (2) The stack doesn't grow unbounded (max 5; oldest dropped).
- **Explicit Swarm Guardrails:** HITL on the message wording (G19). Cite G29.

---

## EPIC 15 — LHM Library Host Architecture (Productization Pass, 2026-07-16)
- **System Objective:** Eliminate the LibreHardwareMonitor.exe HTTP-auto-start regression by consuming `LibreHardwareMonitorLib.dll` directly via a tiny elevated .NET host process. This is THE architectural move that makes "click pill → sensors appear" reliable. The current dependency on LHM-the-GUI (with its flaky HTTP server, LHM Issue #1855) is the single biggest productization blocker.
- **Status:** Planned 2026-07-16. MUST land before v1.0.0 (per maintainer directive). The 2026-07-16 deep investigation confirmed: the library API is stable + documented (`LibreHardwareMonitorLib.xml` exposes `Computer.Open()` + `Hardware[].Sensors[]`), the DLL is already shipped, no new runtime is needed (.NET 4.7.2 is already required by the GUI).
- **Dependency convention:** Epic 15 is post-Epic 14 (the launch-failure visibility from 14.1 is a prerequisite — even with the lib-host, launch can fail). Stories 15.1→15.2→15.3 are sequential (host → trait refactor → delete dead HTTP code).

### STORY 15.1: Elevated .NET sensor host (sidebar-monitor-host)
- **User Story:** As the sidebar runtime, I want a lean elevated process that loads LibreHardwareMonitorLib.dll + emits sensor frames over stdout, so I never depend on LHM's HTTP server starting.
- **Technical Context:** New `resources/sidebar-monitor-host.exe` (~50 lines C#). Loads `LibreHardwareMonitor.Hardware.Computer`, enables CPU/GPU/RAM/motherboard/storage/battery, walks `Hardware[].Sensors[]`, emits JSON to stdout in the same shape as LHM's `/data.json` (sidebar already parses this — `lhm_model.rs`). Runs elevated (ring-0 MSR/SMBIOS access) via the same `ShellExecuteExW("runas")` + Job Object wiring as the current LHM launch. guardrails.md G10 (ownership) + G16 (zero runtime egress — the host emits to a pipe, not HTTP).
- **Wiring:**
  - **Layer:** integration + smoke
  - **Depends-On:** [14.1]
  - **Blocks:** [15.2]
  - **Next:** 15.2
  - **Parallel-With:** —
  - **DoD:** `sidebar-monitor-host.exe` exists, is hash-pinned like LHM, loads the library, emits valid JSON sensor frames to stdout on demand (or continuously). Integration test: spawn the host, read stdout, parse the JSON, assert sensor coverage matches the current LHM HTTP path.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Decide stdout protocol (continuous JSON-lines vs request/response over stdin). Continuous is simpler — sidebar reads the latest frame each poll tick.
  2. [ ] **Implement:** C# project in `resources/sidebar-monitor-host/`. Build produces a signed EXE. The host enables all sensor categories + emits one JSON frame per second (or on stdin newline).
  3. [ ] **Validate:** Run the host standalone; verify JSON shape matches `lhm_model.rs` expectations.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) The host emits at least one JSON frame within 2s of launch. (2) The JSON parses as `Vec<LhmNode>`.
  - **Boundary & Edge Case Test Cases (cite G10, G16):** (1) If the library fails to load (e.g. .NET 4.7.2 missing), the host exits non-zero with a clear stderr message. (2) The host does NOT open any network socket (G16 — pipe only).
- **Explicit Swarm Guardrails:** HITL mandatory (G11/G19) — this is a new signed binary running elevated. The C# source MUST be in the repo (not just the binary) for audit. Cite G10 (sidebar kills only hosts it launched) + G16 (pipe, not HTTP).

### STORY 15.2: SensorSource trait refactor + pipe client
- **User Story:** As the sidebar adapter layer, I want a `SensorSource` trait so I can consume either the new pipe host OR the old HTTP path (for fallback / testing) without changing the adapter.
- **Technical Context:** `crates/sidebar-adapter-ohm/src/http.rs:153` (`RealHttpClient` + `HttpClient` trait). `crates/sidebar-adapter-ohm/src/lib.rs:154` (`format!("http://127.0.0.1:{}/data.json")`). Refactor `HttpClient` → `SensorSource` with two impls: `HttpSource` (existing, for fallback) + `PipeSource` (new, reads the host's stdout). guardrails.md G16 (loopback validation now applies to the pipe, not HTTP — but the contract is "local process only").
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [15.1]
  - **Blocks:** [15.3]
  - **Next:** 15.3
  - **Parallel-With:** —
  - **DoD:** `SensorSource` trait replaces `HttpClient`. `PipeSource` spawns the host via `StdCommand::new(host_exe).stdout(Stdio::piped())`, reads the latest JSON frame, parses it. The adapter consumes the trait, not a specific impl. `OhmSupervisor` is generic over `SensorSource`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** The trait method is `fn read_frame(&self) -> Result<String>` (JSON body) — same shape as the current `HttpClient::get`.
  2. [ ] **Implement:** Rename `HttpClient` → `SensorSource`. Add `PipeSource`. Wire `OhmSupervisor` to spawn the host + wrap its stdout in `PipeSource`.
  3. [ ] **Validate:** Unit test `PipeSource` parsing; integration test the supervisor→host→frame chain.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `PipeSource::read_frame` returns a valid JSON string. (2) The adapter parses it identically to the HTTP path.
  - **Boundary & Edge Case Test Cases (cite G16, G10):** (1) Host exits mid-read → `Err` surfaced + child-liveness degrades to Basic. (2) Malformed JSON → `Err` (same as HTTP path).
- **Explicit Swarm Guardrails:** No new Rust dependency (`std::process::Command` is stdlib). Cite G16 (pipe is local-process only, not network).

### STORY 15.3: Delete dead HTTP code + config-patching + port-fallback
- **User Story:** As the codebase, I want the HTTP client, the LHM config-patching, and the 11-port fallback chain removed — they're dead code once the pipe host is the primary path.
- **Technical Context:** `crates/sidebar-adapter-ohm/src/http.rs` (validate_loopback_url, MAX_BODY_BYTES, RealHttpClient — all become dead if PipeSource is primary). `crates/sidebar-platform/src/ohm_supervisor.rs:896-1076` (`patch_lhm_config`, `patch_lhm_user_config`, `update_app_setting_key` — dead, no HTTP server to configure). `ohm_supervisor.rs:834-872` (`pick_free_port`, the T-45 11-port chain — dead, no port to pick). guardrails.md G17 (deletion over addition).
- **Wiring:**
  - **Layer:** unit
  - **Depends-On:** [15.2]
  - **Blocks:** — (terminal for Epic 15)
  - **Next:** 16.1
  - **Parallel-With:** —
  - **DoD:** `HttpSource` kept as a `#[cfg(test)]` mock-only impl (for adapter unit tests). Production path uses `PipeSource` exclusively. The LHM config-patching + port-fallback code is deleted. The `validate_loopback_url` + `MAX_BODY_BYTES` tests move to the `HttpSource` test module (they're still valid for the test impl).
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Audit every reference to HTTP/port/config-patch. Confirm `PipeSource` covers every production call site.
  2. [ ] **Implement:** Delete the dead code. Move HTTP tests to `#[cfg(test)]`. Update docs (G16, T-10, T-45 — note the HTTP path is now test-only).
  3. [ ] **Validate:** Full workspace regression green.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) No production code references `validate_loopback_url` or `pick_free_port`. (2) The adapter test suite still passes via `HttpSource` mock.
  - **Boundary & Edge Case Test Cases (cite G17):** (1) The deletion does not weaken G16 (the pipe is local-process, stricter than loopback HTTP).
- **Explicit Swarm Guardrails:** Cite G17 (deletion over addition) + G3 (no dead code).

---

## EPIC 16 — Windows Service + Installer (Productization Pass, 2026-07-16)
- **System Objective:** Ship a Windows Service (`sidebar-monitor-svc.exe`) that owns the elevated sensor host + an Inno Setup installer that registers the service — so UAC happens ONCE (at install time) and the user never sees a recurring elevation prompt. This is the maintainer's chosen path (Option A) for reliable, set-and-forget elevation.
- **Status:** Planned 2026-07-16. MUST land before v1.0.0 (per maintainer directive). The installer is **Inno Setup** (free, OSS, used by VS Code, winget-compatible via `InstallerType: inno` with the known `PrivilegesRequired` workaround documented in [winget-cli #254](https://github.com/microsoft/winget-cli/issues/254)).
- **Dependency convention:** Epic 16 is post-Epic 15 (the service owns the lib-host from 15.1). Stories 16.1→16.2→16.3→16.4 are sequential (service → IPC → installer → CI).

### STORY 16.1: sidebar-monitor-svc (Windows Service binary)
- **User Story:** As the sidebar, I want a Windows Service running as LocalSystem that owns the elevated sensor host, so I can request sensor data via IPC without re-prompting UAC on every launch.
- **Technical Context:** New Rust binary `sidebar-monitor-svc.exe` (in a new crate `sidebar-svc` or as a `[[bin]]` in sidebar-platform). Uses the `windows-service` crate (MIT/Apache-2.0, T-32-allowed) or direct `OpenSCManager` + `CreateService` FFI. The service spawns `sidebar-monitor-host.exe` (from Story 15.1) as a child, owns the Job Object (G10), and exposes a named-pipe IPC endpoint (`\\.\pipe\sidebar-monitor`) for the non-elevated sidebar UI to request sensor frames. guardrails.md G10 (ownership) + G16 (pipe, not network).
- **Wiring:**
  - **Layer:** integration + smoke
  - **Depends-On:** [15.1]
  - **Blocks:** [16.2]
  - **Next:** 16.2
  - **Parallel-With:** —
  - **DoD:** `sidebar-monitor-svc.exe` registers as a Windows Service (via `sc create` or the installer), runs as LocalSystem, spawns the host, exposes the named pipe, + shuts down cleanly on service stop. Integration test: install the service (test env), connect to the pipe, read a sensor frame.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Decide crate structure (new `sidebar-svc` crate vs `[[bin]]` in platform). New crate is cleaner (separate signing, separate Cargo features).
  2. [ ] **Implement:** Service entry point (`ServiceMain`), host-child spawn + Job Object, named-pipe server thread, clean shutdown on `SERVICE_CONTROL_STOP`.
  3. [ ] **Validate:** Manual test on the reference machine (service install + IPC).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) The pipe server reads a request + responds with a frame. (2) Service stop kills the host child (G10).
  - **Boundary & Edge Case Test Cases (cite G10, G16):** (1) Host crash → service detects + restarts it (liveness probe). (2) Multiple sidebar UIs connecting → one frame per request (no contention).
- **Explicit Swarm Guardrails:** HITL mandatory (G11/G19) — a LocalSystem service is the highest-trust binary in the product. The `windows-service` crate addition requires a G3 license audit. Cite G10 + G16.

### STORY 16.2: Sidebar UI IPC client (named-pipe consumer)
- **User Story:** As the sidebar UI, I want to talk to the service over a named pipe instead of spawning an elevated child myself, so I never need UAC after install.
- **Technical Context:** `crates/sidebar-platform/src/ohm_supervisor.rs` — replace `launch_elevated` (ShellExecuteExW runas) with a named-pipe client connect to `\\.\pipe\sidebar-monitor`. If the service is not running (portable / non-installed mode), fall back to the current `launch_elevated` path (so the portable ZIP still works). guardrails.md G16 (named pipe is local, not network).
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [16.1]
  - **Blocks:** [16.3]
  - **Next:** 16.3
  - **Parallel-With:** —
  - **DoD:** `OhmSupervisor` tries the pipe first; on connection, uses it as the `SensorSource`. On failure (service not installed), falls back to `launch_elevated` (the Epic 15 pipe-host path). The user sees no UAC in installed mode; the portable mode still works with per-launch UAC.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** The pipe client is a new `SensorSource` impl (`PipeServiceSource`) alongside `PipeSource` (direct host) + `HttpSource` (test).
  2. [ ] **Implement:** `OhmSupervisor::new()` probes the pipe; if present, wraps it; else falls back.
  3. [ ] **Validate:** Integration test both paths.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) Pipe available → uses service. (2) Pipe absent → falls back to direct host.
  - **Boundary & Edge Case Test Cases (cite G16):** (1) Pipe connect timeout (1s) → fall back. (2) Service dies mid-session → child-liveness degrades to Basic (existing 12.8 wiring).
- **Explicit Swarm Guardrails:** Cite G16.

### STORY 16.3: Inno Setup installer (.iss) + service registration
- **User Story:** As a non-technical user, I want to download one installer, run it, accept one UAC prompt, and have sidebar + the service + the sensor host installed + the service auto-started — so I never think about elevation again.
- **Technical Context:** New `installer/sidebar.iss` (Inno Setup script). `PrivilegesRequired=admin` (service install needs elevation) + `PrivilegesRequiredOverridesAllowed=dialog` (winget compatibility per [winget-cli #254](https://github.com/microsoft/winget-cli/issues/254)). `[Run]` section: `sc create sidebar-monitor-svc binPath= ... start= auto` + `sc start`. `[UninstallRun]`: `sc stop` + `sc delete`. Signs the installer EXE via SignPath (the installer itself, not just the payload). guardrails.md G19 (HITL on the installer — it's the trust entry point) + nfr-thresholds.md T-47 (installer thresholds).
- **Wiring:**
  - **Layer:** smoke (L4)
  - **Depends-On:** [16.2]
  - **Blocks:** [16.4]
  - **Next:** 16.4
  - **Parallel-With:** —
  - **DoD:** `sidebar-setup.exe` (Inno Setup output) installs: sidebar-app.exe, sidebar-monitor-svc.exe, sidebar-monitor-host.exe, LibreHardwareMonitorLib.dll + dependencies, to `%PROGRAMFILES%\sidebar`. Registers + starts the service. Creates Start Menu shortcut. Uninstalls cleanly (stops + deletes service). Signed via SignPath.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Inno Setup is free (OSS), used by VS Code, winget `InstallerType: inno`. The winget elevation caveat ([#254](https://github.com/microsoft/winget-cli/issues/254)) is worked around via `PrivilegesRequiredOverridesAllowed=dialog` + `Scope: machine` in the winget manifest.
  2. [ ] **Implement:** `installer/sidebar.iss`. Build step compiles it via `iscc.exe` (Inno Setup Compiler, free). Output: `sidebar-setup.exe`.
  3. [ ] **Validate:** Manual install/uninstall on the reference machine.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) The .iss parses via `iscc /qp` (compile-only check). (2) The output EXE is signed.
  - **Boundary & Edge Case Test Cases (cite T-47, G19):** (1) Uninstall removes the service. (2) Reinstall upgrades without error. (3) The installer works when invoked by winget (`winget install` path).
- **Explicit Swarm Guardrails:** HITL mandatory (G11/G19) — the installer is the trust entry point. Inno Setup itself is free + OSS; the `iscc` compiler is a build-tool dependency (like `actionlint`). Cite T-47.

### STORY 16.4: winget manifest + release pipeline integration
- **User Story:** As a user, I want to `winget install sidebar` and get the installed + serviced version, not a portable ZIP.
- **Technical Context:** New `installer/winget/manifest.yaml` (winget package manifest). `.github/workflows/release.yml` updated: build stage produces sidebar-app.exe + sidebar-monitor-svc.exe + sidebar-monitor-host.exe + LHM sidecar; a new "Build installer" step runs `iscc sidebar.iss` to produce `sidebar-setup.exe`; the sign stage signs all three EXEs + the installer; the publish stage uploads the installer as the primary release artifact + submits the winget manifest PR.
- **Wiring:**
  - **Layer:** integration + smoke
  - **Depends-On:** [16.3]
  - **Blocks:** — (terminal for Epic 16)
  - **Next:** 17.1
  - **Parallel-With:** —
  - **DoD:** `release.yml` produces a signed `sidebar-setup-vX.Y.Z.exe`. The winget manifest references it as `InstallerType: inno`, `Scope: machine`. A winget PR submission step (manual trigger, HITL-gated) opens the PR to `microsoft/winget-pkgs`.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** winget manifest schema (see [winget-cli #412](https://github.com/microsoft/winget-cli/issues/412) for InstallerType values).
  2. [ ] **Implement:** Add the build + sign + publish stages to release.yml. Add the winget manifest.
  3. [ ] **Validate:** Dry-run the release workflow.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `release.yml` actionlint-clean. (2) The winget manifest validates against the schema.
  - **Boundary & Edge Case Test Cases (cite T-47, G19):** (1) SignPath failure → unsigned draft (existing fallback). (2) winget PR submission is manual (HITL).
- **Explicit Swarm Guardrails:** HITL on the winget PR submission (G19 — public artifact). Cite T-47.

### STORY 16.5: Installer upgrade + rollback testing
- **User Story:** As a user upgrading from v1.0.0 to v1.0.1, I want the installer to upgrade cleanly (stop old service, replace files, start new service) without losing my config or bandwidth data — and if the new version is broken, I want to roll back.
- **Technical Context:** Inno Setup `[Code]` section with Pascal scripting for service stop-before-overwrite + start-after. The installer MUST detect an existing installation (registry key or service presence), stop the service, overwrite files, restart the service. Config (`%APPDATA%\sidebar\`) + bandwidth DB live outside `%PROGRAMFILES%` so they survive upgrades. Rollback: Inno Setup's `CreateBackupCopy` + a documented "uninstall v1.0.1, reinstall v1.0.0" path. guardrails.md G19 (HITL on the upgrade path) + nfr-thresholds.md T-47.
- **Wiring:**
  - **Layer:** smoke (L4)
  - **Depends-On:** [16.3, 16.4]
  - **Blocks:** — (terminal for Epic 16)
  - **Next:** 17.1
  - **Parallel-With:** —
  - **DoD:** (1) Upgrade v1.0.0 → v1.0.1 preserves config + bandwidth.db + re-registers the service. (2) Uninstall + reinstall older version works (rollback path). (3) A smoke test documents the upgrade steps for the reference-machine runner.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** Inno Setup `[InstallDelete]` + `[Run]` with `Check: IsServiceInstalled('sidebar-monitor-svc')` Pascal functions.
  2. [ ] **Implement:** `installer/sidebar.iss` `[Code]` section: `function IsServiceInstalled(name: String): Boolean;` via `sc query`. Stop-before-overwrite, start-after-overwrite.
  3. [ ] **Validate:** Manual upgrade on the reference machine (install v1.0.0, add config + bandwidth data, upgrade to a dummy v1.0.1, verify preservation).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `IsServiceInstalled` detects a running service. (2) The .iss compiles with the `[Code]` section.
  - **Boundary & Edge Case Test Cases (cite T-47):** (1) First install (no prior service) → skip the stop/upgrade logic. (2) Service is installed but stopped → start-after-overwrite fires. (3) `%PROGRAMFILES%\sidebar` is locked (files in use) → installer prompts to close sidebar first.
- **Explicit Swarm Guardrails:** HITL mandatory (G19 — upgrades can brick an installation). Cite T-47.

### STORY 16.6: Portable ZIP dual-distribution
- **User Story:** As a power user who doesn't want an installer, I want a portable ZIP that works without admin rights — with the tradeoff that I'll see a UAC prompt on each Full-mode launch (no service).
- **Technical Context:** The Inno Setup installer is the primary distribution. The portable ZIP (current `release.yml` output) remains as a secondary artifact for users who can't or won't install. The portable path uses the Epic 15 pipe-host directly (Story 15.2 `PipeSource`), bypassing the service entirely — so UAC recurs per-launch (the `launch_elevated` fallback from Story 16.2). guardrails.md G16 (the portable host still emits via pipe, not HTTP).
- **Wiring:**
  - **Layer:** smoke (L4)
  - **Depends-On:** [16.2]
  - **Blocks:** —
  - **Next:** 17.1
  - **Parallel-With:** [16.5]
  - **DoD:** The release pipeline produces TWO artifacts: `sidebar-setup-vX.Y.Z.exe` (installer, primary) + `sidebar-portable-vX.Y.Z.zip` (portable, secondary). The portable ZIP contains sidebar-app.exe + sidebar-monitor-host.exe + LibreHardwareMonitorLib.dll + dependencies + a README.txt explaining the UAC-per-launch tradeoff. Both are linked from the GitHub Release body.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `release.yml` publish stage runs both `iscc` (installer) + `zip` (portable).
  2. [ ] **Implement:** Add a "Package portable ZIP" step alongside the installer step. Add `installer/PORTABLE-README.txt` explaining the tradeoff.
  3. [ ] **Validate:** Dry-run the release workflow; confirm both artifacts are produced.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `release.yml` actionlint-clean with both artifacts. (2) The portable ZIP contains all required files.
  - **Boundary & Edge Case Test Cases (cite T-47):** (1) Portable ZIP runs without admin rights (Basic mode). (2) Portable ZIP Full mode prompts UAC (expected — no service).
- **Explicit Swarm Guardrails:** Cite T-47.

---

## EPIC 17 — UX Polish + Feature Gaps (Productization Pass, 2026-07-16)
- **System Objective:** Close the remaining UX gaps that prevent sidebar from being the best software in its category: alert ack persistence, threshold UI, bandwidth CSV export, DPI-change handling, crash-recovery messaging, discoverability polish, monitor-picker dropdown. These are the items that make the product feel *complete* rather than *functional*.
- **Status:** Planned 2026-07-16. MUST land before v1.0.0 (per maintainer directive). All stories are independent + parallelizable.
- **Dependency convention:** Epic 17 is post-Epic 14 (the message-stack framework from 14.5 is used by several stories here). Stories 17.1-17.7 are parallelizable.

### STORY 17.1: Alert ack persistence (acks.toml sidecar)
- **User Story:** As a user who snoozed a CPU-temp alert, I want the snooze to survive a restart — not re-fire immediately on next launch.
- **Technical Context:** `SidebarView.alert_acks` (`gui/mod.rs:1247-1250`) is session-only. Persist to `%APPDATA%\sidebar\acks.toml` using the existing `toml` workspace dep + `atomic_write_config` pattern (no serde in sidebar-domain — hand-roll a small `AckEntry` struct in sidebar-app). guardrails.md G28.
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [12.6, 13.1]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.2, 17.3, 17.4, 17.5, 17.6, 17.7]
  - **DoD:** `acks.toml` persists active acks. On startup, load + prune expired snoozes. On ack mutation, persist. On restart, snoozes that haven't expired are restored.
- **Gentle-AI SDD Phase Checklist:**
  1. [ ] **Plan:** `AlertAck` is `Copy + Eq + Hash`; `Snoozed(i64)` carries an epoch. Define `AckEntry { category, instance, kind, variant, until }` in sidebar-app (Serialize/Deserialize via toml).
  2. [ ] **Implement:** `crates/sidebar-app/src/gui/acks_store.rs`. Load on startup, persist on mutation (debounced), prune on load.
  3. [ ] **Validate:** Unit test the round-trip + prune logic.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) A snooze with `until > now` survives restart. (2) A snooze with `until < now` is pruned on load.
  - **Boundary & Edge Case Test Cases (cite G28):** (1) Corrupt `acks.toml` → recover to empty (no crash). (2) Atomic write (temp + rename).
- **Explicit Swarm Guardrails:** No new dependency (toml is already a workspace dep). Cite G28.

### STORY 17.2: Threshold configuration UI
- **User Story:** As a user, I want to set "warn me when CPU temp > 80°C" via the settings panel — not by hand-editing config.toml.
- **Technical Context:** `ThresholdConfig` exists (`config.rs:172-190`: cpu_temp_warn/critical, gpu_temp_warn/critical). The settings panel (`settings_panel.rs:107-238`) never surfaces them. guardrails.md G28.
- **Wiring:**
  - **Layer:** ui + unit
  - **Depends-On:** [8.5]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.3, 17.4, 17.5, 17.6, 17.7]
  - **DoD:** A "Temperature alerts" section in the settings panel with two `egui::Slider`s (warn, critical) for CPU + GPU. Validated `warn < critical`. Fires `on_change` (autosave). The existing `alert_indicator::classify` consumer reads the new values with no downstream change.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) Sliders render with the current config values. (2) Changing a slider fires on_change + persists.
  - **Boundary & Edge Case Test Cases (cite G28):** (1) warn >= critical → clamped + warning shown.
- **Explicit Swarm Guardrails:** Cite G28.

### STORY 17.3: Bandwidth CSV export
- **User Story:** As a user tracking my bandwidth usage, I want to export my history to CSV.
- **Technical Context:** `sidebar-persistence::bandwidth_repo` has the data. `bandwidth_panel.rs` is pure-render. Add an Export button + `bandwidth_repo::export_csv(conn, &path)`. Cross-thread DB access via the accountant (owns the `!Send` Connection). guardrails.md G28.
- **Wiring:**
  - **Layer:** integration + ui
  - **Depends-On:** [8.4]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.2, 17.4, 17.5, 17.6, 17.7]
  - **DoD:** "Export CSV" button in the bandwidth panel. On click, a native Save dialog prompts for a path; the accountant thread exports `current_cycle` + `bandwidth_history` to CSV.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `export_csv` produces valid CSV with headers.
  - **Boundary & Edge Case Test Cases (cite G28):** (1) Empty DB → CSV with headers only.
- **Explicit Swarm Guardrails:** Cite G28.

### STORY 17.4: DPI-change handling (WM_DPICHANGED)
- **User Story:** As a user who changes Windows display scaling mid-session, I want the sidebar to re-render at the new DPI — not stay tiny or overflow.
- **Technical Context:** `WM_DPICHANGED` is not handled (`gui/mod.rs:648-711` handles WM_SETTINGCHANGE + WM_DISPLAYCHANGE but not DPI). egui 0.35 supports `ctx.set_pixels_per_point()`. guardrails.md G28.
- **Wiring:**
  - **Layer:** integration
  - **Depends-On:** [6.3]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.2, 17.3, 17.5, 17.6, 17.7]
  - **DoD:** `WM_DPICHANGED` added to the `PlatformRuntime::poll` PeekMessageW filter. On receipt, `ctx.set_pixels_per_point(new_dpi / 96.0)` + re-fire `send_dock_position`.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) The DPI message is decoded + `set_pixels_per_point` called.
  - **Boundary & Edge Case Test Cases (cite G28):** (1) DPI = 96 (100%) → no-op.
- **Explicit Swarm Guardrails:** Cite G28. Unsafe FFI per G2.

### STORY 17.5: Crash-recovery messaging (last_tier sentinel)
- **User Story:** As a user whose sidebar crashed while in Full mode, I want to know on next launch that Full mode needs re-enabling — not a surprising gray pill.
- **Technical Context:** After a crash, sidebar restarts at Basic (Job Object reaped the elevated child). Persist a `last_tier` sentinel; on restart, if it was Full, surface a one-shot message via the 14.5 stack. guardrails.md G29.
- **Wiring:**
  - **Layer:** unit + integration
  - **Depends-On:** [14.5, 12.8]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.2, 17.3, 17.4, 17.6, 17.7]
  - **DoD:** `last_tier` persisted to config (or sidecar). On launch, if `last_tier == Full` and current is Basic, push a `UserMessage::Info` "sidebar closed unexpectedly. Click the pill to re-enable Full mode." Do NOT auto-relaunch (would re-prompt UAC without intent).
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) `last_tier=Full` + current=Basic → message fires once.
  - **Boundary & Edge Case Test Cases (cite G29):** (1) Message is one-shot (dismissed on first pill click).
- **Explicit Swarm Guardrails:** Cite G29.

### STORY 17.6: Discoverability polish (hotkey + drag label + About wording)
- **User Story:** As a non-technical user, I want to discover the click-through hotkey, the drag-to-move grip, and the honest UAC cadence without reading docs.
- **Technical Context:** `gui/about.rs` (add hotkey line reading from config; fix "one-time setup" wording to be honest about recurring UAC in portable mode vs one-time in installed mode). `gui/mod.rs:1490` (change "⠿ drag to move" to "Drag here to move" + tooltip). `gui/status_pill.rs` (tooltip hint). guardrails.md G28.
- **Wiring:**
  - **Layer:** ui + unit
  - **Depends-On:** [13.4]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.2, 17.3, 17.4, 17.5, 17.7]
  - **DoD:** About dialog shows the configured hotkey + honest UAC cadence. Drag grip label is plain English. Status-pill tooltip hints at the click-through toggle.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) About renders the config hotkey. (2) Drag label is "Drag here to move".
  - **Boundary & Edge Case Test Cases (cite G28):** (1) Custom hotkey string reflected in About.
- **Explicit Swarm Guardrails:** HITL on wording (G19). Cite G28.

### STORY 17.7: Monitor-picker dropdown (wizard + settings)
- **User Story:** As a multi-monitor user, I want to pick my target monitor from a dropdown — not type a DeviceID string.
- **Technical Context:** `first_run.rs:122-138` (raw TextEdit for monitor_id). `settings_panel.rs` (no monitor picker). `monitors::enumerate()` provides the list. Replace TextEdit with `egui::ComboBox` populated from enumerate. guardrails.md G28.
- **Wiring:**
  - **Layer:** ui + unit
  - **Depends-On:** [6.6, 8.5]
  - **Blocks:** —
  - **Next:** —
  - **Parallel-With:** [17.1, 17.2, 17.3, 17.4, 17.5, 17.6]
  - **DoD:** Wizard + settings panel use a ComboBox of friendly monitor names → DeviceID mapping. Falls back to TextEdit only if enumerate fails.
- **TDD Contract & Test Cases:**
  - **Unit Test Cases (Happy Path):** (1) ComboBox renders the enumerated monitors. (2) Selecting one sets `config.dock.monitor_id`.
  - **Boundary & Edge Case Test Cases (cite G28):** (1) Enumerate fails → TextEdit fallback.
- **Explicit Swarm Guardrails:** Cite G28.

---

## APPENDIX: Story Wiring Matrix (Audit Pass 4)

Every story's `Wiring:` block in a single lookup table. The swarm consults this appendix to compute the ready set and the critical-path next pickup. See `regression-harness.md` §3 for the schema and §4 for the critical path.

| Story | Layer | Depends-On | Blocks | Next | Parallel-With |
|---|---|---|---|---|---|
| 0.1 | unit | — | [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 1.1, all] | 0.2 | — |
| 0.2 | integration | [0.1] | [11.1, 11.2, 9.2] | 0.3 | — |
| 0.3 | unit | [0.1] | [9.1] | 0.4 | [0.2] |
| 0.4 | unit | [0.1] | [all release stories] | 0.5 | [0.2, 0.3] |
| 0.5 | unit | [0.1] | [9.1, 9.2] | 0.6 | [0.2, 0.3, 0.4] |
| 0.6 | unit | [0.1] | [all lint-sensitive stories] | 1.1 | [0.2, 0.3, 0.4, 0.5] |
| 0.7 | integration | [0.1] | [6.5, 11.2] | 1.1 | [0.3, 0.4, 0.5, 0.6] |
| 1.1 | unit | [0.1, 0.6] | [1.2, 1.3, 1.4, 1.5, 1.6, 2.1, 3.x, 5.1, 8.x] | 1.2 | [0.2..0.6 leftover, 11.1] |
| 1.2 | unit | [1.1] | [8.8 (alert UI)] | 1.3 | — |
| 1.3 | unit | [1.1] | [8.3 (metric row)] | 1.4 | [1.2] |
| 1.4 | unit | [1.1] | [5.2 (rollover), 8.5 (settings)] | 1.5 | [1.2, 1.3] |
| 1.5 | unit | [1.1, 1.4] | [8.5, 8.9, 8.10] | 1.6 | [1.2, 1.3, 1.4] |
| 1.6 | unit | [1.1] | [8.7 (sparkline), 8.9 (top-N UI)] | 2.1 | [1.2..1.5] |
| 2.1 | unit | [1.1] | [2.2, 2.3, 3.x, 7.1, 7.2] | 2.2 | — |
| 2.2 | unit | [2.1] | [2.3, 3.x] | 2.3 | — |
| 2.3 | unit | [2.1, 2.2] | [3.x, 7.1] | 3.1 ‖ 4.1 | — |
| 3.1 | unit + integration | [2.3] | [7.1, 8.1] | 3.2 | [3.2, 3.3, 3.4, 3.5, 3.6, 4.1, 4.2, 4.3] |
| 3.2 | unit + integration | [2.3] | [3.2b, 7.1] | 3.2b | [3.1, 3.3, 3.4, 3.5, 3.6, 4.x] |
| 3.2b | unit + bench | [3.2, 10.1] | [8.x GPU panels] | 3.3 | [3.3, 3.4, 3.5, 3.6] |
| 3.3 | unit + integration | [2.3] | [7.1] | 3.4 | [3.4, 3.5, 3.6] |
| 3.4 | unit + integration | [2.3] | [7.1] | 3.5 | [3.5, 3.6] |
| 3.5 | unit + integration | [2.3] | [5.1, 5.2, 8.4 (bandwidth panel)] | 3.6 | [3.6] |
| 3.6 | unit + integration | [2.3] | [6.4, 7.3] | 5.1 (if Epic 4 done) else 4.1 | — |
| 4.1 | unit + integration | [2.3] | [4.2, 4.3, 5.2] | 4.2 | [3.x, 4.2 after, 4.3 after] |
| 4.2 | unit + integration | [4.1] | [5.2] | 4.3 | — |
| 4.3 | unit + integration | [4.1] | [5.2] | 5.1 (if 3.5 done) | — |
| 5.1 | unit | [1.4, 3.5] | [5.2] | 5.2 | — |
| 5.2 | unit + integration | [4.2, 4.3, 5.1] | [5.3, 8.4] | 5.3 | — |
| 5.3 | unit | [5.2] | [8.4] | 6.1 | — |
| 6.1 | ui + smoke | [0.4] | [8.1] | 6.2 | [6.2, 6.3, 6.6] |
| 6.2 | ui + smoke | [6.1] | [8.1] | 6.3 | [6.3, 6.6] |
| 6.3 | ui + smoke | [6.1] | [8.1] | 6.4 | [6.6] |
| 6.4 | integration + smoke | [3.6, 6.1, 6.2, 6.3] | [7.3, 7.4] | 6.5 | — |
| 6.5 | integration | [0.5] | [9.2] | 6.6 | — |
| 6.6 | integration + ui | [6.1, 1.5] | [8.5 (settings)] | 7.1 | — |
| 7.1 | unit + integration | [2.3, 3.1..3.6] | [7.2] | 7.2 | — |
| 7.2 | unit + integration + bench | [7.1] | [8.1, 10.1] | 7.3 | — |
| 7.3 | integration | [3.6, 6.4, 7.2] | [8.2] | 7.4 | — |
| 7.4 | unit + integration | [7.2, 6.4] | [8.1, 8.2] | 7.5 | — |
| 7.5 | unit + integration | [7.2, 7.4, 5.2, 6.4] | [9.2] | 8.1 | — |
| 8.1 | ui | [6.1, 6.2, 6.3, 7.2, 7.4, 11.3] | [8.2..8.10] | 8.2 | — |
| 8.1 note | | 11.3 provides the snapshot harness; 8.1 adds the first real snapshot on top. 11.3 itself depends only on [0.1, 11.1] (self-contained bootstrap snapshot). | | | |
| 8.2 | ui | [8.1, 7.3] | — | 8.3 | — |
| 8.3 | ui | [8.1, 1.3] | — | 8.4 | [8.6, 8.7, 8.8, 8.9] |
| 8.4 | ui | [8.1, 5.3] | — | 8.5 | [8.6, 8.7, 8.8, 8.9] |
| 8.5 | ui | [8.1, 1.5, 6.6] | — | 8.6 | [8.6, 8.7, 8.8, 8.9] |
| 8.6 | ui | [8.1, 1.5, 6.6] | — | 8.7 | [8.7, 8.8, 8.9] |
| 8.7 | ui | [8.1, 1.6] | — | 8.8 | [8.8, 8.9] |
| 8.8 | ui | [8.1, 1.2] | — | 8.9 | [8.9] |
| 8.9 | ui | [8.1, 1.5, 1.6] | — | 8.10 | — |
| 8.10 | ui | [8.1, 1.5, 6.6] | — | 9.1 | — |
| 9.1 | integration | [0.3, 0.5, 6.5] | [9.2] | 9.2 | — |
| 9.2 | integration | [9.1, 7.5, 10.1] | [9.3] | 9.3 | — |
| 9.3 | integration | [9.2] | — | 10.1 | (optional v1.1) |
| 10.1 | bench + integration | [0.2, 7.2, 3.1, 3.2, 3.3, 3.4, 3.5, 3.6] | [9.2, 3.2b] | 10.2 | — |
| 10.2 | integration + smoke | [10.1] | [9.2 release gate] | 11.x | — |
| 11.1 | unit + integration + bench | [0.1, 0.2] | [11.2, 11.3, 11.4, all stories using Layer field] | 11.2 | [0.3..0.6, 1.1] |
| 11.2 | integration + bench | [0.2, 11.1] | [every code story — the gate] | 11.3 | [11.4] |
| 11.3 | ui | [0.1, 11.1] | [8.1, 8.2..8.10] | 8.1 | [11.4] |
| 11.4 | unit + integration | [0.2, 11.2] | — (terminal enabler) | — | [11.3] |
| 12.1 | ui | [8.1] | — (optional polish) | 12.2 | [12.2, 12.7] |
| 12.2 | ui + unit | [8.7, 11.3, 11.4] | — | 12.3 | [12.4] |
| 12.3 | integration + smoke | [6.6, 8.5, 11.4] | — | 12.4 | — |
| 12.4 | ui + config | [8.5, 8.6] | — | 12.5 | — |
| 12.5 | integration | [3.3, 3.5, 8.4] | — | 12.6 | — |
| 12.6 | ui + unit | [1.2, 8.8, 11.4] | — | 12.7 | — |
| 12.7 | ui + unit | [1.3, 12.1] | — | 12.8 | (optional/deferred) |
| 12.8 | integration + smoke | [6.4, 7.2, 7.3, 7.4, 7.5, 8.2, 8.4, 11.4] | — (closure gate) | — | — |
| 13.1 | unit + integration | [1.5, 8.5] | [13.5] | 13.2 | [13.2, 13.3, 13.4] |
| 13.2 | unit + integration | [4.1, 4.2] | [13.5] | 13.3 | [13.1, 13.3, 13.4] |
| 13.3 | unit + integration | [0.1, 6.4] | [13.5] | 13.4 | [13.1, 13.2, 13.4] |
| 13.4 | ui + unit | [8.5, 8.6] | [13.5] | 13.5 | [13.1, 13.2, 13.3] |
| 13.5 | smoke + integration | [13.1, 13.2, 13.3, 13.4, 10.2] | [14.1] | 14.1 | — |
| 14.1 | unit + integration | [12.8] | [14.5] | 14.2 | [14.2, 14.3, 14.4, 14.5] |
| 14.2 | integration | [8.10, 7.2, 7.5] | [14.5] | 14.3 | [14.1, 14.3, 14.4, 14.5] |
| 14.3 | ui + unit | [8.3] | [14.5] | 14.4 | [14.1, 14.2, 14.4, 14.5] |
| 14.4 | unit + integration | [13.1, 13.2] | [14.5] | 14.5 | [14.1, 14.2, 14.3, 14.5] |
| 14.5 | ui + unit | [14.1, 14.2, 14.3, 14.4] | — (terminal) | 15.1 | — |
| 15.1 | integration + smoke | [14.1] | [15.2] | 15.2 | — |
| 15.2 | unit + integration | [15.1] | [15.3] | 15.3 | — |
| 15.3 | unit | [15.2] | — (terminal) | 16.1 | — |
| 16.1 | integration + smoke | [15.1] | [16.2] | 16.2 | — |
| 16.2 | unit + integration | [16.1] | [16.3] | 16.3 | — |
| 16.3 | smoke (L4) | [16.2] | [16.4] | 16.4 | — |
| 16.4 | integration + smoke | [16.3] | [16.5] | 16.5 | — |
| 16.5 | smoke (L4) | [16.3, 16.4] | — (terminal) | 17.1 | [16.6] |
| 16.6 | smoke (L4) | [16.2] | — | 17.1 | [16.5] |
| 17.1 | unit + integration | [12.6, 13.1] | — | — | [17.2, 17.3, 17.4, 17.5, 17.6, 17.7] |
| 17.2 | ui + unit | [8.5] | — | — | [17.1, 17.3, 17.4, 17.5, 17.6, 17.7] |
| 17.3 | integration + ui | [8.4] | — | — | [17.1, 17.2, 17.4, 17.5, 17.6, 17.7] |
| 17.4 | integration | [6.3] | — | — | [17.1, 17.2, 17.3, 17.5, 17.6, 17.7] |
| 17.5 | unit + integration | [14.5, 12.8] | — | — | [17.1, 17.2, 17.3, 17.4, 17.6, 17.7] |
| 17.6 | ui + unit | [13.4] | — | — | [17.1, 17.2, 17.3, 17.4, 17.5, 17.7] |
| 17.7 | ui + unit | [6.6, 8.5] | — | — | [17.1, 17.2, 17.3, 17.4, 17.5, 17.6] |

### Reading the matrix

- **Layer** declares which test layer(s) the story's own tests live at. Determines which CI jobs MUST pass for the story's PR.
- **Depends-On** lists story IDs that MUST be `merged` in `PROGRESS.md` before this story can start.
- **Blocks** lists stories that cannot start until this one merges.
- **Next** is the deterministic critical-path pickup after this story merges (when the swarm is single-threaded). Where multiple stories are eligible, the lowest `(Epic, Story)` number wins.
- **Parallel-With** lists stories that may run concurrently (multi-agent swarm) once mutual `Depends-On` constraints are met.

### Critical path (single-threaded swarm)

```
0.1 → 0.2 → 0.3 → 0.4 → 0.5 → 0.6
  → 1.1 → 1.2 → 1.3 → 1.4 → 1.5 → 1.6
  → 2.1 → 2.2 → 2.3
  → 3.1 → 3.2 → 3.2b → 3.3 → 3.4 → 3.5 → 3.6
  → 4.1 → 4.2 → 4.3
  → 5.1 → 5.2 → 5.3
  → 6.1 → 6.2 → 6.3 → 6.4 → 6.5 → 6.6
  → 7.1 → 7.2 → 7.3 → 7.4 → 7.5
  → 8.1 → 8.2 → 8.3 → 8.4 → 8.5 → 8.6 → 8.7 → 8.8 → 8.9 → 8.10
  → 9.1 → 9.2 → 9.3
  → 10.1 → 10.2
  → 11.4 → 12.1 → 12.2 → 12.3 → 12.4 → 12.5 → 12.6 → 12.7 → 12.8 (post-release parity/closure; optional/deferred rows may be skipped)
  → 13.1 → 13.2 → 13.3 → 13.4 → 13.5 (hardening for non-technical users; SHOULD merge before v1.0.0 tag)
  → 14.1 → 14.2 → 14.3 → 14.4 → 14.5 (silent-failure elimination; MUST merge before v1.0.0)
  → 15.1 → 15.2 → 15.3 (LHM library host — eliminates HTTP auto-start regression; MUST merge before v1.0.0)
  → 16.1 → 16.2 → 16.3 → 16.4 → 16.5 → 16.6 (Windows Service + Inno Setup installer + upgrade/rollback + portable ZIP; MUST merge before v1.0.0)
  → 17.1 → 17.2 → 17.3 → 17.4 → 17.5 → 17.6 → 17.7 (UX polish + feature gaps; MUST merge before v1.0.0)
```

Length: 48 stories on the current delivery critical path (out of 60 current
rows, including INT), plus the 8-story post-release parity/closure extension
+ the 5-story Epic 13 hardening extension (73 total). The other current
stories are parallel-burst-eligible per §5 of `regression-harness.md`.

### Parallel-burst optimization (multi-agent swarm, max 3 concurrent per G17)

| Burst window | Eligible stories |
|---|---|
| After 2.3 merges | {3.1, 3.2, 3.3, 3.4, 3.5, 3.6} ‖ {4.1, 4.2, 4.3} |
| After 5.3 merges | {6.1, 6.2, 6.3, 6.6} (then 6.4, 6.5) |
| After 8.1 merges | {8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9} |
| After 0.2 merges | {11.1} → {11.2, 11.3, 11.4} |

### Definition of Done (per story, per G25)

A story is `merged` iff ALL of:
1. Story's own new tests pass at their declared Layer(s).
2. ALL tests from Stories 1..N-1 still pass (full L0+L1+L2+L3 matrix).
3. Coverage delta for touched crate(s) ≥ 0 (T-42, G26).
4. `cargo clippy --workspace -- -D warnings` clean.
5. `cargo fmt --check` clean.
6. `cargo deny check bans licenses advisories sources` clean.
7. `cargo audit` clean (zero unmuted advisories).
8. HITL gates per G11/G19 cleared (`requires-hitl-*` labels removed).

---

**END OF EPICS & STORIES (AUDIT PASS 4 + current-state parity extension + Epic 13-17 productization).** 18
Epics, 96 Stories (60 current delivery rows including INT + 8 Epic 12
parity/closure + 5 Epic 13 hardening + 5 Epic 14 silent-failure + 3 Epic 15
LHM-host + 6 Epic 16 service/installer + 7 Epic 17 UX-polish). Companion:
`README.md`, `guardrails.md` (G1–G29), `nfr-thresholds.md` (T-1–T-48),
`tdd-fixtures.md` (F1–F15), `regression-harness.md`, `PROGRESS.md`,
`docs/dev-env.md`. Source: `docs/PRD.md`, `docs/architecture.md`,
