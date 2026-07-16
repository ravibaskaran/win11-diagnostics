# Cross-Cutting Guardrails — sidebar-v1 (Audited, Hardened)

**Applies to every story in `epics-and-stories.md`.** Ingest as part of the swarm's system prompt before executing any story. Two-pass audit added G13–G22 and hardened G1–G12.

---

## G1 — TDD Discipline (RED first, mandatory)

- The failing test (RED phase) must exist in the commit history BEFORE the implementation commit. The orchestrator verifies this via `git log --diff-filter=A -- '*test*'` ordering on the story's PR.
- "Behavioral acceptance criteria programmatically defined" means: a `#[test]` that exercises the public API and asserts a specific output. Aspirational test stubs (`assert!(true);`) do NOT satisfy this rule.
- Every public function in `sidebar-domain` and `sidebar-sensor` has at least one unit test before it has a body.
- **Audit hardening:** Tests MUST cite a fixture pattern from `tdd-fixtures.md#F-*` and (where numeric) a threshold from `nfr-thresholds.md#T-*` in a doc-comment. Example:
  ```rust
  /// Verifies NFR-8 (PRD §6) + AD-13 (architecture §5.4). Threshold T-30. Fixture F1.
  #[test] fn format_hz_ghz() { ... }
  ```

## G2 — `unsafe` Policy

- No `unsafe` block without a `// SAFETY:` comment explaining the invariant being upheld.
- `unsafe` blocks in adapter/platform crates (Win32 FFI, `ShellExecuteW`, `DwmSetWindowAttribute`, `SetWindowDisplayAffinity`, `GetIfTable2`, etc.) require HITL sign-off per G19. The reviewer must confirm the invariant holds on Win11 24H2 and 25H2.
- `unsafe` in pure-domain crates (`sidebar-domain`, `sidebar-sensor`) is FORBIDDEN. If you believe one is needed, escalate — it indicates an architecture leak.
- **Audit hardening:** Every `unsafe` test must use fixture `F11` (unsafe FFI test with SAFETY contract). The CI clippy gate enables `clippy::undocumented_unsafe_blocks = "deny"`.

## G3 — Dependency & License Audit

- No new dependencies without HITL license audit per G19.
- **Allowed licenses (T-32):** MIT, Apache-2.0, MPL-2.0, BSD-3-Clause, ISC, Zlib, Unicode-DFS-2016, CC0-1.0.
- **Forbidden:** GPL/AGPL/LGPL/SSPL, proprietary, "unlicensed", unknown.
- Every new `Cargo.toml` entry must reference the license in a comment: `sysinfo = "0.39.3"  # MIT`.
- Bundled binary (`LibreHardwareMonitor.exe`) is MPL-2.0.
- **Audit hardening:** Enforced by `cargo deny check bans licenses` in CI (Story 0.3). Manual audit is advisory; the tool is the gate.

## G4 — Chained-PR Strategy: Single-Trunk

- Every story is ONE PR merged to `main`. No long-lived feature branches.
- Branch naming: `story-X.Y-short-slug` (e.g. `story-1.3-format-module`).
- One commit per logical TDD step (RED test → impl → GREEN → refactor), all on the same PR.
- Rebase before merge; squash-merge to `main` with the story ID in the commit title (`Story 1.3: NFR-8 format module`).

## G5 — Review Budget: 3 Passes

- Each PR gets at most **3 review rounds** before escalating to the orchestrator.
- Round count resets on substantive author change (not on a `suggestion` applied via the review UI).
- If a PR exhausts 3 rounds without convergence, the orchestrator intervenes: either splits the story, reassigns, or marks it blocked on an open question.

## G6 — Platform Gating

- Integration tests touching Win32 APIs MUST be gated `#[cfg(target_os = "windows")]`.
- Hardware-dependent tests (NVML present, OHM installed, battery present) MUST be marked `#[ignore]` with a comment naming the prerequisite. They run via `cargo test --ignored` on a suitably-equipped machine.
- CI runs the full ungated suite on `windows-latest`. Linux/macOS runners are not required for v1 but unit tests in `sidebar-domain`/`sidebar-sensor` MUST compile and pass on any platform.

## G7 — Convergence Rule

- Every story's test doc-comments MUST cite both `docs/PRD.md §<section>` AND `docs/architecture.md §<section>` for the requirement being verified.
- This makes single-product drift detectable in code review: if a test cites only the PRD or only the architecture, the reviewer flags it.

