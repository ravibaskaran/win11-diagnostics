# Smoke Checklist — sidebar-v1 (Story 10.2)

Manual + scriptable acceptance smoke for the Win11 sidebar binary. Run on
a real Win11 24H2/25H2 host before tagging a release. Cited: Story 10.2
DoD, architecture.md §7.4, nfr-thresholds.md T-31.

## How to run

```pwsh
# Scriptable items (automatable subset):
pwsh verify/smoke-checklist.ps1

# Manual items (require human eyes):
# Walk the table below; mark each item PASS / FAIL / BLOCKED.
```

## Items

| # | Layer | Item | Automatable | Notes |
|---|---|---|---|---|
| 1 | L4 | Cold-start ≤2s (Basic mode) | yes (nfr_cold_start) | T-7 |
| 2 | L4 | Cold-start ≤6s (Full mode, incl. LHM) | manual (UAC) | T-8 |
| 3 | L4 | Steady RSS ≤80 MiB (Basic) | yes (nfr_rss, #[ignore]) | T-4 |
| 4 | L4 | Steady RSS ≤120 MiB (Full) | manual (UAC) | T-5 |
| 5 | L4 | SQLite RSS contribution ≤3 MiB | yes (nfr_sqlite_rss) | T-6 |
| 6 | L4 | Zero runtime egress (netstat diff) | yes (runtime_no_egress, #[ignore]) | G16 |
| 7 | L4 | Transparent topmost viewport (no title bar) | manual | Story 6.1 |
| 8 | L4 | AppBar dock registration (right edge) | manual | Story 6.2 |
| 9 | L4 | Per-monitor DPI v2 (text crisp on 4K) | manual | Story 6.3 |
| 10 | L4 | UAC elevation flow (status pill → LHM launch) | manual (UAC) | Story 6.4 / 12.8 |
| 11 | L4 | Job Object reap on host exit (no orphan LHM) | manual (Task Manager) | G10 |
| 12 | L4 | Capture cloak (WDA_EXCLUDEFROMCAPTURE under OBS) | manual (OBS) | Story 6.1 |
| 13 | L4 | Hotkey toggle (Ctrl+Shift+S → click-through) | manual | T-34 |
| 14 | L4 | Theme switch (Dark ↔ Light via system setting) | manual | T-35 |
| 15 | L4 | Multi-monitor re-dock (disconnect primary) | manual (HW) | T-36 |
| 16 | L4 | Bandwidth counter persists across restart (R11) | yes (restart_mid_cycle test) | Story 5.2 |
| 17 | L4 | Poll interval clamp (1s-60s, warn on clamp) | yes (config tests) | T-3 |
| 18 | L4 | Graceful shutdown ≤3s (close window) | manual (timer) | T-19 |

## Scriptable harness

`verify/smoke-checklist.ps1` runs the automatable subset (items 1, 3, 5, 6,
16, 17) via `cargo test --ignored` + `cargo test` filters. A failed item
prints the failing test name + the relevant T-* / Story id. Manual items
must be walked by a human and marked PASS / FAIL on the release checklist.

## Reference machine runner (Story 13.5, T-46)

For the v1.0.0 tag, the scriptable subset above is NOT sufficient on its
own — the 13 `#[ignore]`'d integration tests (real AppBar/DPI/DWM/OHM-supervisor
FFI) + the NFR-1 poll-cost bench + the 12 manual items must also be walked
on the designated T-31 reference machine. `verify/reference-machine.ps1`
bottles all of that into one command:

```pwsh
# Elevated PowerShell 7 on the T-31 reference machine (LAPTOP-PLN56DNU).
# Run after `git pull` on main + after the release exe is built.
pwsh verify/reference-machine.ps1
```

The script writes the full evidence bundle to `verify/evidence/<date>/`
(workspace-tests.txt, ignored-suite.txt, poll_cost.txt, scriptable-smoke.txt,
sha256.txt, manual-smoke.md) and exits 0 on full PASS / 1 on any failure.
See `nfr-thresholds.md` T-46 for the bundle contract.

## Full-mode one-time setup (LHM, Story 13.5)

The bundled LibreHardwareMonitor v0.9.6 binary does NOT auto-start its HTTP
server from any config key. The first time a user enables Full mode, they
must perform a one-time click:

1. Click the **BASIC** status pill in the sidebar → accept the Windows UAC
   prompt. The sidebar launches the bundled LHM as an elevated, hidden
   subprocess.
2. If sensor readings (CPU package temp, fan speeds, voltages) do NOT
   appear within ~10 seconds, find the **LibreHardwareMonitor** icon in the
   system tray (bottom-right), right-click it → **View** → **Web Server**.
   This is a one-time setup; LHM remembers the setting for subsequent
   launches.
3. Click the sidebar status pill again (it may still show BASIC) — the
   probe now succeeds, the pill turns green, and Full-mode sensors render.

This is the only non-idiot-proof step in v1.0.0 (per Epic 13's Path A
decision). The About dialog (Story 13.4) + the first-run wizard document
this. A v1.1 story may revisit upgrading LHM to a build that auto-starts
the HTTP server.

## Release gate

Per Story 9.2, a release tag MUST NOT be cut until every item is PASS or
explicitly waived (with HITL rationale). Scriptable items failing blocks
the release; manual items failing blocks unless a maintainer signs off.
For v1.0.0+, the reference-machine runner (Story 13.5) MUST also produce a
green evidence bundle under `verify/evidence/<date>/` before the tag is
cut (T-46).
