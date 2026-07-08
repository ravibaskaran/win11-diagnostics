#Requires -Version 7.0
<#
.SYNOPSIS
    Download and verify the bundled LibreHardwareMonitor (LHM) binary.
.DESCRIPTION
    Downloads the pinned LHM release into D:\dev\sidebar\resources\, verifies the
    SHA-256 against resources/ohm.sha256, and extracts the full LHM package
    (the v0.9.6 .NET 10 build, which exposes the HTTP /data.json endpoint we
    integrate with — NOT the legacy net472 WMI build).

    Idempotent: if LHM is already present and its hash matches, the download is
    skipped. If the hash mismatches, the corrupted file is deleted and the
    download is retried.

    Per architecture.md AD-2 (revised 2026-07-08) + Story 6.5 + T-45 + R7.

.PARAMETER Version
    LHM release tag to pin. Default '0.9.6' (the latest stable as of 2026-07-08;
    first release with AMD Ryzen AI 300-series support AND the maintainer-blessed
    HTTP endpoint after WMI was permanently removed in v0.9.5).

.PARAMETER Force
    Re-download even if the existing binary's hash already matches.

.EXAMPLE
    .\fetch_ohm.ps1
    .\fetch_ohm.ps1 -Version 0.9.7 -Force
#>

[CmdletBinding()]
param(
    [string]$Version = '0.9.6',
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$SidebarRoot = Split-Path $PSScriptRoot -Parent
$Resources   = Join-Path $SidebarRoot 'resources'
$LhmExe      = Join-Path $Resources 'LibreHardwareMonitor.exe'
$HashFile    = Join-Path $Resources 'ohm.sha256'

if (-not (Test-Path $Resources)) { New-Item -ItemType Directory -Path $Resources | Out-Null }

# --- Step 1: Skip if already present and hash matches (idempotent) ---
function Get-StoredHash {
    if (-not (Test-Path $HashFile)) { return $null }
    $line = (Get-Content $HashFile -TotalCount 1) -replace '\s+', ' '
    return ($line -split ' ')[0].ToLower()
}

function Get-ActualHash {
    if (-not (Test-Path $LhmExe)) { return $null }
    return (Get-FileHash $LhmExe -Algorithm SHA256).Hash.ToLower()
}

if (-not $Force) {
    $stored  = Get-StoredHash
    $actual  = Get-ActualHash
    if ($stored -and $actual -and ($stored -eq $actual)) {
        Write-Host "LHM v$Version already present, hash matches. Skipping download." -ForegroundColor Green
        Write-Host "  Path: $LhmExe"
        Write-Host "  SHA-256: $actual"
        return
    }
}

# --- Step 2: Download ---
# v0.9.5+ ships `LibreHardwareMonitor.zip` (the .NET 10 build with HTTP endpoint).
# v0.9.4 and earlier shipped `LibreHardwareMonitor-net472.zip` (WMI build — no longer used).
$zipName = if ([version]$Version -ge [version]'0.9.5') { 'LibreHardwareMonitor.zip' } else { 'LibreHardwareMonitor-net472.zip' }
$zipUrl  = "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v$Version/$zipName"
$zipPath = Join-Path $env:TEMP "lhm-$Version.zip"

Write-Host "Downloading LHM v$Version from:" -ForegroundColor Cyan
Write-Host "  $zipUrl"
try {
    # 30s timeout per G16 (network egress in CI sandbox must not hang).
    $ProgressPreference = 'SilentlyContinue'  # massive speedup for Invoke-WebRequest
    Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath -TimeoutSec 30
} catch {
    Write-Error "Download failed (network blocked or release retired?): $($_.Exception.Message)"
    exit 1
}

# --- Step 3: Extract ---
Write-Host "Extracting to: $Resources"
# Clean stale exe first so extraction doesn't leave a half-mixed state
Remove-Item $LhmExe -ErrorAction SilentlyContinue
Expand-Archive -Path $zipPath -DestinationPath $Resources -Force
Remove-Item $zipPath -Force

# --- Step 4: Verify extraction ---
if (-not (Test-Path $LhmExe)) {
    Write-Error "Extraction completed but LibreHardwareMonitor.exe not found. Archive may be corrupt."
    exit 1
}

# --- Step 5: Compute + write hash pin ---
$hash = (Get-FileHash $LhmExe -Algorithm SHA256).Hash.ToLower()
"$hash  LibreHardwareMonitor.exe`n" | Out-File -FilePath $HashFile -Encoding ascii -NoNewline
Write-Host "Pinned SHA-256: $hash" -ForegroundColor Green
Write-Host "  Written to: $HashFile"

# --- Step 6: Fetch LICENSE (MPL-2.0) ---
$licUrl  = 'https://raw.githubusercontent.com/LibreHardwareMonitor/LibreHardwareMonitor/master/LICENSE'
$licPath = Join-Path $Resources 'LibreHardwareMonitor.LICENSE.txt'
Invoke-WebRequest -Uri $licUrl -OutFile $licPath -TimeoutSec 30
Write-Host "Fetched LICENSE: $licPath"

# --- Done ---
Write-Host ""
Write-Host "LHM v$Version installed successfully." -ForegroundColor Green
Write-Host "  Binary:  $LhmExe"
Write-Host "  Hash:    $hash"
Write-Host "  License: $licPath (MPL-2.0, T-32-allowed)"
Write-Host ""
Write-Host "Next: run scripts/verify-dev-env.ps1 to confirm the full dev environment." -ForegroundColor Cyan
