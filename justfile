# justfile — cross-platform command runner for sidebar.
# Docs: https://github.com/casey/just
#
# Install just:
#   cargo install just
#   winget install Casey.Just
#   scoop install just
#
# The canonical implementation lives in scripts/*.ps1 (Windows). This justfile
# is pure sugar: it wraps the common workflows so contributors don't have to
# memorize PowerShell paths. On non-Windows platforms (no PowerShell), the
# build/test/fmt/clippy recipes still work — only fetch-ohm + installer require
# Windows.
#
# Run `just` (no args) to list all recipes.

# Default: list available recipes.
default:
    @just --list

# --- Verification chain (mirrors .github/workflows/ci.yml) ---

# Full local verification gate — same chain CI runs on every PR.
verify: fmt clippy test deny
    @echo "All gates green."

# cargo fmt --check (no fix). Fails on any unformatted file.
fmt:
    cargo fmt --all -- --check

# Apply rustfmt to all crates.
fmt-fix:
    cargo fmt --all

# cargo clippy with the workspace-wide lint policy.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Workspace test suite (lib + integration).
test:
    cargo test --workspace --all-targets

# cargo-deny: license + advisory + ban + source checks.
deny:
    cargo deny check

# --- Build ---

# Debug build.
build:
    cargo build --workspace

# Release build for x86_64 Windows (matches the release pipeline target).
build-release:
    cargo build --release --target x86_64-pc-windows-msvc

# --- LHM runtime (Windows-only) ---

# Download + hash-verify the pinned LibreHardwareMonitor binary.
fetch-ohm:
    pwsh -NoProfile -File scripts/fetch_ohm.ps1

# Verify only (no download). Fails on hash mismatch.
fetch-ohm-check:
    pwsh -NoProfile -File scripts/fetch_ohm.ps1 -CheckOnly

# --- Installer (Windows + Inno Setup 6 required) ---

# Build the Inno Setup installer -> dist/sidebar-setup.exe.
# Version override: just installer 0.2.0
installer version='0.1.0':
    ISCC.exe /DAppVersion={{version}} installer\sidebar.iss

# --- Dev-env activation (Windows-only) ---

# Activate the dev env in your current PowerShell session (PATH only, no system mutation).
# Usage from pwsh: . .\scripts\env.ps1  — just can't dot-source for you, so this
# just prints the command to run.
env:
    @echo "Run this in your shell:  . .\scripts\env.ps1"

# --- Cleanup ---

# Wipe build artifacts + the dist folder (keeps dist/.gitkeep tracked).
clean:
    cargo clean
    -rm -rf dist/sidebar-setup.exe
    @echo "Cleaned target/ + dist/."
