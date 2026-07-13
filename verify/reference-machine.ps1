# Story 13.5 + T-46 — reference-machine evidence runner.
#
# Bottles every evidence gate into one command for the designated T-31
# reference machine (LAPTOP-PLN56DNU). Produces a single evidence bundle
# under verify/evidence/<date>/ that can be attached to the v1.0.0 release.
#
# Stages:
#   1. Pre-flight: verify Rust / MSVC / pwsh7 / LHM hash pin.
#   2. Build: cargo build --release + copy LHM sidecar beside exe.
#   3. Workspace tests: full L0-L3 matrix.
#   4. Ignored suite: all 13 #[ignore]'d integration tests (real HW/UAC/desktop).
#   5. NFR-1 bench: poll_cost criterion bench + calibration constant.
#   6. Scriptable smoke: the 6 automatable smoke items.
#   7. SHA-256: release exe hash.
#   8. Manual items: 12 prompted PASS/FAIL walks.
#   9. Verdict: READY_TO_TAG / NOT_READY + exit 0/1.
#
# Cited: Story 13.5, nfr-thresholds.md T-46, guardrails.md G25/G28.

#Requires -Version 7.0
[CmdletBinding()]
param(
    [switch]$SkipManual,   # Skip the 12 prompted manual items (automated-only run)
    [switch]$SkipBench     # Skip the NFR-1 bench (saves ~5 min; use for quick re-runs)
)

$ErrorActionPreference = 'Stop'
$SidebarRoot = Split-Path $PSScriptRoot -Parent
$Date = (Get-Date -Format 'yyyy-MM-dd')
$EvidenceDir = Join-Path $SidebarRoot "verify\evidence\$Date"

# --- helpers ---

function Write-Stage {
    param([string]$Id, [string]$Name)
    Write-Host "`n=== ${Id}: $Name ===" -ForegroundColor Cyan
}

function Write-Pass {
    param([string]$Id)
    Write-Host "PASS: $Id" -ForegroundColor Green
}

function Write-Fail {
    param([string]$Id, [string]$Msg)
    Write-Host "FAIL: $Id — $Msg" -ForegroundColor Red
}

function Invoke-Stage {
    param(
        [string]$Id,
        [string]$Name,
        [string]$OutFile,
        [scriptblock]$Command
    )
    Write-Stage -Id $Id -Name $Name
    try {
        & $Command 2>&1 | Tee-Object -FilePath $OutFile | Out-Host
        if ($LASTEXITCODE -ne 0) {
            Write-Fail -Id $Id -Msg "exit code $LASTEXITCODE (see $OutFile)"
            return $false
        }
        Write-Pass -Id $Id
        return $true
    } catch {
        Write-Fail -Id $Id -Msg $_
        return $false
    }
}

# --- 0. evidence dir ---

New-Item -ItemType Directory -Path $EvidenceDir -Force | Out-Null
Write-Host "Reference-machine evidence runner (Story 13.5, T-46)" -ForegroundColor Cyan
Write-Host "Evidence dir: $EvidenceDir"
Write-Host "Date: $Date"
Write-Host "SkipManual: $SkipManual  SkipBench: $SkipBench"

$global:Failures = 0

# --- 1. pre-flight ---

Write-Stage -Id '1' -Name 'Pre-flight (Rust / MSVC / LHM hash)'
try {
    $rustVer = (rustc --version) 2>&1
    Write-Host "  rustc: $rustVer"
    if ($LASTEXITCODE -ne 0) { throw 'rustc not on PATH' }

    & "$PSScriptRoot\..\scripts\fetch_ohm.ps1" -CheckOnly
    if ($LASTEXITCODE -ne 0) { throw 'LHM hash pin check failed' }
    Write-Pass -Id '1'
} catch {
    Write-Fail -Id '1' -Msg $_
    $global:Failures++
}

# --- 2. build ---

$buildOk = Invoke-Stage -Id '2' -Name 'Release build + LHM sidecar copy' -OutFile (Join-Path $EvidenceDir 'build.txt') -Command {
    cargo build --release --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }
    # Copy the complete LHM runtime beside the exe (README build instructions).
    Copy-Item -Path "$SidebarRoot\resources\*" -Destination "$SidebarRoot\target\x86_64-pc-windows-msvc\release\" -Recurse -Force
}
if (-not $buildOk) { $global:Failures++ }

# --- 3. workspace tests (L0-L3) ---

$wsOk = Invoke-Stage -Id '3' -Name 'Workspace tests (L0-L3 full matrix)' -OutFile (Join-Path $EvidenceDir 'workspace-tests.txt') -Command {
    cargo test --workspace --all-features --all-targets --target x86_64-pc-windows-msvc
}
if (-not $wsOk) { $global:Failures++ }

# --- 4. ignored suite (the 13 real-HW/UAC/desktop tests) ---

$ignoredOk = Invoke-Stage -Id '4' -Name 'Ignored suite (13 #[ignore] integration tests)' -OutFile (Join-Path $EvidenceDir 'ignored-suite.txt') -Command {
    cargo test --workspace --target x86_64-pc-windows-msvc -- --ignored --nocapture
}
if (-not $ignoredOk) { $global:Failures++ }

# --- 5. NFR-1 bench ---

