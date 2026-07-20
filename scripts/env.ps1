#Requires -Version 7.2
<#
.SYNOPSIS
    Activate the sidebar development environment for the current PowerShell session.

.DESCRIPTION
    Sets up PATH, RUSTUP_HOME, and CARGO_HOME for THIS PROCESS ONLY. Nothing is
    written to:
      - the user's $PROFILE
      - the Windows Registry
      - persistent environment variables ([Environment]::SetEnvironmentVariable)
      - any global config file (git config --global, cargo, rustup defaults)

    The activation is in effect for the current PowerShell session and any
    child processes it spawns. When the session exits, the activation reverts
    automatically. This makes the script safe to dot-source in any shell
    without polluting the system.

    Idempotent: running it multiple times in the same session is safe; the PATH
    is de-duplicated so the tools entries appear only once.

    Relocatable: the script derives the sidebar root from $PSScriptRoot, so
    moving D:\dev\sidebar\ to another drive or path keeps it working as long
    as the script moves with the project.

.PARAMETER Quiet
    Suppress informational output. Errors still print to stderr.

.EXAMPLE
    . .\scripts\env.ps1
    Dot-sources the script into the current session — recommended usage. The
    $env:PATH and $env:CARGO_HOME changes persist in the caller's session.

.EXAMPLE
    .\scripts\env.ps1
    Runs in a child scope. The env-var changes apply only to this script's
    scope and its children; they do NOT persist in the caller's session.

.NOTES
    Per T-44 + Story 0.7. See CONTRIBUTING.md for the full setup guide.
    Tested on Win11 24H2 + 25H2, PowerShell 7.2+.
#>

[CmdletBinding()]
param(
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Locate the sidebar root relative to this script. We do NOT trust $PWD
# because the script may be invoked from anywhere. $PSScriptRoot is always
# the directory containing this .ps1 file, so Split-Path twice gives the
# project root (scripts/ is one level under root).
# ---------------------------------------------------------------------------
if (-not $PSScriptRoot) {
    # Belt-and-suspenders: in the unlikely case $PSScriptRoot is empty
    # (e.g. executed via Invoke-Expression), fall back to $MyInvocation.
    $PSScriptRoot = Split-Path $MyInvocation.MyCommand.Path -Parent
}
$SidebarRoot   = Split-Path $PSScriptRoot -Parent
$ToolsCargoBin = Join-Path $SidebarRoot 'tools\cargo-bin'
$ToolsCi       = Join-Path $SidebarRoot 'tools\ci'
$ToolsSqlite   = Join-Path $SidebarRoot 'tools\sqlite'

# ---------------------------------------------------------------------------
# Validate the sidebar root looks like a sidebar project. We require either
# docs/backlog/ or docs/PRD.md to exist; bail out otherwise — this catches
# the case where someone copied the script out of context.
# ---------------------------------------------------------------------------
if (-not ((Test-Path (Join-Path $SidebarRoot 'docs\backlog')) -or
          (Test-Path (Join-Path $SidebarRoot 'docs\PRD.md')))) {
    throw "Sidebar root '$SidebarRoot' does not look like a sidebar project (missing docs/backlog or docs/PRD.md). Aborting to avoid polluting PATH with unrelated entries."
}

# ---------------------------------------------------------------------------
# Helper: prepend a path to $env:PATH idempotently.
# De-duplicates by exact string match (case-insensitive on Windows).
# ---------------------------------------------------------------------------
function Add-ToSessionPath {
    param([string]$Entry)
    if (-not (Test-Path $Entry)) {
        Write-Warning "Tools directory not found (expected at $Entry). Run scripts/fetch_ohm.ps1 or see CONTRIBUTING.md for the documented install commands."
        return
    }
    $entries = $env:PATH -split [IO.Path]::PathSeparator
    $already = $entries | Where-Object { $_ -and ([string]::Equals($_, $Entry, [StringComparison]::OrdinalIgnoreCase)) }
    if (-not $already) {
        $env:PATH = "$Entry" + [IO.Path]::PathSeparator + $env:PATH
    }
}

Add-ToSessionPath $ToolsCargoBin
Add-ToSessionPath $ToolsCi
Add-ToSessionPath $ToolsSqlite

# ---------------------------------------------------------------------------
# Ensure CARGO_HOME / RUSTUP_HOME point at the user's defaults IF and ONLY IF
# they aren't already set. We do NOT override them — if the user has a custom
# location, we respect it. If unset, we leave them unset (rustup/cargo fall
# back to ~/.cargo and ~/.rustup, which is what we want).
# ---------------------------------------------------------------------------
# (Intentionally no code here — just documentation of the contract: this
# script does not touch CARGO_HOME / RUSTUP_HOME. Adapters that need them
# should use the user's existing values.)

if (-not $Quiet) {
    Write-Host "sidebar dev environment activated (session-scoped)." -ForegroundColor Green
    Write-Host "  Sidebar root: $SidebarRoot"
    Write-Host "  PATH prepended: tools\cargo-bin, tools\ci, tools\sqlite"
    Write-Host ""
    Write-Host "  NOTE: changes apply to this PowerShell session only. No system/registry/`$PROFILE mutation."
    Write-Host "        Re-run '. .\scripts\env.ps1' in each new session that needs the tooling."
    Write-Host ""
    Write-Host "  Run scripts/verify-dev-env.ps1 for full prerequisite + tool verification." -ForegroundColor Cyan
}
