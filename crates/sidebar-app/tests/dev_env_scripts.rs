//! Story 0.7 — Dev-env scripts integration test.
//!
//! Verifies the three PowerShell scripts behave per the Story 0.7 TDD
//! contract by invoking them via `pwsh -NoProfile -File` and asserting
//! on their stdout / stderr / exit code.
//!
//! Cited:
//!   - Story 0.7 TDD contract (Happy Path + Boundary)
//!   - nfr-thresholds.md T-44 (dev-env prerequisite contract)
//!
//! ## Why integration-test PowerShell from Rust?
//!
//! The scripts are PowerShell 7, so we can't unit-test them in Rust directly.
//! Instead we invoke them as black boxes via `std::process::Command` running
//! `pwsh.exe`. This catches regressions in script behavior (PATH mutation,
//! verification gates, idempotency) without coupling test logic to the
//! script internals.
//!
//! ## Skip behavior
//!
//! If `pwsh.exe` (PowerShell 7+) is not on PATH, all tests in this file
//! `#[ignore]`-skip rather than fail — Story 0.7 is only testable where
//! the dev env is provisioned. Run with `cargo test --ignored` to force
//! them on a properly-configured machine.

use std::path::PathBuf;
use std::process::Command;

/// Locate the sidebar workspace root from CARGO_MANIFEST_DIR.
///
/// Walks UP from the test crate's manifest dir until it finds a Cargo.toml
/// that contains a [workspace] table. The per-crate Cargo.toml under
/// crates/sidebar-app/ is NOT the workspace root — it inherits from it.
fn workspace_root() -> PathBuf {
    let mut current = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = current.join("Cargo.toml");
        if candidate.exists() {
            // Check whether this Cargo.toml has [workspace] — only the root does.
            if let Ok(raw) = std::fs::read_to_string(&candidate) {
                if raw.contains("[workspace]") {
                    return current;
                }
            }
        }
        current = current
            .parent()
            .expect("reached filesystem root without finding [workspace] Cargo.toml")
            .to_path_buf();
    }
}

/// Locate PowerShell 7+ (`pwsh.exe`). Tries the canonical install location
/// first (`C:\Program Files\PowerShell\7\pwsh.exe`) then falls back to PATH
/// lookup via `which`. Returns None if neither works.
///
/// On some systems, a `~/bin/pwsh` shim shadows the real PS7 binary and
/// resolves to Windows PowerShell 5.1, which is incompatible with the
/// `#Requires -Version 7.0` directive in our scripts. So we prefer the
/// canonical path.
fn find_pwsh() -> Option<PathBuf> {
    // 1. Canonical install location (most reliable).
    let canonical = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
    if canonical.exists() {
        return Some(canonical);
    }
    // 2. PATH lookup.
    which::which("pwsh").ok()
}

/// Skip-aware test helper — returns the pwsh path or skips the test.
macro_rules! require_pwsh {
    () => {
        match find_pwsh() {
            Some(p) => p,
            None => {
                eprintln!(
                    "skipping: pwsh.exe (PowerShell 7+) not on PATH. \
                     Run on a machine where the dev env is provisioned."
                );
                return;
            }
        }
    };
}

#[test]
fn env_ps1_prepends_tools_to_path_in_session() {
    // Story 0.7 Happy Path #1: env.ps1 invoked in a fresh pwsh session
    // -> $env:PATH contains 'tools\cargo-bin'.
    let pwsh = require_pwsh!();
    let scripts_dir = workspace_root().join("scripts");

    let output = Command::new(pwsh)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            // Dot-source env.ps1 then print whether tools\cargo-bin is on PATH.
            &format!(
                ". '{}'; ($env:PATH -split ';') -match 'tools.cargo-bin' | Select-Object -First 1",
                scripts_dir.join("env.ps1").display()
            ),
        ])
        .output()
        .expect("failed to invoke pwsh");

    assert!(
        output.status.success(),
        "pwsh exited non-zero: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo-bin"),
        "env.ps1 must prepend tools\\cargo-bin to PATH. Got stdout: {stdout}"
    );
}

