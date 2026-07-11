//! Story 10.1 — executable host-probe RSS check (T-4/T-5/T-6).
//!
//! The probe bypasses production GUI/LHM composition; full production RSS
//! evidence remains a Windows smoke gate.

use sidebar_app::nfr::percentile;

#[cfg(windows)]
#[test]
#[ignore = "30-second Windows process smoke; run with cargo test --ignored"]
fn basic_host_probe_rss_p95_stays_under_80_mib() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let marker = temp.path().join("rss-start.txt");
    let hold_ms = 30_000_u64;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_sidebar-app"))
        .arg("--bench-cold-start")
        .env("SIDEBAR_BENCH_COLD_START_FILE", &marker)
        .env("SIDEBAR_BENCH_HOLD_MS", hold_ms.to_string())
        .spawn()
        .expect("failed to launch RSS probe");

    let mut samples = Vec::with_capacity(60);
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(bytes) = working_set_bytes(child.id()) {
            samples.push(bytes);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(samples.len(), 60, "RSS probe exited before all samples");
    let p95 = percentile(&samples, 95).expect("RSS samples are non-empty");
    let limit = 80 * 1024 * 1024;
    assert!(
        p95 <= limit,
        "T-4 Basic RSS p95 {p95} exceeds {limit} bytes"
    );
}

#[cfg(windows)]
fn working_set_bytes(pid: u32) -> Option<u64> {
    use std::mem::size_of;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: `pid` comes from a live child process we spawned; the requested
    // access only reads process memory counters and the handle is closed below.
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }.ok()?;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>())
        .expect("PROCESS_MEMORY_COUNTERS size fits DWORD");
    // SAFETY: `counters` is a valid writable buffer with the documented size.
    let ok = unsafe { GetProcessMemoryInfo(handle, &raw mut counters, size).is_ok() };
    // SAFETY: `handle` was returned by OpenProcess and is owned by this scope.
    let _ = unsafe { CloseHandle(handle) };
    ok.then_some(counters.WorkingSetSize as u64)
}

#[cfg(not(windows))]
#[test]
fn rss_probe_is_windows_only() {
    assert!(!cfg!(windows));
}
