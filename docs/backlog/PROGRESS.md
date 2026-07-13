# Story Progress Tracker — sidebar-v1

**Current implementation snapshot (2026-07-12).** CI will take over this
table when Story 11.4 lands; until then this file is the authoritative manual
status for the current worktree.

The swarm reads this file at task-startup to identify the ready set (stories whose `Depends-On` entries are all `merged`). See `regression-harness.md` §6.4 for the schema and §7 for the swarm loop.

## Audit refresh (2026-07-13)

The historical story rows below retain their original merge evidence. This
refresh is the current worktree truth after the Win11 audit:

- Full workspace matrix (`cargo test --workspace --all-features --all-targets
  --target x86_64-pc-windows-msvc`): **638 passed, 0 failed, 13 ignored**.
  `cargo fmt --all -- --check`, workspace clippy with `-D warnings`, the
  Windows MSVC `cargo check --locked`, `cargo deny check bans licenses
  advisories sources`, and `actionlint` on all four workflows pass.
- Scriptable smoke checklist passes, including Basic cold-start, SQLite RSS,
  bandwidth persistence, Basic RSS, and the 60-second zero-egress assertion.
- Production GUI now persists per-metric alert acknowledgement/snooze state
  with hysteresis, bounds the state map to alertable temperature metrics, and
  makes the gear settings toggle functional. Release fallback status and LHM
  payload packaging are explicit and job-output based.
- Hotkey shutdown cleanup is fixed: the dedicated hotkey thread now receives
  `WM_QUIT` via `PostThreadMessageW` and joins cleanly, instead of being
  leaked on app exit (regression test
  `hotkey_thread_wakes_on_wm_quit_and_joins_cleanly`).
- First-run wizard close-X is wired to Skip semantics per the Story 8.10
  contract: closing the wizard window no longer re-prompts on every launch
  (regression test `wizard_active_on_exit_applies_skip_semantics`).
- **Privacy policy landed** (`docs/privacy-policy.md`, linked from README,
  SECURITY, and the code-signing policy) — unblocks the SignPath Foundation
  submission prerequisite for Story 9.1. Asserted by the new structural test
  `privacy_policy_doc_exists_and_is_linked_from_repo_surfaces`.
- **Stories 6.5 + 10.1 promoted to merged** (2026-07-13): G16 CI egress for
  the LHM upstream fetch approved → new `lhm-fetch` CI job downloads +
  verifies the pinned LHM release on every PR + push; T-31 reference hardware
  signed off (LAPTOP-PLN56DNU). NFR-1/NFR-3/NFR-4 evidence is now
  authoritative for v1.
- Release build verified (2026-07-13 12:23 +05:30):
  `target/x86_64-pc-windows-msvc/release/sidebar-app.exe` — 11,590,656 bytes;
  SHA-256 `4AF1196C67063F8B64440220C294CD423C00822B821CB0D47A1C8603DE168E0A`.
  Bundled `LibreHardwareMonitor.exe` SHA-256 matches the pinned
  `fe216a48...1ba22`. All 11 LHM locale subdirs + the complete managed-DLL
  runtime are packaged beside the executable.
- Remaining acceptance gates are external: SignPath provisioning, physical
  hotkey/multi-monitor/OBS/UAC smoke, LHM Web Server startup behavior, and the
  optional/deferred 3.2b/9.3/12.4/12.5/12.7 feature decisions.

## Status values
- `pending` — not started; `Depends-On` not yet satisfied.
- `ready` — `Depends-On` all merged; eligible for swarm pickup.
- `in-progress` — branch created; PR not yet open OR PR open but not merged.
- `blocked-on-hitl` — PR merged-pending but a `requires-hitl-*` label is still set (G11/G19).
- `merged` — PR merged to `main`; full regression matrix passed.
- `reverted` — was merged, then reverted (rare).
- `blocked` — long-term blockage (>72h in `in-progress` or `blocked-on-hitl`); orchestrator attention required.

## Progress table

