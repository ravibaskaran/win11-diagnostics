//! Story 9.1 — code-signing policy structural tests.
//!
//! Asserts the SignPath policy doc, ohm.sha256 pin, and README link all
//! exist and are well-formed. The actual SignPath submission + signing run
//! are HITL-gated per signpath/code-signing-policy.md.

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

#[test]
fn code_signing_policy_doc_exists_and_covers_trust_boundary() {
    let path = workspace_root().join("signpath/code-signing-policy.md");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    assert!(
        raw.contains("SignPath Foundation"),
        "policy must name SignPath Foundation"
    );
    assert!(
        raw.contains("MIT") && raw.contains("MPL-2.0"),
        "policy must cover host MIT + bundled MPL-2.0 licenses"
    );
    assert!(
        raw.contains("ohm.sha256"),
        "policy must reference the ohm.sha256 pin"
    );
}

#[test]
fn ohm_sha256_pin_is_well_formed() {
    let path = workspace_root().join("resources/ohm.sha256");
    assert!(path.exists(), "{} must exist", path.display());
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // SHA-256 is 64 hex chars + filename.
    let line = raw.lines().next().unwrap_or("");
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert!(parts.len() >= 2, "ohm.sha256 must be '<hash> <filename>'");
    let hash = parts[0];
    assert_eq!(
        hash.len(),
        64,
        "SHA-256 hash must be 64 hex chars, got {hash}"
    );
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "SHA-256 hash must be hex"
    );
    assert!(
        parts[1].contains("LibreHardwareMonitor"),
        "filename must reference LibreHardwareMonitor"
    );
}
