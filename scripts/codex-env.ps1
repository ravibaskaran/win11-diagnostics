#Requires -Version 7.2
<#
.SYNOPSIS
    Codex-specific dev environment activator for the sidebar workspace.

.DESCRIPTION
    Codex's managed PowerShell host runs under shell_environment_policy.inherit = "none"
    (see config.toml), which strips USERPROFILE, HOME, HOMEDRIVE, HOMEPATH, RUSTUP_HOME,
    and CARGO_HOME from the spawned shell. Without those, cargo cannot locate the
    toolchain and rustc cannot locate the MSVC linker.

    This script re-establishes the minimum environment needed for the Rust workspace
    to build and test from inside Codex. It is session-scoped: nothing is written to
    the registry, $PROFILE, or any global config file. It is idempotent and safe to
    dot-source in every Codex session.

    Responsibilities (per the user-facing contract):
      1. Set USERPROFILE, HOME, RUSTUP_HOME, CARGO_HOME to the user's real paths.
      2. Load the MSVC x64 developer environment (VsDevCmd.bat) so link.exe and
         Windows SDK headers/libraries are on PATH/LIB/INCLUDE.
      3. Configure writable TEMP/TMP and CARGO_TARGET_DIR only if the sandbox
         actually permits writes to the chosen location. Falls back gracefully.
      4. Never read, print, persist, or modify GitHub tokens or credentials.
      5. Fail loudly and clearly when the sandbox denies linker/temp writes —
         we never silently claim success.

    Token safety contract:
      - This script does NOT touch GH_TOKEN, GITHUB_TOKEN, GH_ENTERPRISE_TOKEN,
        %APPDATA%\GitHub CLI\hosts.yml, or the Windows Credential Manager.
      - It does NOT call gh auth login or any command that would persist creds.
      - Codex must supply an already-approved GH_TOKEN in its own env, or rely
        on the host's existing keyring auth. See notes in the GH section below.

.PARAMETER Quiet
    Suppress informational output. Errors and sandbox-denial failures still print.

.PARAMETER SkipMsvc
    Skip loading VsDevCmd.bat. Use only when you know link.exe is already on PATH
    (e.g. the calling shell was launched from a Developer PowerShell).

.PARAMETER NoTargetRedirect
    Do not redirect CARGO_TARGET_DIR. By default we point cargo at .\target\ inside
    the workspace (always sandbox-writable) instead of leaving it at the default
    $CARGO_HOME\target (which may be outside the writable root set).

.EXAMPLE
    . .\scripts\codex-env.ps1
    Dot-source into the current Codex PowerShell session.

.EXAMPLE
    . .\scripts\codex-env.ps1 -Quiet
    Same, but suppress the informational banner.

.NOTES
    Companion to scripts/env.ps1 (which handles the project-local tools\* PATH).
    Run order inside Codex:
        . .\scripts\env.ps1        # project tools
        . .\scripts\codex-env.ps1  # this script — Rust + MSVC + TEMP
        .\scripts\verify-dev-env.ps1 -NoActivate
#>

