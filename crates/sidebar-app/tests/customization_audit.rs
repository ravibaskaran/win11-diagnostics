//! Story 12.4 — customization parity audit structural test.

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
fn customization_audit_covers_in_deferred_out() {
    let path = workspace_root().join("docs/customization-parity.md");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // The audit must classify every option as IN, DEFERRED, or OUT.
    assert!(raw.contains("**IN**"), "audit must define the IN legend");
    assert!(
        raw.contains("**DEFERRED**"),
        "audit must define the DEFERRED legend"
    );
    assert!(raw.contains("**OUT**"), "audit must define the OUT legend");
    // Plugin/scripting + cloud sync must be OUT (PRD §4).
    assert!(raw.contains("Plugin/scripting"));
    assert!(raw.contains("Cloud sync"));
    // The NFR guardrail section must exist.
    assert!(raw.contains("NFR-1/NFR-4 guardrail"));
}