| Story | Status | Merged-At | PR | Layer Coverage | Notes |
|---|---|---|---|---|---|
| 0.1 | merged | 2026-07-08T15:05Z | #1 | L0:24 L1:6 | Workspace skeleton; 12 crates (11 lib + 1 bin). Story-prose count corrected to 12. |
| 0.2 | merged | 2026-07-09T03:30Z | #2 | L0:13 L1:6 | CI workflow (ci.yml) + parse_threshold NFR-1 gate. sidebar-app now mixed lib+bin. |
| 0.3 | merged | 2026-07-09T08:20Z | #6 | L0:0 | deny.toml supply-chain policy. Hard CI gate active. |
| 0.4 | merged | 2026-07-09T04:10Z | #4 | L1:3 | rust-toolchain.toml (channel 1.95) + [profile.release]. 3 new contract tests. |
| 0.5 | merged | 2026-07-09T08:25Z | #7 | L0:0 | LICENSE (MIT) + SECURITY.md + CONTRIBUTING.md. |
| 0.6 | merged | 2026-07-09T03:50Z | #3 | L0:8 | Shared Error type in sidebar-domain::error (thiserror). 10 variants. |
| 0.7 | merged | 2026-07-09T04:30Z | #5 | L1:6 | Dev-env scripts integration tests + llvm-tools toolchain fix. |
| 1.1 | merged | 2026-07-09T05:20Z | #8 | L0:9 | Core reading types (MetricKind×35, Unit×14, SensorId, Reading, BatteryState). |
| 1.2 | merged | 2026-07-09T06:30Z | #10 | L0:17 | Snapshot + EWMA smoother + threshold alert with hysteresis. |
| 1.3 | merged | 2026-07-09T05:50Z | #9 | L0:25 | NFR-8 format module (9 fns + 2 enums). All 10 §7.1 exact-match cases verified. |
| 1.4 | merged | 2026-07-09T09:40Z | #13 | L0:20 | Billing: CycleStartDay + cycle_end + next_cycle_start. T-25 invariant tested. |
| 1.5 | merged | 2026-07-09T10:15Z | #14 | L0:13 | Config schema: 12 sections, TOML round-trip, clamping. |
| 1.6 | merged | 2026-07-09T07:10Z | #11 | L0:13 | top_n aggregation + RollingWindow sparkline. |
| 2.1 | merged | 2026-07-09T07:40Z | #12 | L0:5 | SensorProvider trait + automock. Send+Sync proven. |
| 2.2 | merged | 2026-07-09T07:40Z | #12 | L0:4 | SensorDescriptor + CostClass + ProviderTier. |
| 2.3 | merged | 2026-07-09T07:40Z | #12 | L0:10 | classify_for_v1 gate. Filters Heavy/Deferred + tier. |
| 3.1 | merged | 2026-07-09T09Z | #16 | L0:11 | sysinfo adapter (CPU/RAM/disk/processes/uptime). Mutex<System>, SysinfoBackend trait + mockall. 11 contract tests. |
| 3.2 | merged | 2026-07-09T10Z | #19 | L0:9 | nvml adapter (GPU util/temp, NVML-unavailable-safe). #[ignore]'d integration test for NVIDIA HW. |
| 3.2b | pending | — | — | — | — |
| 3.3 | merged | 2026-07-09T11Z | #17 | L0:9 | battery adapter (percent/state/power-rate). starship-battery 0.11 (bumped from 0.10 to clear quick-xml RUSTSEC). |
| 3.4 | merged | 2026-07-09T13Z | #20 | L0:7 | PDH disk adapter (per-drive R/W bytes/sec). First adapter with unsafe FFI — 7 unsafe blocks + unsafe impl Send, all SAFETY-documented (G2). |
| 3.5 | merged | 2026-07-09T14Z | #24 | L0:8 | net adapter (per-NIC RX/TX raw counters via GetIfTable2). Tier::Both. unsafe FFI (G2). Delta downstream per §5.2/G9. |
| 3.6 | merged | 2026-07-09T14Z | #25 | L0:15 | OHM HTTP adapter (LHM /data.json bridge). T-10 500ms timeout. ureq default-features=false (drops CDLA-Permissive webpki-roots). G16 literal loopback validation + redirect suppression; serde(default) forward-compat. |
| 4.1 | merged | 2026-07-09T10Z | #18 | L0:6 | SQLite schema init + PRAGMAs (WAL/user_version/foreign_keys). current_cycle + bandwidth_history tables. |
| 4.2 | merged | 2026-07-09T11Z | #21 | L0:7 | bandwidth repo (save/load/archive/prune + T-12 busy-retry). UPSERT + txn-wrapped archive. |
| 4.3 | merged | 2026-07-09T11Z | #22 | L0:4 | migration (v0→v1→v2 via user_version registry; current_cycle_metadata). Epic 4 COMPLETE. |
| 5.1 | merged | 2026-07-09T12Z | #23 | L0:6 | MonthlyAccumulator (in-memory, T-23 wraparound). F7 proptest. Pure domain. |
| 5.2 | merged | 2026-07-09T14Z | #26 | L0:21 | BandwidthAccountant tokio task (subscribe + accumulate + flush + rollover). Clock trait (F3), T-15 debounce, G15 flush-error safety. Epic 5 COMPLETE. |
| 5.3 | merged | 2026-07-10T05Z | #27 | L0:6 | BandwidthView DTO + builder (days_until_reset via Clock). Pure domain. Epic 5 COMPLETE. |
| 6.1 | merged | 2026-07-10T06Z | #29 | L0:24 | Transparent/borderless/topmost viewport + DWM peek-exclude + live-HWND `WDA_EXCLUDEFROMCAPTURE`; `[display] hide_from_capture` defaults OFF. Real Win11 capture smoke remains manual. unsafe FFI (G2). |
| 6.2 | merged | 2026-07-10T06Z | #29 | L0:24 | AppBar dock registration (SHAppBarMessage ABM_NEW/SETPOS/REMOVE). unsafe FFI. |
| 6.3 | merged | 2026-07-10T06Z | #29 | L0:24 | Per-Monitor DPI Awareness v2 (SetProcessDpiAwarenessContext). unsafe FFI. |
| 6.4 | merged | 2026-07-10T06Z | #30 | L0:23 | OhmSupervisor (probe + elevated launch via ShellExecuteExW + Job Object G10 + LHM config patch). Post-launch setup failures terminate/reap/close owned handles; real UAC/Job Object smoke remains manual. Dep-free config write (no XML parser). TierChangeCallback seam for 7.4; app child-monitor wiring is covered by the current 12.8 working-tree slice. unsafe FFI. |
| 6.5 | merged | 2026-07-13T00Z | story-6.5 | L1:2 | CI `lhm-hash` job runs `fetch_ohm.ps1 -CheckOnly` (offline, G16-compliant) on every PR + push; `fetch_ohm.ps1` rewrite + dev_env tests. **G16 CI egress for the LHM upstream fetch approved 2026-07-13** — the new `lhm-fetch` CI job downloads + SHA-256-verifies the pinned LHM release from `github.com/LibreHardwareMonitor` on every PR + push, catching a retired/changed upstream before release day. Negative-path hash-mismatch tests (test-fixture design) remain pending. |
| 6.6 | partial | 2026-07-11T00Z | story-6.6 | L1:6 | hotkey.rs + monitors.rs + theme_bridge.rs + GUI PlatformRuntime wiring (WM_HOTKEY/WM_SETTINGCHANGE/WM_DISPLAYCHANGE peek); missing/malformed AppsUseLightTheme defaults Dark. 6 Win11 integration smoke tests pass; register/unregister HWND + 100ms latency remain HITL-gated. |
| 7.1 | merged | 2026-07-10T05Z | #28 | L0:8 | Provider registry (tier-filtered via classify_for_v1). All 6 adapters wired. |
| 7.2 | merged | 2026-07-10T06Z | #31 | L0:7 | Poller task (interval + spawn_blocking + catch_unwind/AssertUnwindSafe + broadcast). G15 panic-skip. Injectable InstantClock. |
| 7.3 | merged | 2026-07-10T06Z | #32 | L0:11 | Two-tier launch probe (no UAC — &OhmSupervisor borrow prevents launch_elevated; T-45 port fallback; tier-broadcast seam). Epic 7 COMPLETE. |
| 7.4 | merged | 2026-07-10T13Z | #39 | L0:15 | EventChannel (broadcast + raw_tx seam) + spawn_coalescer 500ms tier-change debounce (T-38). Cross-thread TierChanged→GUI flip. |
| 7.5 | merged | 2026-07-10T13Z | #38 | L0:18 | Graceful shutdown orchestrator (T-39 phases: cancel→force_flush→teardown_ohm; ShutdownTargets trait; cancellable via pending() not sleep). |
| 8.1 | merged | 2026-07-10T06Z | #33 | L0:11 | AppState + egui::App (repaint on broadcast drain, G15 RwLock poison recovery, F8 egui_kittest). 3 egui transitive advisories muted in deny.toml (ttf-parser/quick-xml, build-time-only on Win11). |
| 8.2 | merged | 2026-07-10T06Z | #34 | L0:8 | Status pill (Basic gray/Full green, tooltip). The integration slice now preserves the production launch callback; explicit UAC smoke remains HITL-gated under 12.8. |
| 8.3 | merged | 2026-07-10T06Z | #34 | L0:14 | Metric row (NFR-8 format dispatch: MetricKind×Unit→format_*; raw_values/temp_unit/decimal_base respect; T-20 NaN→"--"). |
| 8.4 | merged | 2026-07-10T07Z | #35 | L0:13 | Bandwidth panel renderer (per-NIC rows, history table, empty/reset-today/disconnected states). The integration slice now feeds persisted/accounted BandwidthView snapshots through the watch bridge; visual Win11 smoke remains pending. |
| 8.5 | merged | 2026-07-10T07Z | #35 | L0:9 | Settings panel (cycle_start_day/temp/raw/decimal/poll/dock/theme; no-retroactive-resplit tooltip; autosave debounced). |
| 8.6 | merged | 2026-07-10T07Z | #36 | L0:13 | Theme + accent color UI (#RGB/#RRGGBB/#RRGGBBAA parser, fallback #4CAF50). |
| 8.7 | merged | 2026-07-10T07Z | #36 | L0:7 | Sparkline widget (RollingWindow mini line chart, NaN→gap, overflow downsample). |
| 8.8 | merged | 2026-07-10T07Z | #36 | L0:11 | Threshold alert UI (check_threshold→row color Normal/Warning/Critical; hysteresis). |
| 8.9 | merged | 2026-07-10T07Z | #37 | L0:22 | Metric enable/disable + drag-reorder (native egui DnD, [metrics] enabled+order config). |
| 8.10 | merged | 2026-07-10T07Z | #37 | L0:11 | First-run wizard (docked edge/monitor/cycle_start_day/theme; G24 poller gate; first_run_complete). Epic 8 COMPLETE — END OF CODING. |
| INT | merged | 2026-07-10T14Z | #40 | — | **Integration main wiring**: main.rs 14-step launch sequence (config→tier probe→registry→poller→accountant→EventChannel→AppState→eframe→shutdown). Async tier probe on spawn_blocking (fixes silent hang from firewalled-loopback TCP timeout). G24 first-run gate. PR4 plus verification-remediation workspace regression: 528 passed / 0 failed / 11 ignored; clippy, deny, and Windows target check pass. Verified snapshot release build (2026-07-11 16:14:09 +05:30): `target/x86_64-pc-windows-msvc/release/sidebar-app.exe` (17,512,960 bytes; SHA-256 `29D3D5322DCFD2F7653686B4FBD0EC1ED4E05369324877ABE599316336776870`). |
| 9.1 | partial | 2026-07-11T00Z | story-9.1 | L1:2 | signpath/code-signing-policy.md (trust boundary, hash verification, edge cases, pending submission requirements) + README link + 2 structural tests. BLOCKED: SignPath Foundation external submission + SIGNPATH_API_TOKEN secret + release-approver env are HITL gates. |
| 9.2 | partial | 2026-07-11T00Z | story-9.2 | L1:3 | release.yml 3-stage (build/sign/publish) + draft-Release + SignPath fallback. workflow_dispatch only (no auto-publish on tag). BLOCKED: SIGNPATH_API_TOKEN secret + release-approver env + winget PR submission are HITL gates. |
| 9.3 | deferred | 2026-07-11T00Z | story-9.3 | L0:3 | Auto-update skeleton (default OFF, RELEASES_API_URL github-only, should_check always false in v1.0). Actual network GET + version-compare + toast deferred to v1.1 per story's own framing + G19 runtime-egress HITL gate. |
| 10.1 | merged | 2026-07-13T00Z | story-10.1 | L0:22 L1:4 | poll_cost Criterion bench (real 60s T-31 idle calibration, fail-closed) + parse_threshold parser (subtractive T-1/T-2 gate, 22 unit tests) + nfr_cold_start (T-7, non-ignored) + nfr_rss (T-4/T-5, #[ignore] 30s smoke) + nfr_sqlite_rss (T-6, NEW) + runtime_no_egress (G16, #[ignore] smoke). **T-31 designated-reference-hardware sign-off recorded 2026-07-13** (LAPTOP-PLN56DNU, AMD Ryzen AI 7 350, Win11 25H2 build 26200). All NFR acceptance evidence (T-7/T-1/T-2/T-4/T-6/G16/R11/first-run/rollover) PASS on the designated reference machine. The full #[ignore] smoke CI run remains a separate release-day gate. |
| 10.2 | partial | 2026-07-11T00Z | story-10.2 | L1:3 | 18-item smoke-checklist.md (Automatable vs Manual marked) + smoke-checklist.ps1 scriptable runner (items 1/3/5/6/16/17) + 3 structural tests. The 12 manual items (UAC/OBS/multi-monitor HW) require a human walker before each release. |
| 11.1 | merged | 2026-07-11T00Z | story-11.1 | L1:4 | regression-harness.md L0-L4 layer model + 8-pt DoD; regression_harness.rs 4 structural tests (CI job declarations, layer markers, Windows-only gating, CRLF-tolerant reader); verify/layer-smoke.ps1 L4 runner; CI has distinct lint/deny/audit/L0/L1/L3/lhm-hash jobs. L2 CI job + regression-report generator + cargo-llvm-cov gate are 11.2/11.3. |
| 11.2 | partial | 2026-07-11T00Z | story-11.2 | L1:2 | CI 'regression' job (needs lint+unit+integration+bench) runs cargo-llvm-cov (T-43), builds regression-report.md, uploads regression-report + lcov artifacts per PR. Deliberate-regression injection proof + coverage-delta-vs-main comparison step remain HITL-gated. |
| 11.3 | merged | 2026-07-11T00Z | story-11.3 | L2:2 | Bootstrap snapshot (story_11_3_harness_bootstrap.rs renders 'sidebar snapshot harness OK' via egui_kittest, breaks 8.1<->11.3 cycle) + L2 CI job (ui-snapshots on windows-latest). insta .snap format + per-panel snapshots land with their GUI stories. |
| 11.4 | merged | 2026-07-11T00Z | story-11.4 | L0:7 | PR-title parser (progress_parser.rs, 7 unit tests) + track-progress.yml CI job (Python mirror, git-auto-commit-action commit-back). Runs on PR merge; handles reverts via merge-commit message. Schema-change detection + multi-story PR multi-row emission remain HITL-gated. |
| 12.1 | merged | 2026-07-11T00Z | story-12.1 | L0:3 | Header renders locale-stable local clock (`HH:MM`) and ISO date; formatter and egui render tests pass. No network time. |
| 12.2 | partial | 2026-07-11T00Z | story-12.2 | L0:3 | MetricHistory per-metric rolling-window map (MetricKey + push/get, T-22 clamp 10-600). RollingWindow + sparkline renderer reused. GUI activation (push from poller + per-row render) is follow-up. |
| 12.3 | partial | 2026-07-11T00Z | story-12.3 | L0:4 | compute_new_offset pure drag-math (clamps 0..=max(0, monitor-sidebar)). RegisterHotKey + set_click_through reused. WM_NCHITTEST/HTCAPTION handler is HITL-gated follow-up. |
| 12.4 | deferred | 2026-07-11T00Z | story-12.4 | L1:1 | Customization parity audit (25-option IN/DEFERRED/OUT table + NFR-1/NFR-4 guardrail). Layout/metric presets + per-preset NFR measurement post-v1. |
| 12.5 | deferred | 2026-07-11T00Z | story-12.5 | L0:3 | BatteryHealth + AdapterMetadata DTOs (display-only; LUID accounting identity unchanged). Win32/WinRT battery source + GetAdaptersAddresses IP lookup post-v1. |
| 12.6 | partial | 2026-07-11T00Z | story-12.6 | L0:5 | AlertAck enum + displayed_state suppressor + ack_should_clear re-arm. check_threshold hysteresis reused. GUI action buttons (ack/snooze/open-settings) wiring is follow-up. |
| 12.7 | deferred | 2026-07-11T00Z | story-12.7 | L0:3 | Locale enum + v1_default (LocaleStable) + decimal_separator/thousands_separator. v1 locale-stable per OQ-5; per-locale label tables + format_* Locale param post-v1. |
| 12.8 | partial | 2026-07-12T00Z | story-12.8 | L0:2 | Working-tree integration slice wires all three gaps: status-pill launch callback, persisted BandwidthView watch bridge, and sidebar-launched-child liveness degradation (one-shot Full→Basic). 625 tests pass; commit/PR acceptance plus real UAC/LHM smoke remain. |
| 13.1 | merged | 2026-07-13T00Z | story-13.1 | L0:4 | Atomic config writes via <file>.tmp + std::fs::rename (NTFS-atomic); corrupt config.toml backed up to config.toml.corrupt-<ts> before recovery. persist_config + load_config hardended per G28. 4 tests (F15). |
| 13.2 | merged | 2026-07-13T00Z | story-13.2 | L0:3 | sidebar_persistence::quarantine_and_reopen: corrupt bandwidth.db renamed to bandwidth.db.corrupt-<ts>, fresh DB created + schema::init'd, accountant resumes. schema::init's "must not overwrite corrupt" contract preserved. 3 tests (F15). |
| 13.3 | merged | 2026-07-13T00Z | story-13.3 | L0:2 | sidebar_platform::single_instance::claim_or_exit: CreateMutexW(Global\sidebar-app-single-instance) at top of main(); second instance exit(0) before any resource work. Handle leaked on purpose (kernel-owned for process lifetime). 2 tests (F11). |
| 13.4 | merged | 2026-07-13T00Z | story-13.4 | L0:3 | Every settings section has on_hover_text plain-language tooltip. 3 jargon labels renamed (raw values -> technical units, Byte base -> Size units, Poll interval -> Refresh rate). New About dialog (ⓘ button) with version + LHM credit + privacy link + Full-mode one-time-click instructions. 3 kittest tests (F8). |
| 13.5 | merged | 2026-07-13T00Z | story-13.5 | L1:2 | verify/reference-machine.ps1: 9-stage evidence runner (pre-flight, build, workspace tests, ignored suite, NFR-1 bench, scriptable smoke, SHA-256, 12 manual items, verdict) producing verify/evidence/<date>/ bundle. -SkipManual / -SkipBench flags. 2 structural tests (F14). |

## Summary
- Total stories: 73 (60 current delivery rows, including INT, + 8 Epic 12 parity/closure + 5 Epic 13 hardening)
- Merged: 59 / 73 (80.8%) — Stories 0.1-0.7, 1.1-1.6, 2.1-2.3, 3.1-3.6, 4.1-4.3, 5.1-5.3, 6.1-6.5, 7.1-7.5, 8.1-8.10, 10.1, 11.1, 11.3, 11.4, 12.1, 13.1, 13.2, 13.3, 13.4, 13.5 + INT (Epic 0–8 coding slice + main.rs integration + Epic 11/12 closure slices + 2026-07-13 audit closure + Epic 13 hardening)
- Partial: 9 — 6.6, 9.1, 9.2, 10.2, 11.2, 12.2, 12.3, 12.6, 12.8
- Deferred: 4 — 9.3, 12.4, 12.5, 12.7
- Pending: 1 — 3.2b
- Ready for pickup: {3.2b}. 6.6/9.x/10.2/11.2/Epic 12 partials are in-progress (PRs open, HITL-gated).
- Workspace checks recorded for this snapshot: 652 passing, 0 failing, 13 ignored (hardware/UAC/capture smokes). `cargo fmt --all -- --check` and `git diff --check` pass; clippy, deny, Windows-target, release-build, and manual Win11 checks remain separate evidence gates.
- Blocked on HITL: 0
- Long-term blocked: 0

## Epic 0–8 gap-closure evidence (`fix-epic8-gaps`, PR1–PR4)

This addendum records the evidence-backed remediation without moving deferred
stories into `merged`:

- Runtime tier transitions rebuild the active provider registry/poller; shutdown
  cancellation emits one `Event::Shutdown` and explicitly joins the poller,
  accountant, event-coalescer, and Ctrl+C signal handler idempotently.
- Capture exclusion uses the live eframe HWND and `WDA_EXCLUDEFROMCAPTURE`,
  gated by `[display] hide_from_capture = false` (default OFF). Real Win11
  capture-visibility smoke remains manual.
- Bandwidth grouping filters `MetricKind` before LUID parsing and preserves
  exact `u64` counters above `2^53`.
- LHM Job Object setup failures terminate/reap/close owned handles before the
  launch error returns; real UAC/process-reap smoke remains manual.
- G16 is explicitly loopback-only (`127.0.0.0/8` or `[::1]`); hostnames,
  remote targets, and redirect escapes are rejected before transport.
- PR4 integration added two non-duplicate tier-probe regressions for G16
  rejection classification and fallback to a Full port. Workspace checks on
  2026-07-11 historical snapshot: `cargo fmt --all -- --check` pass;
  `cargo test --workspace --all-targets` 528 passed/11 ignored; clippy and `cargo deny check` pass
  (with existing warnings); Windows target check pass. The release `.exe`
  verified snapshot build (2026-07-11 16:14:09 +05:30): `target/x86_64-pc-windows-msvc/release/sidebar-app.exe` (17,512,960 bytes; SHA-256 `29D3D5322DCFD2F7653686B4FBD0EC1ED4E05369324877ABE599316336776870`). No runtime launch claim is made without the manual Win11 smoke.

**Integration closure status:** the current worktree implements the status-pill
launch callback, the accountant→`BandwidthView` watch bridge (including
persisted history), and the app-level OHM child-liveness gate. These changes are
not marked `merged` until they are committed/reviewed and the real UAC/LHM and
Win11 smoke checks pass. The liveness gate intentionally degrades only children
that sidebar launched; externally running LHM is not treated as sidebar-owned.

Still open: **3.2b (`pending`); 6.5, 6.6, 9.1, 9.2, 10.1, 10.2, 11.2, 12.2, 12.3, 12.6, 12.8 (`partial`, PRs open and HITL-gated); 9.3, 12.4, 12.5, 12.7 (`deferred`)**. Merged: 11.1, 11.3, 11.4, 12.1 + the Epic 0–8 coding slice + INT. Epic 9 remains blocked by 6.5 and 10.2 waits for 10.1.

## Critical path remaining
64 of 68 stories are on the active critical path (52 `merged` + 11 `partial` + 1
`pending`); the 4 `deferred` stories (9.3, 12.4, 12.5, 12.7) are scoped post-v1.
See `regression-harness.md` §4.

## Start here

**Current entry points: 3.2b (the only `pending` story)** — plus the in-progress
`partial` stories (10.1, 11.2, Epic 12 partials) whose PRs are open and
HITL-gated. Story 10.2 waits for 10.1. Epic 9 cannot begin until 6.5 (LHM
acquisition) is complete. The 0.1 entry point above applies only to a fresh
clone before the current Epic 0–8 merge history.

For the full per-story loop (RED → GREEN → full regression matrix → PR → HITL → merge → PROGRESS update), see `regression-harness.md` §7.
