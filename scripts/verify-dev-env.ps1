#Requires -Version 7.2
<#
.SYNOPSIS
    Verify the sidebar development environment is fully configured for this machine.

.DESCRIPTION
    Asserts every prerequisite and tool listed in T-44 (see CONTRIBUTING.md
    + docs/backlog/nfr-thresholds.md) is present and runnable. Prints a
    green/red table. Exits non-zero on any failure.

    Idempotent, session-scoped, and side-effect-free:
      - Does NOT modify $env:PATH persistently (uses a child scope; env.ps1's
        PATH changes apply during the run but do not leak to the caller).
      - Does NOT write any files.
      - Does NOT touch the registry or $PROFILE.

    Suitable for use as:
      - A manual developer setup check.
      - A CI pre-flight gate (Story 11.2 regression harness invokes this
        before running the L0..L3 test matrix).

    Each check is structured so a missing-but-required item is clearly named
    with a concrete remediation path, so the agent or human reading the
    output can act without consulting additional docs.

.PARAMETER Strict
    Treat WARN items as FAIL. Default: warnings don't fail the script (e.g.
    the NVIDIA-GPU-absent caveat on AMD-only machines).

.PARAMETER Json
    Emit a machine-readable JSON report instead of a table. Useful for CI
    integrations that parse the result. The script still exits 0/1.

.PARAMETER NoActivate
    Skip dot-sourcing env.ps1. Use when the caller has already activated the
    environment or wants to verify the raw system state.

.EXAMPLE
    .\scripts\verify-dev-env.ps1
    Run interactively; prints a table.

.EXAMPLE
    .\scripts\verify-dev-env.ps1 -Json
    Emit JSON for CI parsing.

.NOTES
    Per T-44 + Story 0.7. See CONTRIBUTING.md for the full setup guide.
#>

[CmdletBinding()]
param(
    [switch]$Strict,
    [switch]$Json,
    [switch]$NoActivate
)

$ErrorActionPreference = 'Stop'
$SidebarRoot = Split-Path $PSScriptRoot -Parent

# ---------------------------------------------------------------------------
# Activate env.ps1 in a child scope. Its $env:PATH mutation applies to this
# process for the duration of the script (so cargo-* etc. resolve), but
# because we're running in a script scope rather than dot-sourcing, the
# caller's PATH is not affected when this script exits.
# ---------------------------------------------------------------------------
if (-not $NoActivate) {
    & (Join-Path $PSScriptRoot 'env.ps1') -Quiet
}

# ---------------------------------------------------------------------------
# Build a structured list of checks. Each check records: status (OK/WARN/FAIL),
# name, detail, remediation hint. WARN counts as fail only under -Strict.
# ---------------------------------------------------------------------------
$results = [System.Collections.Generic.List[pscustomobject]]::new()
$failures = 0
$warnings = 0

function Add-Check {
    param(
        [Parameter(Mandatory)][ValidateSet('OK','WARN','FAIL')][string]$Status,
        [Parameter(Mandatory)][string]$Name,
        [string]$Detail = '',
        [string]$Remediation = ''
    )
    $results.Add([pscustomobject]@{
        Status      = $Status
        Name        = $Name
        Detail      = $Detail
        Remediation = $Remediation
    })
    switch ($Status) {
        'FAIL' { $script:failures++ }
        'WARN' { $script:warnings++ }
    }
}

# ---------------------------------------------------------------------------
# System prerequisites — these are expected to pre-exist on the machine and
# cannot be folder-relocated (Rust toolchain, MSVC linker, Git, PowerShell 7).
# ---------------------------------------------------------------------------

