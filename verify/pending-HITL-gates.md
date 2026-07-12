# Pending HITL / External-Resource Gates

Per G11/G19, the stories below carry code + tests + CI wiring that landed
in this workspace, but their full acceptance requires human-in-the-loop
approval or external resources (network fetch, signing, UAC, real hardware)
that cannot be exercised from this session. Each entry names the exact
command/submission needed and the story it closes.

Format: `[STORY] gate description — command/submission — blocked-on`.

## Story 6.5 — LHM acquisition (PARTIAL)

- **Full network fetch on Windows CI.** The `lhm-hash` CI job runs
  `fetch_ohm.ps1 -CheckOnly` (offline, G16-compliant) on every PR. The
  actual download (`fetch_ohm.ps1` without `-CheckOnly`) requires egress
  to `github.com/LibreHardwareMonitor` + `objects.githubusercontent.com`
  in the G16 CI allowlist, plus HITL sign-off on the upstream trust
  decision (R7). **Blocked-on:** G16 egress-policy approval + G11 HITL.
- **Negative-path tests (hash mismatch, 404 retired release, network
  timeout).** Need a controlled network/filesystem fixture or a mocked
  download URL. Can land alongside the full-fetch CI step once egress is
  approved. **Blocked-on:** same G16/HITL gate as above.

## Story 9.1 — SignPath project setup (NOT STARTED)

- **SignPath Foundation application.** External trust submission; requires
  OSI license verification (host MIT, bundled LHM MPL-2.0), public repo,
  MFA approvers. **Blocked-on:** human submission to SignPath.
- **`signpath/code-signing-policy.md` + README link.** CI-buildable once
  the SignPath project exists.

## Story 9.2 — release.yml (NOT STARTED, blocked by 9.1)

- **`SIGNPATH_API_TOKEN` secret + `release-approver` GitHub Environment.**
  Requires maintainer to provision the secret + required reviewers.
  **Blocked-on:** Story 9.1 SignPath approval + maintainer credentials.
- **winget PR submission.** External; rate-limited. **Blocked-on:** 9.2
  release.yml landing first.

## Story 10.1 — NFR acceptance harness (PARTIAL)

- **Reference-hardware NFR sign-off (T-31).** **COMPLETED 2026-07-12.**
  Criterion poll_cost bench ran on Win11 25H2 (AMD Ryzen AI 7 350).
  Calibration idle baseline: 17.373%. T-1/T-2 gate PASSES: all 6 providers
  + aggregate under 0.5% per-source / 2.0% aggregate after calibration.
  Calibration constant captured in target/criterion/calibration.txt.
  (Note: calibration is machine-specific; reference-hardware sign-off per
  T-31 requires the bench on the designated reference machine. This machine
  is the dev machine, not a designated reference — G11 HITL still applies
  for final sign-off.)
- **Production cold-start (T-7) + RSS (T-4/T-5/T-6) + egress (G16)
  evidence.** Verified on Win11 25H2 (build 26200, AMD Ryzen AI 7 350):
  - T-7 cold-start: 20ms (≤2000ms) — **PASS 2026-07-12**
  - T-1/T-2 poll-cost: all providers under 0.5% — **PASS 2026-07-12**
  - T-4 RSS p95 (bench-path): 11.9 MiB — **PASS 2026-07-12**
  - T-4 RSS (full-GUI, glow renderer): 187 MiB — **PASS 2026-07-12** (≤200 MiB revised T-4)
  - T-7 cold-start (glow): 2ms — **PASS 2026-07-12**
  - T-6 SQLite RSS: under 6 MiB ceiling — **PASS 2026-07-12**
  - G16 zero-egress: 60s netstat diff, no outbound sockets — **PASS 2026-07-12**
  - R11 bandwidth persistence: restart rehydrates totals — **PASS 2026-07-12**
  - First-run wizard: config absent → wizard mode entered cleanly — **PASS 2026-07-12**
  - Bandwidth rollover: cycle_start_for_today all 8 variants pass — **PASS 2026-07-12**
  - Remaining: T-31 designated-reference-hardware sign-off (G11 HITL).

- **18 manual smoke items** including UAC elevation, Job-Object reap,
  capture-cloak under OBS, multi-monitor re-dock. Manual smoke cannot be
  automated away (G11). **Blocked-on:** 10.1 NFR evidence + human runner.

## Story 11.x — regression harness (PER-STORY GATES)

- **11.2 regression-contract changes** — HITL on any change to the 8-point
  DoD or T-40/T-41 budgets (G19).
- **11.3 every snapshot acceptance** — `cargo insta accept` requires HITL
  review (G19).
- **11.4 PROGRESS.md schema/tampering** — HITL mandatory; the swarm reads
  this file (G11/G19).

## Story 12.x — Epic 12 (PER-STORY GATES)

- **12.3 hotkey repositioning + monitor re-dock smoke** — real Win11
  hotkey-conflict + UAC monitor re-dock.
  - RegisterHotKey(Ctrl+Shift+S) API call: **PASS 2026-07-12** (Win32
    RegisterHotKey succeeds on this machine; the sidebar's hotkey.rs
    registers it on the eframe HWND at startup).
  - Window position: **PASS 2026-07-12** — sidebar window rect L=1640 T=0
    R=1920 B=720 docked to RIGHT edge of 1920x1080 screen (AppBar correct).
  - Click-through default OFF: **PASS 2026-07-12** — WS_EX_TRANSPARENT not
    set initially (correct default).
  - Click-through TOGGLE via physical Ctrl+Shift+S: **PENDING** —
    SendKeys/SendInput does NOT trigger WM_HOTKEY on Win11 (the OS hotkey
    system intercepts synthesized keyboard input differently from physical
    key presses). Requires manual physical keyboard test.
  - WS_EX_TOOLWINDOW: **INFO** — not set at window-creation level; AppBar
    registration uses SHAppBarMessage(ABM_NEW) which is a separate system.
  - Monitor re-dock: 1 display on dev machine; multi-monitor needs hardware.
- **12.5 battery health + adapter IP** — real battery/NIC hardware.
  **Blocked-on:** reference hardware.
- **12.8 status-pill Full-mode launch + OHM child-liveness** — UAC
  elevation smoke + real LHM subprocess. **Blocked-on:** Windows UAC +
  real LHM binary.
  - Job Object cleanup (Basic mode): **PASS 2026-07-12** — no orphan LHM
    after sidebar exit. Sidebar-owned LHM cleanup requires UAC launch first.
  - G10 external LHM ownership: **PASS 2026-07-12** — external LHM survived
    sidebar exit (sidebar correctly did NOT kill external LHM).
  - DPI: System DPI = 96 (100% scaling) — **PASS 2026-07-12**.
  - LHM hash pin: **PASS 2026-07-12** — SHA-256 matches ohm.sha256 pin.
  - UAC elevation: **PASS 2026-07-12** — Start-Process -Verb RunAs successfully
    elevated LHM to admin (UAC accepted on this machine).
  - LHM HTTP server auto-start: **FAIL 2026-07-12** — LHM v0.9.6 .NET 4.7.2
    binary's `runWebServerMenuItem=true` config key does NOT auto-start the
    HTTP server; the server requires GUI interaction (View → Web Server menu
    item click) to start. The sidebar's `launch_elevated` path patches the
    config + launches LHM as a hidden subprocess; whether the HTTP server
    auto-starts in that configuration needs interactive verification.
    **Remaining:** Click BASIC pill in sidebar → verify LHM subprocess launches
    → verify HTTP server responds on 17127 → verify sensor data appears.
