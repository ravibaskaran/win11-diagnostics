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

## Release gate

Per Story 9.2, a release tag MUST NOT be cut until every item is PASS or
explicitly waived (with HITL rationale). Scriptable items failing blocks
the release; manual items failing blocks unless a maintainer signs off.
