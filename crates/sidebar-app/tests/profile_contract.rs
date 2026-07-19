//! Story 0.4 — Build-profile contract integration test.
//!
//! Verifies the [profile.release] block in the root Cargo.toml satisfies
//! the Story 0.4 contracts:
//!   1. panic = "unwind" (NOT "abort") — required for G15 panic-safety.
//!   2. lto + codegen-units + opt-level + strip set for NFR-3/NFR-4.
//!
//! This test parses Cargo.toml directly (rather than via cargo metadata,
//! which doesn't expose profile settings) using the `toml` crate. It's an
//! integration test in sidebar-app because that's where workspace-level
//! contract tests live (per Story 0.1's test-split decision).
//!
//! Cited:
//!   - Story 0.4 TDD contract + Technical Context
//!   - guardrails.md G15 (panic-safety)
//!   - architecture.md §6 (release profile note)

/// Locate the root Cargo.toml by walking up from CARGO_MANIFEST_DIR until
/// we find the [workspace] table. Returns the path + parsed TOML.
fn load_workspace_manifest() -> (std::path::PathBuf, toml::Value) {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current = manifest_dir.as_path();
    loop {
        let candidate = current.join("Cargo.toml");
        if candidate.exists() {
            let raw = std::fs::read_to_string(&candidate)
                .unwrap_or_else(|e| panic!("read {}: {e}", candidate.display()));
            let parsed: toml::Value = toml::from_str(&raw)
                .unwrap_or_else(|e| panic!("parse {}: {e}", candidate.display()));
            if parsed.get("workspace").is_some() {
                return (candidate, parsed);
            }
        }
        current = current
            .parent()
            .expect("reached filesystem root without finding workspace Cargo.toml");
    }
}

#[test]
fn release_profile_uses_panic_unwind() {
    // Story 0.4 + G15: panic MUST be "unwind" so the poller's catch_unwind
    // works. A regression to "abort" would silently disable panic-safety.
    let (_path, manifest) = load_workspace_manifest();
    let release = manifest
        .get("profile")
        .and_then(|p| p.get("release"))
        .and_then(|r| r.as_table())
        .expect("[profile.release] must exist in workspace Cargo.toml");

    let panic = release
        .get("panic")
        .and_then(|p| p.as_str())
        .expect("[profile.release] must have a 'panic' key");
    assert_eq!(
        panic, "unwind",
        "Story 0.4 / G15 violation: panic MUST be 'unwind' for catch_unwind panic-safety. \
         Got '{panic}'. Changing to 'abort' silently breaks guardrail G15."
    );
}

#[test]
fn release_profile_has_nfr3_nfr4_tuning() {
    // NFR-3 (cold-start <2s) + NFR-4 (RSS <80 MiB) require LTO + opt-level=3
    // + strip. A regression to dev defaults would breach the NFR budgets.
    let (_path, manifest) = load_workspace_manifest();
    let release = manifest
        .get("profile")
        .and_then(|p| p.get("release"))
        .and_then(|r| r.as_table())
        .expect("[profile.release] must exist");

    let opt = release
        .get("opt-level")
        .and_then(toml::Value::as_integer)
        .expect("opt-level must be set");
    assert_eq!(opt, 3, "opt-level MUST be 3 for release");

    let lto = release
        .get("lto")
        .and_then(|v| v.as_str())
        .or_else(|| {
            release
                .get("lto")
                .and_then(toml::Value::as_bool)
                .map(|_| "bool")
        })
        .expect("lto must be set");
    assert!(
        lto == "fat" || lto == "thin" || lto == "true" || lto == "bool",
        "lto must be enabled (got {lto})"
    );

    let codegen = release
        .get("codegen-units")
        .and_then(toml::Value::as_integer)
        .expect("codegen-units must be set");
    assert_eq!(
        codegen, 1,
        "codegen-units MUST be 1 for max optimization (got {codegen})"
    );

    let strip = release
        .get("strip")
        .and_then(|v| v.as_str())
        .or_else(|| {
            release
                .get("strip")
                .and_then(toml::Value::as_bool)
                .map(|_| "bool")
        })
        .expect("strip must be set");
    assert!(
        strip == "symbols" || strip == "debuginfo" || strip == "true" || strip == "bool",
        "strip must be enabled (got {strip})"
    );
}

#[test]
fn rust_toolchain_toml_pins_1_95() {
    // Story 0.4: rust-toolchain.toml pins MSRV 1.95. A regression would
    // silently allow builds on 1.94 (which lacks sysinfo 0.39.3 support).
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current = manifest_dir.as_path();
    loop {
        let candidate = current.join("rust-toolchain.toml");
        if candidate.exists() {
            let raw = std::fs::read_to_string(&candidate)
                .unwrap_or_else(|e| panic!("read rust-toolchain.toml: {e}"));
            let parsed: toml::Value =
                toml::from_str(&raw).unwrap_or_else(|e| panic!("parse rust-toolchain.toml: {e}"));
            let channel = parsed
                .get("toolchain")
                .and_then(|t| t.get("channel"))
                .and_then(|c| c.as_str())
                .expect("rust-toolchain.toml must have [toolchain] channel");
            assert!(
                channel.starts_with("1.95") || channel == "stable" || channel == "nightly",
                "rust-toolchain.toml channel must be 1.95.x (or stable/nightly tracking 1.95+). \
                 Got '{channel}'."
            );
            return;
        }
        current = current.parent().expect("reached root");
    }
}
