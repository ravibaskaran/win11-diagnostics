//! Story 0.1 — MSRV contract integration test.
//!
//! Per Story 0.1 TDD contract, Boundary test #1: "MSRV violation: set
//! `rust-version = '99.0.0'` in one crate temporarily; cargo build must
//! error with rustc MSRV diagnostic."
//!
//! This is the programmatic equivalent — inspects each package's
//! `rust_version` field via cargo metadata and asserts none exceeds the
//! workspace MSRV. Catches the same class of error before build.
//!
//! Cited:
//!   - Story 0.1 Boundary test #1
//!   - T-44 (dev-env requires Rust >= 1.95)
//!   - architecture.md AD-3 (sysinfo 0.39.3 forces MSRV 1.95)

/// The Minimum Supported Rust Version for the workspace.
/// Forced by sysinfo = 0.39.3 per architecture.md AD-3.
pub const WORKSPACE_MSRV: &str = "1.95.0";

#[test]
fn every_crate_declares_rust_version_at_or_below_workspace_msrv() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let msrv = semver::Version::parse(WORKSPACE_MSRV).expect("WORKSPACE_MSRV must parse as semver");

    for pkg in &metadata.packages {
        if let Some(crate_rv) = &pkg.rust_version {
            // cargo_metadata 0.19 exposes rust_version as a semver::Version (the
            // minimum required version). We require it to be <= workspace MSRV.
            assert!(
                crate_rv <= &msrv,
                "Story 0.1 MSRV violation: crate '{}' declares rust-version='{}' which exceeds \
                 workspace MSRV {}. Either lower the crate's rust-version or bump the workspace \
                 toolchain (Story 0.4).",
                pkg.name,
                crate_rv,
                WORKSPACE_MSRV
            );
        }
    }
}