[CmdletBinding()]
param(
    [switch]$Quiet,
    [switch]$SkipMsvc,
    [switch]$NoTargetRedirect
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Locate the sidebar root from $PSScriptRoot (this file lives in scripts/).
# ---------------------------------------------------------------------------
if (-not $PSScriptRoot) {
    $PSScriptRoot = Split-Path $MyInvocation.MyCommand.Path -Parent
}
$SidebarRoot = Split-Path $PSScriptRoot -Parent

# ---------------------------------------------------------------------------
# 1. USERPROFILE / HOME / RUSTUP_HOME / CARGO_HOME
#
# Under Codex's shell_environment_policy.inherit = "none", $env:USERPROFILE is
# null. We resolve the real path via the OS API ([Environment]::GetFolderPath)
# which queries the registry-backed user-profile path, not the env block.
# Same technique as Verify-Codex-Sandbox.ps1 — proven to work under the
# sandbox where $env:USERPROFILE is stripped.
# ---------------------------------------------------------------------------
function Resolve-UserProfileSafe {
    if (-not [string]::IsNullOrEmpty($env:USERPROFILE) -and (Test-Path $env:USERPROFILE)) {
        return $env:USERPROFILE
    }
    $fallback = [Environment]::GetFolderPath('UserProfile')
    if (-not [string]::IsNullOrEmpty($fallback) -and (Test-Path $fallback)) {
        return $fallback
    }
    throw "Cannot resolve user profile. Set `$env:USERPROFILE manually before running scripts/codex-env.ps1."
}

$UserProfile = Resolve-UserProfileSafe

# Set the four required variables. These are the user's REAL paths — not
# project-local copies — because the Rust toolchain is installed there.
$env:USERPROFILE = $UserProfile
$env:HOME        = $UserProfile          # git, cargo, and many crates look at HOME on Windows
$env:RUSTUP_HOME = Join-Path $UserProfile '.rustup'
$env:CARGO_HOME  = Join-Path $UserProfile '.cargo'

# Prepend cargo's bin dir to PATH so cargo/rustc/rustup resolve even when
# Codex's preserve_paths list doesn't include it.
$CargoBin = Join-Path $env:CARGO_HOME 'bin'
if (Test-Path $CargoBin) {
    $pathEntries = $env:PATH -split [IO.Path]::PathSeparator
    $already = $pathEntries | Where-Object { $_ -and ([string]::Equals($_, $CargoBin, [StringComparison]::OrdinalIgnoreCase)) }
    if (-not $already) {
        $env:PATH = $CargoBin + [IO.Path]::PathSeparator + $env:PATH
    }
}

# ---------------------------------------------------------------------------
# 2. MSVC x64 developer environment
#
# VsDevCmd.bat sets INCLUDE, LIB, LIBPATH, and prepends the MSVC bin path so
# link.exe and the Windows SDK are reachable. We capture its env block via
# `cmd /c "... && set"` and replay the relevant variables into our session.
#
# This is the same mechanism the official "Developer PowerShell for VS" uses,
# translated to plain PowerShell so it works from Codex's host.
# ---------------------------------------------------------------------------
if (-not $SkipMsvc) {
    $vsDevCmd = 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat'
    if (-not (Test-Path $vsDevCmd)) {
        # Try a few common alternate locations.
        $candidates = @(
            'C:\Program Files\Microsoft Visual Studio\18\BuildTools\Common7\Tools\VsDevCmd.bat',
            'C:\Program Files (x86)\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat',
            'C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat'
        )
        foreach ($c in $candidates) {
            if (Test-Path $c) { $vsDevCmd = $c; break }
        }
    }

    if (Test-Path $vsDevCmd) {
        # Run VsDevCmd.bat in cmd, then dump the resulting env. -arch=x64 is
        # critical — without it link.exe defaults to x86 and Rust's x86_64
        # target fails to link.
        $cmd = "`"$vsDevCmd`" -arch=x64 -host_arch=x64 >nul 2>&1 && set"
        $envBlock = & cmd /c $cmd 2>&1

        # Replay only the MSVC-relevant variables. We deliberately do NOT replay
        # PATH wholesale — VsDevCmd.bat prepends ~30 entries and we'd clobber
        # Codex's preserve_paths. Instead we capture the delta: any MSVC bin
        # path that isn't already on our PATH gets prepended.
        $msvcPathAdditions = @()
        foreach ($line in $envBlock) {
            if ($line -match '^([A-Z][A-Z0-9_]*)=(.*)$') {
                $name = $Matches[1]
                $val  = $Matches[2]
                switch ($name) {
                    'INCLUDE'  { $env:INCLUDE  = $val }
                    'LIB'      { $env:LIB      = $val }
                    'LIBPATH'  { $env:LIBPATH  = $val }
                    'WindowsSdkDir'       { $env:WindowsSdkDir       = $val }
                    'VCINSTALLDIR'        { $env:VCINSTALLDIR        = $val }
                    'VCToolsInstallDir'   { $env:VCToolsInstallDir   = $val }
                    'PATH' {
                        # Capture the full PATH that VsDevCmd produced, then
                        # compute the delta vs. our current PATH below.
                        $script:VsDevPath = $val
                    }
                }
            }
        }

        # Prepend any MSVC bin entries not already on PATH.
        if ($script:VsDevPath) {
            $ours = ($env:PATH -split [IO.Path]::PathSeparator) | ForEach-Object { $_.TrimEnd('\').ToLowerInvariant() }
            $vsEntries = $script:VsDevPath -split [IO.Path]::PathSeparator
            $toPrepend = @()
            foreach ($e in $vsEntries) {
                if (-not $e) { continue }
                $norm = $e.TrimEnd('\').ToLowerInvariant()
                if ($norm -notin $ours -and (Test-Path $e)) {
                    $toPrepend += $e
                }
            }
            if ($toPrepend) {
                # Deduplicate while preserving order.
                $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
                $unique = @()
                foreach ($e in $toPrepend) {
                    if ($seen.Add($e)) { $unique += $e }
                }
                $env:PATH = ($unique -join [IO.Path]::PathSeparator) + [IO.Path]::PathSeparator + $env:PATH
            }
        }

        if (-not $Quiet) {
            Write-Host "MSVC x64 developer environment loaded." -ForegroundColor Green
        }
    } else {
        Write-Warning "VsDevCmd.bat not found at expected path. MSVC linker may be unreachable. Pass -SkipMsvc to silence this."
    }
}

# ---------------------------------------------------------------------------
# 3. Writable TEMP/TMP and CARGO_TARGET_DIR
#
# The default %TEMP% is C:\Users\<user>\AppData\Local\Temp, which is INSIDE
# the broad user profile and may not be in the sandbox's writable root set.
# If Codex's sandbox denies writes there, every cargo build and link.exe
# invocation fails with "access denied". We probe three candidates and use
# the first one that is actually writable:
#
#   a. <workspace>\tmp\codex\  — always sandbox-writable (workspace root)
#   b. <workspace>\target\tmp\ — always sandbox-writable
#   c. $env:TEMP (fallback)    — only if writable
#
# We do NOT redirect if the user passes -NoTargetRedirect.
# ---------------------------------------------------------------------------
function Test-PathWritable {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        try {
            New-Item -ItemType Directory -Path $Path -Force -ErrorAction Stop | Out-Null
        } catch {
            return $false
        }
    }
    $probe = Join-Path $Path ("codex-write-probe-" + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        Set-Content -Path $probe -Value 'probe' -Encoding ASCII -ErrorAction Stop
        Remove-Item $probe -Force -ErrorAction SilentlyContinue
        return $true
    } catch {
        return $false
    }
}

# Candidate temp dirs in preference order. Both are inside the workspace and
# therefore inside Codex's writable root set.
$wsTmp1 = Join-Path $SidebarRoot 'tmp\codex'
$wsTmp2 = Join-Path $SidebarRoot 'target\tmp'

$chosenTmp = $null
foreach ($candidate in @($wsTmp1, $wsTmp2, $env:TEMP)) {
    if (-not $candidate) { continue }
    if (Test-PathWritable -Path $candidate) {
        $chosenTmp = $candidate
        break
    }
}

if ($chosenTmp) {
    $env:TEMP = $chosenTmp
    $env:TMP  = $chosenTmp
    # Cargo reads TMPDIR on some targets; harmless to set on Windows.
    $env:TMPDIR = $chosenTmp
    if (-not $Quiet) {
        Write-Host "TEMP/TMP set to: $chosenTmp" -ForegroundColor Green
    }
} else {
    # This is a hard failure — without writable temp, link.exe and cargo both fail.
    throw "Sandbox denies writes to all candidate temp dirs ($wsTmp1, $wsTmp2, `$env:TEMP). Cannot proceed — Codex needs workspace-write permission on $SidebarRoot."
}

# CARGO_TARGET_DIR: redirect into the workspace so builds are always sandbox-writable.
# The user can opt out with -NoTargetRedirect (e.g. if they want to reuse an existing
# CARGO_HOME\target that is already writable).
if (-not $NoTargetRedirect) {
    $targetDir = Join-Path $SidebarRoot 'target'
    if (Test-PathWritable -Path $targetDir) {
        $env:CARGO_TARGET_DIR = $targetDir
        if (-not $Quiet) {
            Write-Host "CARGO_TARGET_DIR set to: $targetDir" -ForegroundColor Green
        }
    } else {
        Write-Warning "CARGO_TARGET_DIR not set: sandbox denies write to $targetDir. Builds may fail."
    }
}

# ---------------------------------------------------------------------------
# 4. Final sanity probes — fail clearly if the sandbox denies essential writes
#
# We do NOT claim success unless we can actually:
#   - resolve cargo and rustc
#   - write to TEMP
#   - resolve link.exe (if MSVC was loaded)
# ---------------------------------------------------------------------------
if (-not $Quiet) { Write-Host "`nVerifying activation..." -ForegroundColor Cyan }

$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    throw "cargo not found on PATH after activation. CARGO_HOME=$env:CARGO_HOME, PATH starts with: $($env:PATH -split ';' | Select-Object -First 3)"
}