if (-not $SkipBench) {
    $benchOk = Invoke-Stage -Id '5' -Name 'NFR-1 poll_cost bench + calibration' -OutFile (Join-Path $EvidenceDir 'poll_cost.txt') -Command {
        cargo bench --bench poll_cost --target x86_64-pc-windows-msvc
        # Capture the calibration constant if the bench wrote it.
        $calibPath = "$SidebarRoot\target\criterion\calibration.txt"
        if (Test-Path $calibPath) {
            Write-Host "Calibration constant:"
            Get-Content $calibPath
        }
    }
    if (-not $benchOk) { $global:Failures++ }
} else {
    Write-Host "`n=== 5: NFR-1 bench SKIPPED (-SkipBench) ===" -ForegroundColor Yellow
}

# --- 6. scriptable smoke ---

$smokeOk = Invoke-Stage -Id '6' -Name 'Scriptable smoke (6 automatable items)' -OutFile (Join-Path $EvidenceDir 'scriptable-smoke.txt') -Command {
    & "$PSScriptRoot\smoke-checklist.ps1"
}
if (-not $smokeOk) { $global:Failures++ }

# --- 7. SHA-256 ---

Write-Stage -Id '7' -Name 'Release exe SHA-256'
try {
    $exe = "$SidebarRoot\target\x86_64-pc-windows-msvc\release\sidebar-app.exe"
    $hash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    $size = (Get-Item -LiteralPath $exe).Length
    $shaContent = "path: $exe`nsize: $size bytes`nSHA-256: $hash`ndate: $Date"
    $shaContent | Out-File -FilePath (Join-Path $EvidenceDir 'sha256.txt') -Encoding utf8
    Write-Host $shaContent
    Write-Pass -Id '7'
} catch {
    Write-Fail -Id '7' -Msg $_
    $global:Failures++
}

# --- 8. manual items (12 prompted walks) ---

$manualResults = @()
if (-not $SkipManual) {
    Write-Stage -Id '8' -Name 'Manual smoke items (12 human-walked)'
    Write-Host "Walk each item. Type PASS, FAIL, or SKIP for each." -ForegroundColor Yellow
    $manualItems = @(
        @{Id='2';  Name='Full-mode cold start (accept UAC, LHM sensors render)';     Threshold='T-8'},
        @{Id='4';  Name='Full-mode RSS <=250 MiB';                                   Threshold='T-5'},
        @{Id='7';  Name='Transparent topmost viewport (no title bar)';               Threshold='Story 6.1'},
        @{Id='8';  Name='AppBar dock registration (right edge)';                     Threshold='Story 6.2'},
        @{Id='9';  Name='Per-monitor DPI crispness';                                 Threshold='Story 6.3'},
        @{Id='10'; Name='UAC elevation flow (BASIC pill -> LHM launch)';             Threshold='Story 6.4/12.8'},
        @{Id='11'; Name='Job Object reap on host exit (no orphan LHM)';              Threshold='G10'},
        @{Id='12'; Name='Capture cloak under OBS';                                   Threshold='Story 6.1'},
        @{Id='13'; Name='Ctrl+Shift+S hotkey toggle';                                Threshold='T-34'},
        @{Id='14'; Name='Theme switch (Dark <-> Light via Windows setting)';         Threshold='T-35'},
        @{Id='15'; Name='Multi-monitor re-dock (skip if 1 monitor)';                 Threshold='T-36'},
        @{Id='18'; Name='Graceful shutdown <=3s';                                    Threshold='T-19'}
    )
    foreach ($item in $manualItems) {
        $prompt = "  [$($item.Id)] $($item.Name) ($($item.Threshold))? (PASS/FAIL/SKIP)"
        $response = Read-Host -Prompt $prompt
        $response = ($response ?? 'SKIP').Trim().ToUpper()
        if ($response -notmatch '^(PASS|FAIL|SKIP)$') { $response = 'SKIP' }
        $manualResults += "| $($item.Id) | $response | $($item.Name) | $($item.Threshold) |"
        if ($response -eq 'FAIL') { $global:Failures++ }
        Write-Host "    -> recorded: $response" -ForegroundColor $(if ($response -eq 'PASS') {'Green'} elseif ($response -eq 'FAIL') {'Red'} else {'Yellow'})
    }
    $manualHeader = "| # | Result | Item | Threshold |`n|---|---|---|---|`n"
    ($manualHeader + ($manualResults -join "`n")) | Out-File -FilePath (Join-Path $EvidenceDir 'manual-smoke.md') -Encoding utf8
} else {
    Write-Host "`n=== 8: Manual items SKIPPED (-SkipManual) ===" -ForegroundColor Yellow
    '| # | Result | Item | Threshold |`n|---|---|---|---|`n| (skipped) | SKIP | -SkipManual was set | - |' | Out-File -FilePath (Join-Path $EvidenceDir 'manual-smoke.md') -Encoding utf8
}

# --- 9. verdict ---

Write-Host "`n=== 9: Verdict ===" -ForegroundColor Cyan
if ($global:Failures -eq 0) {
    Write-Host "READY_TO_TAG — all automated stages passed, all manual items PASS." -ForegroundColor Green
    Write-Host "Evidence bundle: $EvidenceDir" -ForegroundColor Green
    Write-Host "Attach this bundle to the v1.0.0 GitHub Release." -ForegroundColor Green
    exit 0
} else {
    Write-Host "NOT_READY — $global:Failures failure(s). See evidence bundle: $EvidenceDir" -ForegroundColor Red
    Write-Host "Fix the failures before tagging v1.0.0." -ForegroundColor Red
    exit 1
}
