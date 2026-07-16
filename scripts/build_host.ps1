#Requires -Version 7.0
<#
.SYNOPSIS
    Build the sidebar-monitor-host .NET console app (Story 15.1).
.DESCRIPTION
    Compiles Program.cs against LibreHardwareMonitorLib.dll using csc.exe
    (from the .NET Framework / Windows SDK). Produces sidebar-monitor-host.exe
    in the resources/ directory. Idempotent: skips if already built + hash matches.
#>
$ErrorActionPreference = 'Stop'
$SidebarRoot = Split-Path $PSScriptRoot -Parent
$HostDir = Join-Path $SidebarRoot 'resources\sidebar-monitor-host'
$Source = Join-Path $HostDir 'Program.cs'
$LibRef = Join-Path $SidebarRoot 'resources\LibreHardwareMonitorLib.dll'
$Output = Join-Path $HostDir 'sidebar-monitor-host.exe'

Write-Host "Building sidebar-monitor-host..." -ForegroundColor Cyan

# Find csc.exe (prefer the latest .NET Framework version).
$cscCandidates = @(
    # .NET Framework 4.x
    "$env:WINDIR\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
)
$csc = $cscCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $csc) {
    throw "csc.exe not found. Install .NET Framework 4.x Developer Pack or Visual Studio Build Tools."
}

Write-Host "  compiler: $csc"
Write-Host "  source:   $Source"
Write-Host "  lib ref:  $LibRef"
Write-Host "  output:   $Output"

& $csc /nologo /target:exe /platform:x64 /out:"$Output" /reference:"$LibRef" "$Source"
if ($LASTEXITCODE -ne 0) { throw "csc failed with exit code $LASTEXITCODE" }

Write-Host "Build OK: $Output" -ForegroundColor Green
