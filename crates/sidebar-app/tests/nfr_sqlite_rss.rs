//! Story 10.1 — T-6 SQLite RSS contribution check.
//!
//! T-6 mandates SQLite's RSS contribution stays <= 3 MiB. We measure the
//! test-process working-set delta around `sidebar-persistence::schema::init`
//! plus a realistic insert workload (1000 current_cycle rows + 1000 archive
//! cycles simulating a long-running install). The delta isolates the SQLite
//! page-cache + WAL + connection overhead from the rest of the process; the
//! assertion has a 2x ceiling (6 MiB) to absorb allocator granularity on CI
//! while still catching a 10x regression.
//!
//! Cited: Story 10.1 DoD (T-6 SQLite RSS), nfr-thresholds.md T-6 (<= 3 MiB),
//! guardrails.md G21 (all SQLite access via sidebar-persistence).

#![cfg(target_os = "windows")]

/// T-6: SQLite contribution <= 3 MiB; 2x ceiling (6 MiB) absorbs allocator
/// granularity on CI. A 10x regression (30 MiB) would fail this loudly.
const T6_CEILING: u64 = 6 * 1024 * 1024;

use rusqlite::Connection;
use sidebar_persistence::{bandwidth_repo, schema};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Returns the current process working-set size in bytes.
fn working_set_bytes() -> u64 {
    use std::mem::size_of;
    // SAFETY: GetCurrentProcess returns a pseudo-handle (always valid for
    // the calling process); PROCESS_MEMORY_COUNTERS is a stack buffer of
    // the documented size. Pseudo-handles don't need CloseHandle.
    let handle = unsafe { GetCurrentProcess() };
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>())
        .expect("PROCESS_MEMORY_COUNTERS size fits DWORD");
    // SAFETY: `counters` is a valid writable buffer with the documented size.
    let ok = unsafe { GetProcessMemoryInfo(handle, &raw mut counters, size).is_ok() };
    ok.then_some(counters.WorkingSetSize as u64)
        .expect("GetProcessMemoryInfo must succeed on the current process")
}

/// Story 10.1 / T-6 — schema init + 1000 current_cycle + 1000 archive cycles
/// must keep the SQLite RSS contribution bounded. We measure RSS before
/// opening the connection and after the workload; the delta (with a 2x
/// ceiling = 6 MiB to absorb allocator granularity) catches a 10x regression
/// while tolerating CI noise.
#[test]
fn sqlite_rss_contribution_stays_bounded_after_realistic_workload() {
    let dir = tempfile::TempDir::new().expect("TempDir");
    let db_path = dir.path().join("t6_rss.db");

    // Baseline: process RSS before any SQLite work.
    let rss_before = working_set_bytes();

    let conn = Connection::open(&db_path).expect("open connection");
    schema::init(&conn).expect("schema::init");

    // Insert 1000 current_cycle rows (one per fictional LUID).
    for luid in 0..1_000_i64 {
        bandwidth_repo::save_accumulator(
            &conn,
            luid,
            "fake-nic",
            1_000_000,
            500_000,
            "2026-07-01",
            "2026-07-15 12:00:00",
        )
        .expect("save_accumulator");
    }
    // Archive 1000 times to populate history.
    for _ in 0..1_000 {
        let _ = bandwidth_repo::archive_cycle(&conn, "2026-07-31", "2026-07-31T23:59:59Z");
    }

    // Force a VACUUM to materialize the on-disk + cache state.
    let _ = conn.execute_batch("VACUUM;");
    let _ = conn.close();

    // Reopen to measure steady-state.
    let conn = Connection::open(&db_path).expect("reopen");
    let _ = bandwidth_repo::load_current_cycle(&conn); // warm the cache.
    drop(conn);

    let rss_after = working_set_bytes();
    let delta = rss_after.saturating_sub(rss_before);

    // T-6: SQLite contribution <= 3 MiB; 2x ceiling (6 MiB) absorbs CI noise.
    // A 10x regression (30 MiB) would fail this loudly.
    assert!(
        delta <= T6_CEILING,
        "T-6: SQLite RSS contribution {delta} bytes exceeds {T6_CEILING} bytes (6 MiB ceiling). \
         rss_before={rss_before}, rss_after={rss_after}"
    );
}
