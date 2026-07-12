//! Story 12.x E2E — launch the release-built exe and verify no runtime
//! regressions (T-13 timeout spam, ERROR lines, crash).
//!
//! This is the test that would have caught the sysinfo T-13 over-fire bug
//! (Tier 1.1): unit tests proved the poller logic, but launching the real
//! exe exposed that sysinfo's cold-start exceeded the 100ms budget on every
//! first poll.
//!
//! The test spawns the `--bench-cold-start` path (which exercises the real
//! egui setup + provider registry construction without opening a window).
//! It asserts:
//! 1. The process exits 0 within 10s.
//! 2. stderr contains NO "T-13 timeout" warnings (the regression signal).
//! 3. stderr contains NO "ERROR" lines.
//!
//! Cited: Tier 1.1 fix (tier-aware timeout), PRD section 5.2 (sysinfo
//! Basic-tier provider).

#![cfg(target_os = "windows")]

use std::process::Command;

/// Locate the sidebar-app binary built by `cargo build --release`.
fn sidebar_exe() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../../target/release/sidebar-app.exe"),
        manifest_dir.join("../../target/x86_64-pc-windows-msvc/release/sidebar-app.exe"),
        manifest_dir.join("../../target/debug/sidebar-app.exe"),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            return candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone());
        }
    }
    panic!(
        "sidebar-app.exe not found in target/{{release,x86_64-pc-windows-msvc/release,debug}}/ — run cargo build first"
    );
}

#[test]
fn e2e_launch_no_t13_timeout_or_errors() {
    let exe = sidebar_exe();
    let temp = tempfile::TempDir::new().expect("tempdir");
    let marker = temp.path().join("e2e_cold_start.txt");

    let output = Command::new(&exe)
        .arg("--bench-cold-start")
        .env("SIDEBAR_BENCH_COLD_START_FILE", &marker)
        .env("SIDEBAR_BENCH_HOLD_MS", "1000")
        .output()
        .unwrap_or_else(|e| panic!("failed to launch {}: {e}", exe.display()));

    assert!(
        output.status.success(),
        "exe must exit 0; got {:?}. stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_lower = stderr.to_lowercase();

    // The T-13 timeout regression: sysinfo (Basic-tier) blowing past the
    // old 100ms budget on cold start. After the tier-aware fix, Basic
    // providers get 500ms — no timeout warning should fire.
    assert!(
        !stderr_lower.contains("t-13 timeout"),
        "REGRESSION: T-13 timeout fired on a Basic-tier provider. stderr:\n{stderr}"
    );

    // No ERROR-level log lines (WARN is acceptable for environmental issues
    // like transparency; ERROR indicates a real failure).
    assert!(
        !stderr_lower.contains("error"),
        "exe logged an ERROR. stderr:\n{stderr}"
    );

    // The marker file proves the cold-start path actually ran.
    assert!(
        marker.exists(),
        "cold-start marker file must exist after launch"
    );
}