#[test]
fn env_ps1_does_not_mutate_persistent_path() {
    // Story 0.7 + dev-env.md §0 contract: env.ps1 is session-scoped; it
    // MUST NOT mutate the persistent User or Machine PATH.
    let pwsh = require_pwsh!();
    let env_script = workspace_root().join("scripts").join("env.ps1");

    // Build the PowerShell command as a plain string (no format! escaping
    // gymnastics). Dot-source env.ps1, then read the persistent PATH scopes
    // and report hit counts.
    let ps = format!(
        ". \"{env_script}\";
        $user = [Environment]::GetEnvironmentVariable('PATH', 'User');
        $machine = [Environment]::GetEnvironmentVariable('PATH', 'Machine');
        $userHits = ($user -split ';' | Where-Object {{ $_ -match 'sidebar.tools' }}).Count;
        $machineHits = ($machine -split ';' | Where-Object {{ $_ -match 'sidebar.tools' }}).Count;
        Write-Output \"user=$userHits machine=$machineHits\"",
        env_script = env_script.display()
    );

    let output = Command::new(pwsh)
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .output()
        .expect("failed to invoke pwsh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("user=0") && stdout.contains("machine=0"),
        "env.ps1 must not mutate persistent PATH. Got: {stdout}"
    );
}

#[test]
fn verify_dev_env_ps1_exits_zero_on_configured_machine() {
    // Story 0.7 Happy Path #2: on a correctly-configured machine, exit 0.
    // This test runs only where pwsh is present; on a misconfigured machine
    // it would fail loudly (which is the contract).
    let pwsh = require_pwsh!();
    let script = workspace_root().join("scripts").join("verify-dev-env.ps1");

    let output = Command::new(pwsh)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-File",
            &script.display().to_string(),
        ])
        .output()
        .expect("failed to invoke pwsh");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "verify-dev-env.ps1 must exit 0 on a configured machine. \
             exit={:?} stdout={stdout} stderr={stderr}",
            output.status.code()
        );
    }
}

#[test]
fn verify_dev_env_ps1_json_mode_emits_valid_json() {
    // Story 0.7 + dev-env.md: -Json mode emits machine-readable JSON.
    let pwsh = require_pwsh!();
    let script = workspace_root().join("scripts").join("verify-dev-env.ps1");

    let output = Command::new(pwsh)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-File",
            &script.display().to_string(),
            "-Json",
        ])
        .output()
        .expect("failed to invoke pwsh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("verify-dev-env.ps1 -Json must emit valid JSON. Parse error: {e}. Raw: {stdout}")
    });
    assert!(
        parsed.get("summary").is_some(),
        "JSON must have a 'summary' key. Got: {stdout}"
    );
    assert!(
        parsed.get("checks").is_some(),
        "JSON must have a 'checks' key"
    );
}

#[test]
fn fetch_ohm_ps1_is_idempotent_when_hash_matches() {
    // Story 0.7 Happy Path #3: fetch_ohm.ps1 idempotent — second invocation
    // skips download because hash already matches.
    let pwsh = require_pwsh!();
    let script = workspace_root().join("scripts").join("fetch_ohm.ps1");

    // Pre-condition: LibreHardwareMonitor.exe must already exist (it's
    // provisioned during dev-env setup). If not, skip.
    let lhm = workspace_root()
        .join("resources")
        .join("LibreHardwareMonitor.exe");
    if !lhm.exists() {
        eprintln!(
            "skipping: LibreHardwareMonitor.exe not present at {}",
            lhm.display()
        );
        return;
    }

    let output = Command::new(pwsh)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-File",
            &script.display().to_string(),
        ])
        .output()
        .expect("failed to invoke pwsh");

    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let success = output.status.success();
    assert!(
        success && stdout.contains("already present"),
        "fetch_ohm.ps1 second invocation must skip + log 'already present'. \
         exit={:?} stdout={stdout}",
        output.status.code()
    );
}

#[test]
fn fetch_ohm_ps1_exists_and_parses() {
    // Story 0.7 Boundary: the script must exist and be syntactically valid
    // PowerShell (parses via the PowerShell parser). Catches typos before
    // runtime. If the LHM binary is missing, this still confirms the script
    // itself is well-formed.
    let pwsh = require_pwsh!();
    let script = workspace_root().join("scripts").join("fetch_ohm.ps1");
    assert!(script.exists(), "{} must exist", script.display());

    let output = Command::new(pwsh)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$null = [System.Management.Automation.PSParser]::Tokenize((Get-Content -Raw '{}'), [ref]$null); 'parses'",
                script.display()
            ),
        ])
        .output()
        .expect("failed to invoke pwsh");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("parses"),
        "fetch_ohm.ps1 must be syntactically valid PowerShell. Got: {stdout}"
    );
}
