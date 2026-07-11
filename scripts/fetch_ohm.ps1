#Requires -Version 7.0
<#
.SYNOPSIS
    Download and verify the pinned LibreHardwareMonitor (LHM) binary.
.DESCRIPTION
    Downloads LHM v0.9.6 into the repository's resources directory, validates
    the downloaded executable against the committed resources/ohm.sha256 pin,
    and then copies the package plus MPL-2.0 license into resources/.

    Idempotent: a matching executable and license skip the network request.
    A failed download or staged hash mismatch leaves the existing resources
    untouched.

.PARAMETER Force
    Re-download even when the existing binary already matches the pin.

.PARAMETER CheckOnly
    Validate the local executable against resources/ohm.sha256 without using
    the network. Returns a non-zero exit code on a missing or mismatched file.
#>

[CmdletBinding()]
param(
    [switch]$Force,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$SidebarRoot = Split-Path $PSScriptRoot -Parent
$Resources   = Join-Path $SidebarRoot 'resources'
$LhmExe      = Join-Path $Resources 'LibreHardwareMonitor.exe'
$HashFile    = Join-Path $Resources 'ohm.sha256'
$LicenseFile = Join-Path $Resources 'LibreHardwareMonitor.LICENSE.txt'
$Version     = '0.9.6' # HTTP /data.json build; update this and ohm.sha256 together.

function Get-PinnedHash {
    if (-not (Test-Path -LiteralPath $HashFile -PathType Leaf)) {
        throw "Missing SHA-256 pin: $HashFile"
    }

    $line = (Get-Content -LiteralPath $HashFile -TotalCount 1).Trim()
    $match = [regex]::Match($line, '^(?<hash>[0-9a-fA-F]{64})\s+LibreHardwareMonitor\.exe$')
    if (-not $match.Success) {
        throw "Invalid SHA-256 pin format in $HashFile (expected: 64-hex + filename)"
    }
    return $match.Groups['hash'].Value.ToLowerInvariant()
}

function Get-ActualHash([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $null }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Hash([string]$Path, [string]$Expected) {
    $actual = Get-ActualHash $Path
    if (-not $actual) { throw "LHM executable not found: $Path" }
    if ($actual -ne $Expected) {
        throw "SHA-256 mismatch for $Path (expected $Expected, got $actual)"
    }
    return $actual
}

$expectedHash = Get-PinnedHash
if ($CheckOnly) {
    [void](Assert-Hash $LhmExe $expectedHash)
    if (-not (Test-Path -LiteralPath $LicenseFile -PathType Leaf)) {
        throw "LHM license missing: $LicenseFile"
    }
    Write-Host "LHM local files match the v$Version pin." -ForegroundColor Green
    return
}

$actualHash = Get-ActualHash $LhmExe
if ($actualHash -and ($actualHash -eq $expectedHash) -and (Test-Path -LiteralPath $LicenseFile -PathType Leaf) -and -not $Force) {
    Write-Host "LHM v$Version already present, hash matches. Skipping download." -ForegroundColor Green
    Write-Host "  Path: $LhmExe"
    Write-Host "  SHA-256: $actualHash"
    return
}

if (-not (Test-Path -LiteralPath $Resources -PathType Container)) {
    New-Item -ItemType Directory -Path $Resources | Out-Null
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) "sidebar-lhm-$PID"
$zipPath = Join-Path ([IO.Path]::GetTempPath()) "sidebar-lhm-$PID.zip"
$licenseTemp = Join-Path $tempRoot 'LibreHardwareMonitor.LICENSE.txt'
$zipUrl = "https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/download/v$Version/LibreHardwareMonitor.zip"
$licenseUrl = 'https://raw.githubusercontent.com/LibreHardwareMonitor/LibreHardwareMonitor/master/LICENSE'

try {
    Remove-Item -LiteralPath $tempRoot, $zipPath -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $tempRoot | Out-Null

    Write-Host "Downloading LHM v$Version from:" -ForegroundColor Cyan
    Write-Host "  $zipUrl"
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath -TimeoutSec 30

    $extractRoot = Join-Path $tempRoot 'extract'
    Expand-Archive -LiteralPath $zipPath -DestinationPath $extractRoot -Force
    $stagedExe = Get-ChildItem -LiteralPath $extractRoot -Filter 'LibreHardwareMonitor.exe' -File -Recurse | Select-Object -First 1
    if (-not $stagedExe) { throw 'Archive did not contain LibreHardwareMonitor.exe' }

    # Verify before touching resources: a bad archive never replaces a good install.
    [void](Assert-Hash $stagedExe.FullName $expectedHash)

    if ($Force -or -not (Test-Path -LiteralPath $LicenseFile -PathType Leaf)) {
        Invoke-WebRequest -Uri $licenseUrl -OutFile $licenseTemp -TimeoutSec 30
    }

    $packageRoot = $stagedExe.Directory.FullName
    Get-ChildItem -LiteralPath $packageRoot -Force | Copy-Item -Destination $Resources -Recurse -Force
    if (Test-Path -LiteralPath $licenseTemp -PathType Leaf) {
        Copy-Item -LiteralPath $licenseTemp -Destination $LicenseFile -Force
    }

    $finalHash = Assert-Hash $LhmExe $expectedHash
    if (-not (Test-Path -LiteralPath $LicenseFile -PathType Leaf)) {
        throw "LHM license missing after extraction: $LicenseFile"
    }

    Write-Host "LHM v$Version installed successfully (hash verified)." -ForegroundColor Green
    Write-Host "  Binary:  $LhmExe"
    Write-Host "  SHA-256: $finalHash"
    Write-Host "  License: $LicenseFile (MPL-2.0, T-32-allowed)"
} catch {
    Write-Error "LHM acquisition failed: $($_.Exception.Message)"
    exit 1
} finally {
    Remove-Item -LiteralPath $tempRoot, $zipPath -Recurse -Force -ErrorAction SilentlyContinue
}