## G8 — NFR-1 Lightweight Gate (design-time + CI-time)

- Every `SensorDescriptor` MUST carry a `CostClass`. The compiler enforces this (no `Default`).
- `CostClass::Lightweight` requires profiling evidence — a comment pointing to a `poll_cost` bench result showing <0.1% CPU avg.
- `CostClass::Watch` requires the same evidence in the 0.1–0.5% band, AND is feature-flagged so it can be disabled at build time if a later regression breaches the threshold.
- `CostClass::Heavy` and `CostClass::Deferred` descriptors MAY be defined but MUST NOT appear in the v1 provider registry (the `classify_for_v1` gate rejects them with `tracing::warn!`).
- CI bench (`poll_cost`) is a hard gate — a regression over `T-1 (0.5%)` fails the build.

## G9 — Counter-vs-Gauge Semantics (network + bandwidth)

- Adapters emit RAW cumulative counters for `MetricKind::{NetRxBytes, NetTxBytes, NetRxPackets, NetTxPackets, NetRxErrors, NetTxErrors}`. Adapters MUST NOT delta.
- Delta-and-divide happens downstream (in the `BandwidthAccountant` and the live-throughput domain step).
- Violating this rule silently breaks the bandwidth-accounting feature — flagged as a critical review issue.

## G10 — OHM Subprocess Ownership Rule

- `OhmSupervisor` tracks whether IT launched the bundled OHM or whether OHM was already running (user-started).
- On sidebar shutdown, the supervisor kills OHM **only if sidebar launched it**. Killing a user-started OHM is FORBIDDEN.
- **Audit hardening:** The supervisor MUST additionally place any sidebar-launched OHM child into a Win32 Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. If the sidebar host crashes, the kernel reaps the OHM child — no orphans.

## G11 — Marquee-Feature HITL Story Gates

The following stories require mandatory human sign-off before the swarm may mark them complete. (See G19 for the per-action matrix that supplements this.)

| Story | Why HITL |
|---|---|
| 0.3 | `deny.toml` license/advisory policy + initial supply-chain decision. |
| 0.5 | LICENSE choice (MPL-2.0 vs MIT) — affects downstream compatibility. |
| 1.4 | Marquee feature; billing edge-case contract is policy. |
| 3.2b | OQ-2 ship/defer decision based on bench result. |
| 6.1 | R4 — thin Win11 transparency precedent; manual smoke. |
| 6.2 | Multi-monitor AppBar smoke. |
| 6.4 | UAC + process-ownership + Job Object logic. |
| 6.5 | OHM binary pinning — external upstream version choice (R7). |
| 6.6 | Capture cloak is a streamer-privacy feature; visual review. |
| 7.3 | "No UAC on default first launch" success metric. |
| 7.4 | Event-channel contract (G23) — architectural keystone. |
| 7.5 | Shutdown T-39 timeout hierarchy — process-termination policy. |
| 8.4 | Marquee feature; visual review. |
| 8.5 | "No retroactive re-split" rule correctness. |
| 8.10 | First-run wizard UX — first impression; visual review. |
| 9.1 | SignPath external trust submission. |
| 9.2 | External publishing; release-approver env. |
| 10.1 | 0.5% threshold + reference-hardware policy. |
| 11.1 | Regression harness architecture — every downstream test depends on it. |
| 11.4 | Progress-tracker automation — the swarm reads this file to pick the next story; trust boundary. |

## G12 — Skill/Orchestrator Convergence

