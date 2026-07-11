# Story 10.2 — scriptable smoke-checklist runner.
#
# Runs the automatable subset of verify/smoke-checklist.md items. The manual
# items (UAC, OBS, multi-monitor HW, etc.) are walked by a human; this script
# covers items 1, 3, 5, 6, 16, 17.
#
# A failed item prints the failing test name + the relevant T-* / Story id.
# Exit code is non-zero if any item fails.

#Requires -Version 7.0
[CmdletBinding()]
param(
    [switch]$SkipIgnored  # Skip the #[ignore]-gated 30s/60s smokes
)

$ErrorActionPreference = 'Stop'

function Invoke-SmokeItem {
    param(
        [string]$Id,
        [string]$Name,
        [string]$Threshold,
        [scriptblock]$Command
    )
    Write-Host "`n=== $Id: $Name ($Threshold) ===" -ForegroundColor Cyan
    try {
        & $Command
        Write-Host "PASS: $Id" -ForegroundColor Green
    } catch {
        Write-Host "FAIL: $Id — $_" -ForegroundColor Red
        throw "smoke-checklist item $Id failed"
    }
}

# Item 17: config clamp (T-3) — fast, non-ignored.
Invoke-SmokeItem -Id '17' -Name 'Poll interval + top-N + window clamp' -Threshold 'T-3/T-21/T-22' -Command {
    cargo test -p sidebar-domain --lib config:: 2>$null | Select-String 'test result: ok' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'config clamp tests failed' }
}

# Item 1: cold-start (T-7) — non-ignored on Windows.
Invoke-SmokeItem -Id '1' -Name 'Cold-start <=2s Basic' -Threshold 'T-7' -Command {
    cargo test -p sidebar-app --test nfr_cold_start 2>$null | Select-String 'test result: ok' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'cold-start test failed' }
}

# Item 5: SQLite RSS (T-6) — non-ignored on Windows.
Invoke-SmokeItem -Id '5' -Name 'SQLite RSS <=6 MiB (2x ceiling)' -Threshold 'T-6' -Command {
    cargo test -p sidebar-app --test nfr_sqlite_rss 2>$null | Select-String 'test result: ok' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'SQLite RSS test failed' }
}

# Item 16: bandwidth persistence (R11) — non-ignored.
Invoke-SmokeItem -Id '16' -Name 'Bandwidth counter persists across restart' -Threshold 'R11' -Command {
    cargo test -p sidebar-bandwidth --lib restart_mid_cycle 2>$null | Select-String 'test result: ok' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'R11 persistence test failed' }
}

if (-not $SkipIgnored) {
    # Item 3: RSS p95 (T-4) — #[ignore] 30s smoke.
    Invoke-SmokeItem -Id '3' -Name 'Steady RSS <=80 MiB Basic' -Threshold 'T-4' -Command {
        cargo test -p sidebar-app --test nfr_rss -- --ignored 2>$null | Select-String 'test result: ok' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'RSS #[ignore] smoke failed' }
    }

    # Item 6: zero egress (G16) — #[ignore] 60s smoke.
    Invoke-SmokeItem -Id '6' -Name 'Zero runtime egress' -Threshold 'G16' -Command {
        cargo test -p sidebar-app --test runtime_no_egress -- --ignored 2>$null | Select-String 'test result: ok' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'egress #[ignore] smoke failed' }
    }
} else {
    Write-Host "`nSkipping #[ignore] smokes (items 3, 6) per -SkipIgnored." -ForegroundColor Yellow
}

Write-Host "`n=== All scriptable smoke items passed. ===" -ForegroundColor Green
Write-Host "Manual items (2, 4, 7-15, 18) must be walked by a human per verify/smoke-checklist.md." -ForegroundColor Yellow
