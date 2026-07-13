//! Story 9.1 + 9.2 — code-signing policy + release workflow structural tests.
//!
//! Asserts the SignPath policy doc, ohm.sha256 pin, README link, and
//! release.yml workflow all exist and are well-formed. The actual SignPath
//! submission + signing run are HITL-gated per
//! signpath/code-signing-policy.md.

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

/// Story 9.2 — release.yml MUST exist with build/sign/publish stages gated
/// on the `release-approver` environment. Cited: Story 9.2 DoD, G19.
#[test]
fn release_yml_exists_with_build_sign_publish_stages() {
    let path = workspace_root()
        .join(".github")
        .join("workflows")
        .join("release.yml");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let normalized = raw.replace("\r\n", "\n");
    for stage in ["build:", "sign:", "publish:"] {
        assert!(
            normalized.contains(stage),
            "release.yml must define the {stage} stage"
        );
    }
    assert!(
        normalized.contains("release-approver"),
        "release.yml must gate on the release-approver environment (G19 HITL)"
    );
    assert!(
        normalized.contains("workflow_dispatch"),
        "release.yml must trigger on workflow_dispatch (no auto-publish on tag, G19)"
    );
    assert!(
        normalized.contains("SIGNPATH_API_TOKEN"),
        "release.yml must reference the SIGNPATH_API_TOKEN secret"
    );
    assert!(
        normalized.contains("actions: read"),
        "SignPath must be allowed to download the uploaded GitHub artifact"
    );
    assert!(
        normalized.contains("signpath/github-action-submit-signing-request@v2"),
        "release.yml must use the current SignPath GitHub signing-request action"
    );
    for input in [
        "organization-id:",
        "project-slug:",
        "signing-policy-slug:",
        "github-artifact-id:",
        "wait-for-completion: true",
    ] {
        assert!(
            normalized.contains(input),
            "release.yml must provide the SignPath input {input}"
        );
    }
    assert!(
        normalized.contains("draft: true"),
        "release.yml must publish as draft for HITL review"
    );
    assert!(
        normalized.contains("continue-on-error: true"),
        "SignPath failure must reach the explicit unsigned-draft fallback"
    );
    assert!(
        normalized.contains("steps.prepare_payload.outputs.unsigned_release"),
        "the signing job must export unsigned status to the publish job"
    );
    assert!(
        normalized.contains("staging/LibreHardwareMonitor.exe staging/signed/"),
        "both signed and unsigned payloads must include the LHM sidecar"
    );
}
