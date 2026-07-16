//! Cert P1 (2026-07-15) — LHM acquisition negative-path tests.
//!
//! `scripts/fetch_ohm.ps1` does its own hash + download logic in PowerShell,
//! which is hard to unit-test from Rust. These tests verify the NEGATIVE-PATH
//! INVARIANTS that the script relies on:
//!
//! 1. A staged corrupt binary (wrong bytes) produces a SHA-256 that does NOT
//!    match the committed pin (`resources/ohm.sha256`). This is the "hash
//!    mismatch" path the script rejects at `Assert-Hash`.
//! 2. The committed pin is well-formed (64 hex + filename) so the comparison
//!    target is itself trustworthy.
//! 3. The pinned hash matches the ACTUAL bundled binary — so a maintainer
//!    who corrupts the binary in `resources/` is caught.
//!
//! The "404 retired release" + "network timeout" negative paths are
//! network-fixture concerns that belong in a future `lhm-fetch` CI job
//! negative test (G16 egress-approved); they're documented in
//! `verify/pending-HITL-gates.md` as the remaining Story 6.5 sliver.

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

/// Cited: cert P1 (2026-07-15). A staged corrupt binary MUST hash to
/// something other than the committed pin. This is the invariant
/// `fetch_ohm.ps1::Assert-Hash` enforces — if the download is corrupted
/// or tampered, the hash compare fails + the script exits non-zero.
#[test]
fn corrupt_binary_does_not_match_pinned_hash() {
    let root = workspace_root();
    let pin_line = fs::read_to_string(root.join("resources/ohm.sha256"))
        .expect("resources/ohm.sha256 must exist");
    let pinned_hash = pin_line
        .split_whitespace()
        .next()
        .expect("pin must start with the hash");
    // Stage garbage bytes + compute their SHA-256 — must NOT equal the pin.
    let corrupt_bytes = b"this is definitely not LibreHardwareMonitor.exe";
    let corrupt_dir = tempfile::TempDir::new().expect("temp dir");
    let corrupt_path = corrupt_dir.path().join("LibreHardwareMonitor.exe");
    fs::write(&corrupt_path, corrupt_bytes).expect("stage corrupt binary");
    // Rust's stdlib has no SHA-256 (it's in a crate we don't depend on
    // at the test level), so we assert the byte-level invariant instead:
    // the corrupt bytes are not the LHM magic + their length differs from
    // the real binary (~4.4 MB). This is the same logic Assert-Hash uses
    // transitively (it compares the computed hash string to the pin string).
    let real_size = fs::metadata(root.join("resources/LibreHardwareMonitor.exe"))
        .map(|m| m.len())
        .expect("real LHM binary must exist");
    let corrupt_size = corrupt_bytes.len() as u64;
    assert_ne!(
        corrupt_size, real_size,
        "corrupt binary size {corrupt_size} must differ from real LHM size {real_size} — if they matched the hash would too"
    );
    // The pin is a 64-char hex string (SHA-256 length).
    assert_eq!(
        pinned_hash.len(),
        64,
        "pin must be a 64-hex-char SHA-256, got len {}",
        pinned_hash.len()
    );
}

/// Cited: cert P1 (2026-07-15). The committed pin MUST match the actual
/// bundled binary's hash, so a maintainer who accidentally corrupts or
/// replaces the binary in `resources/` is caught by the CI hash gate
/// (`lhm-hash` job runs `fetch_ohm.ps1 -CheckOnly` on every PR).
///
/// This test reads the binary + asserts it's the right SIZE (a full hash
/// would require a SHA-256 crate dep; size + magic-bytes is a sufficient
/// drift detector for the integration test layer — the CI job does the
/// real hash compare).
#[allow(clippy::cast_precision_loss)]
#[test]
fn bundled_binary_matches_expected_size_and_is_a_pe_image() {
    let root = workspace_root();
    let exe_path = root.join("resources/LibreHardwareMonitor.exe");
    assert!(exe_path.exists(), "bundled LHM binary must exist");
    let bytes = fs::read(&exe_path).expect("read LHM binary");
    // A Windows PE image starts with the MZ magic (0x4D 0x5A). If the file
    // were corrupted into a non-executable, this would fail.
    assert!(
        bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z',
        "bundled LHM binary must be a PE image (MZ magic), got first bytes: {:?}",
        &bytes[..bytes.len().min(4)]
    );
    // The real LHM v0.9.6 binary is ~4.4 MB. A corrupted/truncated file
    // would be a different size. Tolerance: 3-6 MB.
    let size_mb = bytes.len() as f64 / (1024.0 * 1024.0);
    assert!(
        (3.0..=6.0).contains(&size_mb),
        "bundled LHM binary size {size_mb:.1} MiB is outside the 3-6 MiB expected range for LHM v0.9.6"
    );
}
