//! Story 13.5 — structural test for verify/reference-machine.ps1.
//!
//! Asserts the reference-machine runner script exists, is well-formed, and
//! contains the required stages per T-46. The script itself is a PowerShell
//! file (not Rust), so this test is a structural assertion on its content
//! rather than an execution. The actual run is performed by the maintainer
//! on the designated T-31 reference machine.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("sidebar-app must live under crates/")
        .to_path_buf()
}

/// Cited: Story 13.5, T-46, F14. The reference-machine runner script MUST
/// exist at verify/reference-machine.ps1 and contain the required stages
/// (pre-flight, build, workspace tests, ignored suite, bench, scriptable
/// smoke, SHA-256, manual items, verdict).
#[test]
fn reference_machine_script_exists_and_is_well_formed() {
    let path = workspace_root().join("verify/reference-machine.ps1");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let normalized = raw.replace("\r\n", "\n");

    // Must require PowerShell 7.
    assert!(
        normalized.contains("#Requires -Version 7.0"),
        "script MUST require PowerShell 7.0"
    );

    // Must contain the 9 stage markers.
    for stage in [
        "Pre-flight",
        "Release build",
        "Workspace tests",
        "Ignored suite",
        "NFR-1",
        "Scriptable smoke",
        "Release exe SHA-256",
        "Manual smoke items",
        "Verdict",
    ] {
        assert!(
            normalized.contains(stage),
            "script MUST contain stage '{stage}'"
        );
    }

    // Must use the 0/1 exit convention per T-46.
    assert!(
        normalized.contains("exit 0"),
        "script MUST exit 0 on success (T-46)"
    );
    assert!(
        normalized.contains("exit 1"),
        "script MUST exit 1 on failure (T-46)"
    );

    // Must write to verify/evidence/<date>/ per T-46.
    assert!(
        normalized.contains("verify\\evidence"),
        "script MUST write to verify/evidence/ (T-46)"
    );
}

/// Cited: Story 13.5, T-46. The verify/evidence/ directory MUST exist and
/// be tracked by git (so the script can write into it on the reference
/// machine without needing a mkdir).
#[test]
fn evidence_directory_exists_with_gitkeep() {
    let root = workspace_root();
    let evidence_dir = root.join("verify/evidence");
    assert!(evidence_dir.exists(), "verify/evidence/ MUST exist");
    assert!(
        evidence_dir.join(".gitkeep").exists(),
        "verify/evidence/.gitkeep MUST exist so the directory is git-tracked"
    );
}
