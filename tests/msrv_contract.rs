//! Story 0.1 — MSRV contract test (RED phase).
//!
//! Verifies the workspace respects MSRV 1.95 (forced by sysinfo 0.39.3 — see
//! architecture.md AD-3, Story 0.1 Technical Context, T-31 adjacent).
//!
//! This is a compile-time contract: if any crate declares `rust-version`
//! above the toolchain, `cargo build` fails with a precise MSRV diagnostic.
//! This test exercises that contract programmatically by inspecting each
//! package's `rust_version` field.
//!
//! Per Story 0.1 TDD contract, Boundary test #1: "MSRV violation: set
//! `rust-version = '99.0.0'` in one crate temporarily; cargo build must
//! error with rustc MSRV diagnostic (not a generic compile error)."
//!
//! Cited thresholds:
//!   - T-44: dev-env requires Rust >= 1.95
//!   - architecture.md AD-3: sysinfo 0.39.3 forces MSRV 1.95

/// The Minimum Supported Rust Version for the workspace.
/// Forced by sysinfo = 0.39.3 per architecture.md AD-3.
pub const WORKSPACE_MSRV: &str = "1.95.0";

#[test]
fn every_crate_declares_rust_version_at_or_below_workspace_msrv() {
    // Per Story 0.1 Boundary test #1: any crate setting rust-version above
    // the workspace MSRV must fail build. This test catches the same error
    // programmatically by inspecting cargo metadata before build.
    //
    // We do NOT require every crate to declare rust-version (only that none
    // exceeds it). The workspace-level rust-toolchain.toml (Story 0.4)
    // pins the active toolchain to 1.95.0.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let msrv = semver::Version::parse(WORKSPACE_MSRV).expect("WORKSPACE_MSRV must parse");

    for pkg in &metadata.packages {
        if let Some(rv) = &pkg.rust_version {
            let crate_rv =
                semver::VersionReq::parse(rv).unwrap_or_else(|_| {
                    panic!("crate {} has unparseable rust-version: {}", pkg.name, rv)
                });
            // rust-version is a VersionReq (e.g. "1.95.0"). We require it to
            // be satisfiable by WORKSPACE_MSRV. If a crate requires > 1.95,
            // this assertion fails.
            assert!(
                crate_rv.matches(&msrv),
                "Story 0.1 MSRV violation: crate '{}' declares rust-version='{}' which is NOT \
                 satisfied by workspace MSRV {}. Either lower the crate's rust-version or bump \
                 the workspace toolchain (Story 0.4).",
                pkg.name,
                rv,
                WORKSPACE_MSRV
            );
        }
    }
}
