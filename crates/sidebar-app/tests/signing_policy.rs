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

/// Privacy policy must exist, link to no-telemetry/no-egress statements,
/// and be reachable from README + SECURITY + code-signing-policy. SignPath
/// Foundation OSS approval requires a public privacy-policy page (the
/// submission requirements list at signpath/code-signing-policy.md calls
/// this out explicitly). Cited: docs/privacy-policy.md, guardrails.md G16.
#[test]
fn privacy_policy_doc_exists_and_is_linked_from_repo_surfaces() {
    let root = workspace_root();

    // The policy file itself.
    let policy_path = root.join("docs/privacy-policy.md");
    let policy = fs::read_to_string(&policy_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", policy_path.display()));
    let normalized = policy.replace("\r\n", "\n");
    let lower = normalized.to_lowercase();
    for required in [
        "telemetry",
        "analytics",
        "loopback",
        "127.0.0.1",
        "%appdata%\\sidebar",
        "librehardwaremonitor",
        "signpath",
        "effective date",
    ] {
        assert!(
            lower.contains(required),
            "privacy policy must mention '{required}' (got header + first 200 chars: {:?})",
            &normalized[..normalized.len().min(200)]
        );
    }
    // The policy must explicitly state zero runtime egress (the architectural
    // invariant guardrails.md G16).
    assert!(
        lower.contains("zero runtime network egress"),
        "privacy policy must state 'zero runtime network egress' (G16)"
    );

    // README must link the policy.
    let readme = fs::read_to_string(root.join("README.md")).expect("read README");
    assert!(
        readme.contains("docs/privacy-policy.md"),
        "README.md must link docs/privacy-policy.md"
    );

    // SECURITY.md must link the policy.
    let security = fs::read_to_string(root.join("SECURITY.md")).expect("read SECURITY");
    assert!(
        security.contains("docs/privacy-policy.md"),
        "SECURITY.md must link docs/privacy-policy.md"
    );

    // Code-signing-policy must link the policy AND list the privacy-policy
    // page in its SignPath Foundation submission requirements.
    let signing = fs::read_to_string(root.join("signpath/code-signing-policy.md"))
        .expect("read code-signing-policy");
    assert!(
        signing.contains("docs/privacy-policy.md"),
        "signpath/code-signing-policy.md must link docs/privacy-policy.md"
    );
    assert!(
        signing.to_lowercase().contains("privacy policy"),
        "signpath/code-signing-policy.md must name 'privacy policy' as a SignPath requirement"
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

/// Story 9.2 — release.yml MUST exist with a build/publish flow that fetches
/// the LHM runtime, builds the installer, packages the portable ZIP, and
/// publishes as a draft (HITL review before going public). Cited: Story 9.2
/// DoD, G19.
///
/// v0.1.0 status: UNSIGNED. The SignPath sign: job is stripped until
/// SignPath Foundation approval lands; this test was updated to pin the
/// unsigned pipeline. When signing is wired back in, re-add assertions for
/// the `sign:` job, `release-approver` environment, `SIGNPATH_API_TOKEN`,
/// and `signpath/github-action-submit-signing-request@v2`.
#[test]
fn release_yml_exists_with_build_sign_publish_stages() {
    let path = workspace_root()
        .join(".github")
        .join("workflows")
        .join("release.yml");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let normalized = raw.replace("\r\n", "\n");

    // Trigger — manual release only, no auto-publish on tag (G19).
    assert!(
        normalized.contains("workflow_dispatch"),
        "release.yml must trigger on workflow_dispatch (no auto-publish on tag, G19)"
    );

    // Build + publish in a single job (no separate sign: stage until SignPath
    // is wired up — see the test doc comment above).
    assert!(
        normalized.contains("build-and-publish:"),
        "release.yml must define the build-and-publish: job"
    );
    assert!(
        normalized.contains("run: cargo build --release"),
        "release.yml must build the release binary"
    );
    assert!(
        normalized.contains("run: ./scripts/fetch_ohm.ps1\n"),
        "release builds must fetch the ignored LHM runtime on clean CI checkouts"
    );
    assert!(
        normalized.contains("cp -R resources/. staging/"),
        "release payload must include the complete LHM runtime bundle"
    );

    // ISCC installer build step — produces dist/sidebar-setup.exe.
    assert!(
        normalized.contains("ISCC.exe"),
        "release.yml must invoke ISCC.exe to build the installer"
    );
    assert!(
        normalized.contains("/DAppVersion="),
        "release.yml must pass the version to ISCC via /DAppVersion="
    );

    // Portable ZIP step + naming.
    assert!(
        normalized.contains("sidebar-portable-"),
        "release.yml must produce the sidebar-portable-<version>.zip asset"
    );

    // Release body must include SHA-256 checksums for verification.
    assert!(
        normalized.contains("sha256sum") || normalized.contains("Get-FileHash"),
        "release.yml must compute SHA-256 checksums for the published assets"
    );

    // HITL gate — draft releases only, no auto-publish.
    assert!(
        normalized.contains("draft: true"),
        "release.yml must publish as draft for HITL review"
    );

    // Required for any job that uses actions/checkout or gh-release.
    assert!(
        normalized.contains("actions: read"),
        "release.yml must grant actions: read (used by checkout + release actions)"
    );
}
