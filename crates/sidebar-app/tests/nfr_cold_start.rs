//! Story 10.1 — executable Basic-mode host-probe timing check (T-7/F9).
//!
//! The `--bench-cold-start` path intentionally measures process-side egui
//! setup only; full production GUI/LHM startup remains a Windows smoke gate.

use sidebar_app::nfr::parse_cold_start_elapsed_ms;

#[cfg(windows)]
#[test]
fn basic_host_probe_stays_under_two_seconds() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let marker = temp.path().join("cold-start.txt");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sidebar-app"))
        .arg("--bench-cold-start")
        .env("SIDEBAR_BENCH_COLD_START_FILE", &marker)
        .output()
        .expect("failed to launch cold-start probe");
    assert!(
        output.status.success(),
        "cold-start probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker_contents = std::fs::read_to_string(&marker).expect("probe marker missing");
    let elapsed_ms = parse_cold_start_elapsed_ms(&marker_contents)
        .expect("probe marker must include elapsed_ms");
    assert!(
        elapsed_ms <= 2_000,
        "T-7 cold start exceeded 2000ms: {elapsed_ms}ms"
    );
}

#[cfg(not(windows))]
#[test]
fn cold_start_probe_is_windows_only() {
    // The actual timing contract is exercised on windows-latest; this keeps
    // the workspace test target portable for domain-only development hosts.
    assert!(!cfg!(windows));
}
