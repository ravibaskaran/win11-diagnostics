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

- **Reference-hardware NFR sign-off (T-31).** The Criterion bench +
  parse_threshold gate + calibration constant mechanism all land in code;
  the actual T-1/T-2 numbers must be captured on a reference Win11 24H2+
  machine and committed to `target/criterion/calibration.txt` (or a
  checked-in equivalent). **Blocked-on:** reference-hardware run +
  HITL on the calibration constant (G11).
- **Production cold-start (T-7) + RSS (T-4/T-5/T-6) + egress (G16)
  evidence.** Verified on Win11 25H2 (build 26200, AMD Ryzen AI 7 350):
  - T-7 cold-start: 20ms (≤2000ms) — **PASS 2026-07-12**
  - T-4 RSS p95: under 80 MiB (30s probe) — **PASS 2026-07-12**
  - T-6 SQLite RSS: under 6 MiB ceiling — **PASS 2026-07-12**
  - G16 zero-egress: 60s netstat diff, no outbound sockets — **PASS 2026-07-12**
  - R11 bandwidth persistence: restart rehydrates totals — **PASS 2026-07-12**
  - T-1/T-2 CPU cost: Criterion bench exists but the calibration constant
    (`calibration_idle_cpu_percent`) must be captured + committed. The
    parse_threshold gate runs in CI but without a checked-in baseline.
    **Remaining:** T-31 calibration constant commit + T-2 aggregate sign-off.

## Story 10.2 — smoke checklist (NOT STARTED, blocked by 10.1)

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
  hotkey-conflict + UAC monitor re-dock. **Blocked-on:** Windows smoke.
- **12.5 battery health + adapter IP** — real battery/NIC hardware.
  **Blocked-on:** reference hardware.
- **12.8 status-pill Full-mode launch + OHM child-liveness** — UAC
  elevation smoke + real LHM subprocess. **Blocked-on:** Windows UAC +
  real LHM binary.