- This backlog converges with `docs/PRD.md` and `docs/architecture.md`. If the swarm discovers that a story cannot be implemented as specified (e.g. a crate version is wrong, an API doesn't exist), it MUST NOT silently pick an alternative.
- The swarm MUST surface the discrepancy to the orchestrator with: (a) the specific PRD/architecture § violated, (b) the discovered fact with citation, (c) proposed options.

---

## G13 — Cold-Start Idempotency (Audit Pass 1)

- Every `init()`, `migrate()`, `register()`, `open()`, `connect()` style function MUST be safe to call multiple times in succession.
- Each such function MUST have an idempotency test using fixture `F6`.
- For SQLite: `CREATE TABLE IF NOT EXISTS` + `PRAGMA user_version` guard.
- For tokio runtime: `tokio::runtime::Handle::try_current()` before constructing a new runtime.
- The current LHM bridge is HTTP-only and requires no COM initialization. Any
  legacy WMI/COM fixture is historical and must not be added to runtime code.

## G14 — Resource Bounds (Audit Pass 2)

Every long-running construct MUST have an explicit bound cited from `nfr-thresholds.md`:

| Resource | Bound | Citation |
|---|---|---|
| Broadcast channel | capacity 8, overflow = drop oldest + warn | T-14 |
| SQLite busy retry | 5 attempts, ≤310ms total | T-12 |
| Tokio runtime | 2 worker threads | T-18 |
| Tokio shutdown grace | 3000 ms | T-19 |
| Bandwidth flush debounce | 60 s | T-15 |
| History retention | current + previous (keep=1) | T-16 |
| WAL checkpoint | SQLite default (autocheckpoint=1000) | T-17 |
| Process top-N | 1–50, default 5 | T-21 |
| Sparkline window | 10–600, default 60 | T-22 |

The swarm MUST NOT introduce unbounded channels, unbounded retries, or unbounded collections in any code path.

## G15 — Panic Safety (Audit Pass 2)

- The poller MUST wrap each `provider.read_all()` call in `std::panic::catch_unwind`. A panicking adapter MUST NOT poison the tokio runtime or the broadcast channel. (Fixture F10 verifies.)
- `Mutex`/`RwLock` poison MUST be handled: production code calls `.lock().unwrap_or_else(|e| e.into_inner())` to recover the inner guard and logs `error!("lock poisoned, recovering: {e}")`. NEVER propagate `PoisonError` to runtime shutdown.
- All `f64` arithmetic in the domain layer MUST defend against `NaN`/`±Inf` (T-20). `format_*` renders `"--"` for non-finite.
- The BandwidthAccountant's flush task MUST catch its own errors and continue — a flush failure MUST NOT terminate the accountant (data preserved in memory for next attempt).

## G16 — Network Egress Allowlist (Audit Pass 2)

- **Runtime application (sidebar.exe on a user's machine):** ZERO *remote*
  network egress. No telemetry, no auto-update check, no "phone home". The
  only socket exception is the bundled LHM HTTP bridge on literal loopback
  (`http://127.0.0.0/8` or `http://[::1]`); URLs are validated before transport
  and redirects are disabled. The app reads `GetIfTable2` for NIC counters and
  `/data.json` for local LHM sensors.
- **CI environment:** Allowed egress limited to:
  - `static.crates.io`, `crates.io`, `index.crates.io`, `static.rust-lang.org` (cargo)
  - `github.com`, `*.githubusercontent.com`, `objects.githubusercontent.com` (actions, SignPath, winget, LHM upstream fetch — approved 2026-07-13)
  - `signpath.io`, `*.signpath.org`, `app.signpath.io`, `api.signpath.io` (signing)
  - `repo.rustsec.org` (cargo-audit DB)
- **CI LHM fetch job (G16 egress approved 2026-07-13).** The `lhm-fetch`
  job in `.github/workflows/ci.yml` downloads the pinned
  `LibreHardwareMonitor.zip` from
  `github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v0.9.6/...`
  into a runner-temp staging directory and verifies the SHA-256 against
  `resources/ohm.sha256`. It does NOT mutate `resources/` (the committed
  binary remains the source of truth). The approval is bounded to this
  exact URL + the matching license URL on `raw.githubusercontent.com`.
- The swarm MUST NOT add any other network dependency to either runtime or CI without HITL approval (G19).
- A runtime-network-egress integration test (Story 10.1 extended) MUST verify
  sidebar.exe opens no non-loopback sockets during a 60-second smoke run
  (verified via `netstat` snapshot diff on Windows); the expected LHM loopback
  connection is allowlisted.

## G17 — Generation-Loop Bounds (Audit Pass 2)

The swarm MUST NOT generate unbounded artifacts. Hard caps:

| Artifact | Cap | Action if exceeded |
|---|---|---|
| Crates in the workspace | 12 (10 libs + 1 bin + 1 bench harness) | STOP; escalate to orchestrator |
| Dependencies per crate | 30 direct | STOP; escalate |
| Total workspace transitive deps | 400 | WARN; review at standup |
| Story PR size (lines changed) | 800 | Split story |
| Test file size | 500 lines | Split into modules |
| `unsafe` blocks per crate | 20 | Architectural review |
| Commits per story PR | 15 | Squash TDD micro-commits |
| Generated stub files | 0 | NEVER ship empty stubs — every file has at least one test |

If the swarm hits any cap, it MUST stop and surface to the orchestrator rather than generating beyond it.

## G18 — Supply Chain Automation (Audit Pass 2)

- `deny.toml` (Story 0.3) is the authoritative supply-chain policy. It MUST be in CI before any code story merges.
- `cargo deny check bans licenses advisories sources` runs on every PR.
- `cargo audit` runs on every PR; zero unmuted RUSTSEC advisories (T-33).
- `Cargo.lock` is committed (this is a binary workspace). A diff in `Cargo.lock` requires the swarm to explain the change in the PR description.
- Reproducible builds: `cargo build --locked --release` MUST produce a byte-identical binary on the same runner (verified by hash in Story 9.2's release dry-run).

## G19 — HITL Action-Permission Matrix (Audit Pass 2)

Beyond the per-story matrix in G11, these specific ACTIONS require human approval before the swarm executes them. The orchestrator MUST surface each to the user; auto-execution is FORBIDDEN.

| Action | Required approval | Reason |
|---|---|---|
| Adding a new dependency (any `Cargo.toml` `[dependencies]` addition) | License audit sign-off | G3/T-32 |
| Adding any `unsafe` block | Reviewer SAFETY-contract sign-off | G2 |
| Adding a network egress endpoint (runtime or CI) | Privacy/trust review | G16 |
| Modifying `Cargo.lock` with a major-version bump | Architect sign-off | Reproducibility |
| Modifying `deny.toml` allowlist | Architect sign-off | Supply-chain policy |
| Modifying `rust-toolchain.toml` | Architect sign-off | MSRV sensitivity |
| Calling `ShellExecuteW` with "runas" (any code path) | UAC-flow review | G10 |
| Modifying `signpath/code-signing-policy.md` | SignPath-recipient review | External trust |
| Submitting a SignPath application | Project-owner approval | External trust |
| Submitting a winget PR | Project-owner approval | Public artifact |
| Tagging a release (`git tag v*`) | Release-approver environment | Publishing |
| Modifying NFR thresholds in `nfr-thresholds.md` | Architect + SRE sign-off | All downstream tests depend on these |
| Disabling a CI gate (clippy/fmt/bench/deny) | Architect sign-off | Each gate exists because of a documented risk |
| Deleting any file under `docs/` | Project-owner approval | Audit trail |
| Pinning/bumping the bundled OHM binary version (R7) | Architect sign-off | External upstream dependency; namespace compatibility |
| Registering a global hotkey (`RegisterHotKey`) | Privacy review — hotkeys are system-wide; conflicts must surface | T-34 |
| Modifying the first-run wizard flow | UX review | First impression; T-37 |
| Modifying the shutdown timeout hierarchy (T-39) | SRE sign-off | Process-termination policy |
| Modifying the `Event` enum or event-channel contract (G23) | Architect sign-off | All UI-affecting notifications depend on it |
| Modifying the test layer model (L0–L4) or per-layer budgets (T-40) | Architect + SRE sign-off | All CI depends on this |
| Modifying the regression contract (8 DoD points) | Architect sign-off | Every PR is gated by this |
| Accepting new UI snapshots (`cargo insta accept`) | UX review | Visual regression baseline |
| Modifying `PROGRESS.md` schema or auto-update logic | Architect sign-off | The swarm reads this to pick stories; tampering = silent story-skipping |
| Disabling a coverage gate or lowering a coverage target (T-42) | Architect sign-off | Hides regressions |

The orchestrator enforces these via PR labels: each HITL action gets a `requires-hitl-<category>` label that blocks merge until a human removes it.

## G20 — Convergence with Source Docs (Audit Pass 1)

- Every story in `epics-and-stories.md` cites `docs/PRD.md §<section>` AND `docs/architecture.md §<section>`.
- If a story cites a section that has been amended (e.g. PRD §3 Tier 4 has a v2 marker), the swarm MUST acknowledge the marker in its PR description.
- Drift between `epics-and-stories.md` and `docs/PRD.md`/`docs/architecture.md` is a BLOCKING review issue.

## G21 — SQLite Operational Discipline (Audit Pass 1)

- All SQLite access goes through `sidebar-persistence`. NO other crate opens a `Connection` directly.
- Every write is inside a transaction (`BEGIN ... COMMIT` or `conn.execute_batch` with implicit txn).
- Every multi-statement migration is inside a single transaction (rollback on any failure).
- `PRAGMA journal_mode = WAL` is set ONCE at `Connection::open` time, not per-query.
- `PRAGMA foreign_keys = ON` is set at open time (defensive; v1 schema has no FKs but v1.1 might).
- Tests use `tempfile::TempDir` (fixture F1) — NEVER the real `%APPDATA%\sidebar\bandwidth.db`.

## G22 — Test Runtime Budget (Audit Pass 2)

- Full `cargo test --workspace` (excluding `#[ignore]`) MUST complete in ≤ 120 seconds on CI.
- `cargo bench --bench poll_cost` MUST complete in ≤ 600 seconds (5-min windows per T-1).
- `cargo clippy --workspace -- -D warnings` MUST complete in ≤ 180 seconds.
- If a story's tests push any budget over, the swarm MUST split the story or optimize — never silently exceed.
- `#[ignore]`'d integration tests (NVML, OHM, battery) are NOT counted toward the 120s budget but MUST each complete in ≤ 30 s when run individually.

---

## G23 — Event Channel Discipline (Audit Pass 3)

- The application has TWO broadcast channels, NOT one:
  1. **Readings broadcast** — `broadcast::Sender<Vec<Reading>>`, capacity T-14 (8). Carries per-tick sensor data. Consumers: GUI AppState, BandwidthAccountant.
  2. **Event broadcast** — `broadcast::Sender<Event>` (fixture F12), capacity T-14 (8). Carries UI-affecting notifications: `TierChanged`, `ThemeChanged`, `MonitorChanged`, `Shutdown`. Consumers: GUI, status pill, hotkey handler.
- Mixing the two is FORBIDDEN. Sensor data on the Event channel (or vice versa) is a critical review issue.
- Tier-change events are coalesced per T-38 (500ms window, latest wins) to avoid UI thrash when OHM flaps.
- Shutdown is broadcast on the Event channel as `Event::Shutdown`; all subscribers drain + cleanup within their T-39 phase.

## G24 — First-Run vs Steady-State Code Paths (Audit Pass 3)

- Every component that reads `config.toml` MUST handle the "first-run" case: config file does not exist.
- The first-run wizard (Story 8.10) runs BEFORE any other component starts polling. Specifically:
  1. Launch → detect absence of `%APPDATA%\sidebar\config.toml` → show wizard.
  2. Wizard writes `config.toml` with `first_run_complete = true` on completion OR skip.
  3. Only then does the poller/accountant/OhmSupervisor start.
- Components MUST NOT silently default-and-proceed if their config section is missing — that hides wizard bugs. Missing sections log `warn!` and use the documented default; the wizard is responsible for collecting the user's preferences upfront.
- The wizard MUST NOT block the GUI thread synchronously — it runs as a modal egui panel that defers the rest of the startup sequence until dismissed.

---

## G25 — Cumulative Regression Gate (Audit Pass 4)

- **Every PR runs the FULL test matrix (L0+L1+L2+L3), not just the touched crate.** There is no "only my crate" mode in CI.
- A story's PR is NOT mergeable until all 8 DoD points in `regression-harness.md` §2 are satisfied.
- "Prior stories' tests still pass" is the regression contract. A green PR for Story N proves Stories 1..N-1 still work.
- The swarm MUST run `cargo test --workspace --all-targets` locally before opening a PR. If a prior test fails, the swarm MUST NOT open the PR — it surfaces a regression-blocker to the orchestrator instead.
- CI artifacts: `regression-report.md` + coverage XML are uploaded on every PR for HITL review.

## G26 — Coverage Non-Regression (Audit Pass 4)

- For every PR touching crate(s) C, the line coverage of C MUST NOT decrease (T-42).
- Coverage measured by `cargo llvm-cov --workspace --lcov --output-path coverage/lcov.info` (per T-43; NOT `cargo tarpaulin` which is Linux-only) vs `main` baseline.
- An intentional decrease requires PR-description justification + HITL sign-off per G19 (label `requires-hitl-coverage`).
- Coverage targets (T-42): `sidebar-domain`/`sidebar-sensor` ≥ 80%; adapter/platform ≥ 60%; `sidebar-app` ≥ 40%.
- Coverage is tracked per-story in `PROGRESS.md` (Story 11.4) so the orchestrator can see drift over time.

## G27 — Story Wiring Discipline (Audit Pass 4)

- Every story MUST carry a `Wiring:` block (defined in `regression-harness.md` §3) with: `Layer`, `Depends-On`, `Blocks`, `Next`, `Parallel-With`, `DoD`.
- The swarm MUST NOT begin a story whose `Depends-On` entries are not all `merged` in `PROGRESS.md`.
- The swarm MUST update `PROGRESS.md` on merge (automated via Story 11.4's CI job).
- If a story's `Depends-On` cannot be satisfied (e.g. a dependency is `blocked-on-hitl` for >72h), the swarm MUST surface the blockage to the orchestrator — it MUST NOT skip ahead.
- "Correctness and completeness" (the user's pass-4 mandate) means: every story's Wiring block is the single source of truth for its place in the sequence. Drift between the Wiring blocks and the prose narrative is a BLOCKING review issue.

## G28 — Non-Technical-User Hardening (Audit Pass 5)

v1.0.0+ targets users with little technical knowledge. The following
hardening invariants are mandatory for the v1.0.0 tag (Stories 13.1–13.5):

- **Atomic config writes.** `persist_config` MUST write via `<file>.tmp` + `std::fs::rename` (atomic on NTFS same-volume). A crash mid-write MUST NOT truncate `config.toml`. (Story 13.1.)
- **Corrupt-file quarantine.** When `load_config` or `schema::init` detects a corrupt file, the app MUST back it up to `<name>.corrupt-<timestamp>` before recovering to defaults / a fresh file. Forensic evidence MUST NOT be silently destroyed. (Stories 13.1, 13.2.)
- **Single-instance guard.** The app MUST detect an already-running instance via a Win32 named mutex (`Global\sidebar-app-single-instance`) and exit(0) on the second launch. Two instances writing the same `config.toml` + `bandwidth.db` is FORBIDDEN. (Story 13.3.)
- **Plain-language settings.** Every settings control MUST carry an `on_hover_text(...)` explanation comprehensible to a user who does not know what "binary" or "poll interval" means. Jargon labels MUST be renamed. (Story 13.4.)
- **About dialog.** The app MUST expose an About dialog (ⓘ button) showing version, LHM credit + license link, privacy-policy link, GitHub issues link, and the LHM one-time-click Full-mode instructions. (Story 13.4.)
- **Reference-machine evidence bundle.** The v1.0.0 tag MUST be backed by a `verify/reference-machine.ps1` run on the designated T-31 reference machine, producing a single evidence bundle under `verify/evidence/<date>/`. (Story 13.5, T-46.)
- **LHM one-time-click documentation.** The bundled LHM v0.9.6 binary does NOT auto-start its HTTP server from any config key. This limitation MUST be documented in the first-run wizard + About dialog + `verify/smoke-checklist.md`. (SUPERSEDED by Epic 15 — the LHM library host architecture eliminates the HTTP dependency entirely; this bullet remains as historical context for v0.9.x.)

## G29 — Silent-Failure Surfaces (Productization Pass, 2026-07-16)

A non-technical user MUST NEVER do something (click a button, complete a
wizard, change a setting) and see nothing happen. Every user action MUST
produce a visible response within 2 seconds, or a clear message explaining
the delay / failure. The following silent-failure traps are FORBIDDEN in
v1.0.0+ (Stories 14.1–14.5):

- **Launch-failure visibility.** When the user clicks the status pill to enable Full mode, the outcome (success / UAC-declined / timeout / binary-missing) MUST be surfaced as a user-facing banner — NOT only as a `tracing::warn!`. (Story 14.1.)
- **Wizard hot-start.** Completing the first-run wizard MUST hot-start the poller/accountant/supervisor in-session. The user MUST NOT be told to "restart sidebar." (Story 14.2.)
- **Per-sensor staleness.** Each reading row MUST check its own `Reading.timestamp` against now. A stale sensor MUST render dimmed + a `⏱` glyph. Only a TOTAL blackout triggers the poller-level stale badge; a single hung sensor MUST be flagged individually. (Story 14.3.)
- **Corruption banners.** Config corruption (`config.toml` malformed) + DB corruption (`bandwidth.db` garbage) MUST surface a dismissible banner naming the backup path — NOT a silent reset to defaults. (Story 14.4.)
- **Generalized message stack.** All user-facing messages (info/warning/error) MUST feed into a single `Vec<UserMessage>` framework with severity + dismiss semantics — NOT ad-hoc `Option<&'static str>` fields. (Story 14.5.)

The architectural move that underpins this guardrail is Epic 15 (LHM
library host) — it removes the root cause of the most common silent failure
(the HTTP auto-start regression). Epic 14 surfaces the rest.