$rustcCmd = Get-Command rustc -ErrorAction SilentlyContinue
if (-not $rustcCmd) {
    throw "rustc not found on PATH after activation. RUSTUP_HOME=$env:RUSTUP_HOME"
}

# Probe link.exe only if we attempted to load MSVC.
if (-not $SkipMsvc) {
    $linkCmd = Get-Command link.exe -ErrorAction SilentlyContinue
    if (-not $linkCmd) {
        # Don't throw — verify-dev-env.ps1 will catch this more precisely.
        # But warn loudly.
        Write-Warning "link.exe not found on PATH after VsDevCmd load. MSVC env may not have applied. Rust x86_64-pc-windows-msvc target will fail to link."
    }
}

if (-not $Quiet) {
    Write-Host "`nCodex dev environment activated." -ForegroundColor Green
    Write-Host "  USERPROFILE     = $env:USERPROFILE"
    Write-Host "  HOME            = $env:HOME"
    Write-Host "  RUSTUP_HOME     = $env:RUSTUP_HOME"
    Write-Host "  CARGO_HOME      = $env:CARGO_HOME"
    Write-Host "  CARGO_TARGET_DIR= $env:CARGO_TARGET_DIR"
    Write-Host "  TEMP / TMP      = $env:TEMP"
    Write-Host "  cargo           = $($cargoCmd.Source)"
    Write-Host "  rustc           = $($rustcCmd.Source)"
    if (-not $SkipMsvc) {
        $linkCmd = Get-Command link.exe -ErrorAction SilentlyContinue
        Write-Host "  link.exe        = $(if ($linkCmd) { $linkCmd.Source } else { '<NOT FOUND>' })"
    }
    Write-Host ""
    Write-Host "  Next: run .\scripts\verify-dev-env.ps1 -NoActivate for full tool audit." -ForegroundColor Cyan
    Write-Host "  Then: cargo fmt --all -- --check  (and the rest of the verification chain)." -ForegroundColor Cyan
}

# ---------------------------------------------------------------------------
# 5. GitHub CLI notes (NOT executed here — see user-facing summary)
#
# This script deliberately does NOT call `gh auth login` or any other credential-
# persisting command. Codex must supply GH_TOKEN through its own env, or the host
# must already be authenticated via the Windows Credential Manager keyring.
#
# To check auth status (read-only, no credential writes):
#     gh auth status
# If that reports not logged in, Codex requires either:
#   (a) GH_TOKEN env var set to a PAT with repo + workflow scopes, OR
#   (b) GH_CONFIG_DIR pointed at a directory containing a valid hosts.yml, OR
#   (c) The host's keyring auth (which works from a real terminal but may not
#       propagate into the sandbox).
# ---------------------------------------------------------------------------