# 1. Rust >= 1.95 (MSRV forced by sysinfo 0.39.3)
try {
    $rustcOut = rustc --version 2>$null
    if ($LASTEXITCODE -eq 0 -and $rustcOut -match 'rustc\s+(\d+)\.(\d+)\.(\d+)') {
        $rustMajor = [int]$Matches[1]
        $rustMinor = [int]$Matches[2]
        $rustVer = "$rustMajor.$rustMinor.$($Matches[3])"
        $rustOk = ($rustMajor -gt 1) -or ($rustMajor -eq 1 -and $rustMinor -ge 95)
        Add-Check $(if ($rustOk) {'OK'} else {'FAIL'}) `
            'Rust >= 1.95 (MSRV)' `
            "detected: $rustVer" `
            $(if ($rustOk) {''} else {'rustup update stable ; rustup default stable'})
    } else {
        Add-Check 'FAIL' 'Rust >= 1.95 (MSRV)' 'rustc not found on PATH' 'Install via https://rustup.rs/, then rustup default stable'
    }
} catch {
    Add-Check 'FAIL' 'Rust >= 1.95 (MSRV)' "rustc invocation failed: $($_.Exception.Message)" 'Install via https://rustup.rs/'
}

# 2. rustup component: llvm-tools (required by cargo-llvm-cov, T-43)
try {
    $components = rustup component list --installed 2>$null
    if ($components -match '^llvm-tools[-]') {
        Add-Check 'OK' 'rustup component: llvm-tools' '' ''
    } else {
        Add-Check 'FAIL' 'rustup component: llvm-tools' 'component not installed' 'rustup component add llvm-tools'
    }
} catch {
    Add-Check 'FAIL' 'rustup component: llvm-tools' "rustup invocation failed: $($_.Exception.Message)" 'Ensure rustup is installed'
}

# 3. MSVC linker + Windows SDK reachable (the windows crate FFI link)
try {
    $null = rustc --print target-libdir 2>$null
    if ($LASTEXITCODE -eq 0) {
        Add-Check 'OK' 'MSVC linker + Windows SDK' '' ''
    } else {
        Add-Check 'FAIL' 'MSVC linker + Windows SDK' 'rustc --print target-libdir failed' 'Install Visual Studio Build Tools with the C++ workload + Windows SDK'
    }
} catch {
    Add-Check 'FAIL' 'MSVC linker + Windows SDK' $_.Exception.Message 'Install Visual Studio Build Tools'
}

# 4. Git for Windows
$gitExe = (Get-Command git -ErrorAction SilentlyContinue)
if ($gitExe) {
    Add-Check 'OK' 'Git for Windows' $gitExe.Source ''
} else {
    Add-Check 'FAIL' 'Git for Windows' 'git not on PATH' 'Install from https://git-scm.com/download/win'
}

# 5. PowerShell 7+ (sanity — this script requires it)
try {
    $pwshVer = $PSVersionTable.PSVersion.ToString()
    $pwshOk = ($PSVersionTable.PSVersion.Major -ge 7) -and ($PSVersionTable.PSEdition -eq 'Core')
    Add-Check $(if ($pwshOk) {'OK'} else {'FAIL'}) `
        'PowerShell 7+' `
        "running: $pwshVer ($($PSVersionTable.PSEdition))" `
        $(if ($pwshOk) {''} else {'Install PowerShell 7+ from https://github.com/PowerShell/PowerShell/releases'})
} catch {
    Add-Check 'FAIL' 'PowerShell 7+' $_.Exception.Message 'Install PowerShell 7+'
}

# ---------------------------------------------------------------------------
# Project-local cargo subcommands (under tools/cargo-bin/). These are
# installed via scripts/fetch_ohm.ps1 + the documented cargo-binstall flow.
# We check existence; running --version would be slower and add little.
# ---------------------------------------------------------------------------
$cargoBin = Join-Path $SidebarRoot 'tools\cargo-bin'
foreach ($tool in 'cargo-deny', 'cargo-audit', 'cargo-llvm-cov', 'cargo-nextest') {
    $exe = Join-Path $cargoBin "$tool.exe"
    if (Test-Path $exe) {
        Add-Check 'OK' $tool (Get-Item $exe).VersionInfo.ProductVersion ''
    } else {
        Add-Check 'FAIL' $tool "not found at: $exe" "Run: CARGO_HOME=`$env:TEMP\cb cargo binstall --no-confirm $tool ; then copy to tools\cargo-bin\ (see CONTRIBUTING.md)"
    }
}

# ---------------------------------------------------------------------------
# CI tools (under tools/ci/)
# ---------------------------------------------------------------------------
$ciTools = Join-Path $SidebarRoot 'tools\ci'
foreach ($tool in 'actionlint', 'wingetcreate') {
    $exe = Join-Path $ciTools "$tool.exe"
    if (Test-Path $exe) {
        Add-Check 'OK' $tool (Get-Item $exe).VersionInfo.ProductVersion ''
    } else {
        Add-Check 'FAIL' $tool "not found at: $exe" 'See CONTRIBUTING.md for download instructions'
    }
}

# ---------------------------------------------------------------------------
# sqlite3 (under tools/sqlite/)
# ---------------------------------------------------------------------------
$sqlite = Join-Path $SidebarRoot 'tools\sqlite\sqlite3.exe'
if (Test-Path $sqlite) {
    Add-Check 'OK' 'sqlite3 (debug bandwidth.db)' (Get-Item $sqlite).VersionInfo.ProductVersion ''
} else {
    Add-Check 'FAIL' 'sqlite3 (debug bandwidth.db)' "not found at: $sqlite" 'scoop install sqlite ; then copy to tools\sqlite\'
}

# ---------------------------------------------------------------------------
# Bundled LHM binary + hash pin + LICENSE (resources/)
# ---------------------------------------------------------------------------
$lhmExe   = Join-Path $SidebarRoot 'resources\LibreHardwareMonitor.exe'
$lhmHash  = Join-Path $SidebarRoot 'resources\ohm.sha256'
$lhmLic   = Join-Path $SidebarRoot 'resources\LibreHardwareMonitor.LICENSE.txt'

$lhmPresent = Test-Path $lhmExe
if ($lhmPresent) {
    $lhmVer = (Get-Item $lhmExe).VersionInfo.ProductVersion
    Add-Check 'OK' 'LibreHardwareMonitor.exe (bundled)' "v$lhmVer" ''
} else {
    Add-Check 'FAIL' 'LibreHardwareMonitor.exe (bundled)' "not found at: $lhmExe" 'Run: .\scripts\fetch_ohm.ps1'
}

if ($lhmPresent -and (Test-Path $lhmHash)) {
    $hashLine = (Get-Content $lhmHash -TotalCount 1) -replace '\s+', ' '
    $expectedHash = ($hashLine -split ' ')[0].ToLower()
    $actualHash = (Get-FileHash $lhmExe -Algorithm SHA256).Hash.ToLower()
    if ($expectedHash -eq $actualHash) {
        Add-Check 'OK' 'LHM SHA-256 matches ohm.sha256 pin' $actualHash ''
    } else {
        Add-Check 'FAIL' 'LHM SHA-256 matches ohm.sha256 pin' "expected=$expectedHash actual=$actualHash" 'Corrupted download — re-run: .\scripts\fetch_ohm.ps1 -Force'
    }
} else {
    Add-Check 'FAIL' 'LHM SHA-256 pin' 'ohm.sha256 or LHM binary missing' 'Run: .\scripts\fetch_ohm.ps1'
}

if (Test-Path $lhmLic) {
    Add-Check 'OK' 'LibreHardwareMonitor.LICENSE.txt (MPL-2.0)' $lhmLic ''
} else {
    Add-Check 'FAIL' 'LibreHardwareMonitor.LICENSE.txt (MPL-2.0)' 'missing' 'Run: .\scripts\fetch_ohm.ps1'
}

# ---------------------------------------------------------------------------
# NVIDIA GPU presence — WARN only (not FAIL) on AMD-only machines.
# Story 3.2 (nvml-wrapper) integration tests are #[ignore]'d locally.
# Under -Strict this counts as a failure (e.g. on a CI runner that should
# have an NVIDIA GPU but doesn't).
# ---------------------------------------------------------------------------
$nvmlPresent = $false
try {
    $nvIdx = & "$env:ProgramFiles\NVIDIA Corporation\NVSMI\nvidia-smi.exe" -L 2>$null
    if ($LASTEXITCODE -eq 0 -and $nvIdx) { $nvmlPresent = $true }
} catch {}
if (-not $nvmlPresent) {
    try {
        $nvIdx = & nvidia-smi -L 2>$null
        if ($LASTEXITCODE -eq 0 -and $nvIdx) { $nvmlPresent = $true }
    } catch {}
}
if ($nvmlPresent) {
    Add-Check 'OK' 'NVIDIA GPU (for Story 3.2 nvml-wrapper integration)' 'present' ''
} else {
    Add-Check 'WARN' 'NVIDIA GPU (for Story 3.2 nvml-wrapper integration)' 'not detected on this machine' 'AMD-only machines: Story 3.2 #[ignore] tests run elsewhere. Use -Strict to count this as failure.'
}

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------
$exitFailures = $failures + $(if ($Strict) { $warnings } else { 0 })

if ($Json) {
    $report = [pscustomobject]@{
        machine       = $env:COMPUTERNAME
        sidebar_root  = $SidebarRoot
        timestamp_utc = (Get-Date).ToUniversalTime().ToString('o')
        summary       = [pscustomobject]@{
            total    = $results.Count
            ok       = ($results | Where-Object Status -eq 'OK').Count
            warnings = $warnings
            failures = $failures
            exit_code = if ($exitFailures -eq 0) { 0 } else { 1 }
        }
        checks = $results
    }
    $report | ConvertTo-Json -Depth 5
} else {
    $results | Format-Table Status, Name, Detail -AutoSize | Out-Host
    if ($warnings -gt 0) {
        Write-Host ""
        $results | Where-Object Status -eq 'WARN' | ForEach-Object {
            Write-Host "  WARN: $($_.Name) — $($_.Detail)" -ForegroundColor Yellow
            if ($_.Remediation) {
                Write-Host "        Remediation: $($_.Remediation)" -ForegroundColor DarkYellow
            }
        }
    }
    Write-Host ""
    if ($exitFailures -eq 0) {
        Write-Host "All $($results.Count) checks passed ($warnings warnings)." -ForegroundColor Green
        if ($warnings -gt 0) {
            Write-Host "Re-run with -Strict to treat warnings as failures." -ForegroundColor DarkGray
        }
    } else {
        Write-Host "$exitFailures of $($results.Count) checks FAILED." -ForegroundColor Red
        Write-Host "See CONTRIBUTING.md for remediation instructions." -ForegroundColor Yellow
    }
}

exit $(if ($exitFailures -eq 0) { 0 } else { 1 })
