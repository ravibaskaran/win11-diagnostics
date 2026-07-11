//! Story 10.1 — host-probe network-egress assertion (G16).
//!
//! The executable probe bypasses production GUI/LHM composition. This smoke
//! checks the host probe's socket behavior; full production egress evidence
//! remains a Windows smoke gate.

use sidebar_app::nfr::remote_endpoints_for_pid;

#[test]
fn netstat_parser_rejects_non_loopback_fixture() {
    let output = "  TCP    127.0.0.1:5000    198.51.100.10:443 ESTABLISHED    42\n";
    assert_eq!(
        remote_endpoints_for_pid(output, 42),
        vec!["198.51.100.10:443".to_string()]
    );
}

#[cfg(windows)]
#[test]
#[ignore = "60-second Windows egress smoke; run with cargo test --ignored"]
fn sidebar_process_opens_no_outbound_sockets() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let marker = temp.path().join("egress-start.txt");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_sidebar-app"))
        .arg("--bench-cold-start")
        .env("SIDEBAR_BENCH_COLD_START_FILE", &marker)
        .env("SIDEBAR_BENCH_HOLD_MS", "65000")
        .spawn()
        .expect("failed to launch egress probe");
    let pid = child.id();
    let before = netstat_snapshot();
    std::thread::sleep(std::time::Duration::from_mins(1));
    let after = netstat_snapshot();
    let before_endpoints = remote_endpoints_for_pid(&before, pid);
    let after_endpoints = remote_endpoints_for_pid(&after, pid);
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        !before.is_empty(),
        "netstat must produce a baseline snapshot"
    );
    assert!(
        before_endpoints.is_empty(),
        "host probe opened outbound sockets at startup: {before_endpoints:?}"
    );
    assert!(
        after_endpoints.is_empty(),
        "G16 egress regression: host probe opened {after_endpoints:?}"
    );
}

#[cfg(windows)]
fn netstat_snapshot() -> String {
    let output = std::process::Command::new("netstat")
        .args(["-ano"])
        .output()
        .expect("netstat must be available on Windows CI");
    assert!(output.status.success(), "netstat failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(not(windows))]
#[test]
fn runtime_egress_probe_is_windows_only() {
    assert!(!cfg!(windows));
}
