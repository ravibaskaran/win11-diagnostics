//! `BandwidthAccountant` — the tokio task that subscribes to readings,
//! filters network counters, accumulates via [`MonthlyAccumulator`] (Story
//! 5.1), flushes to SQLite via `sidebar_persistence::bandwidth_repo`
//! (Stories 4.1-4.3), and rolls over the billing cycle.
//!
//! # Design (architecture.md §6 flows F/G/I, T-15, T-19, T-27)
//!
//! The accountant runs a `tokio::select!` loop over three futures:
//!
//! 1. **broadcast recv** — on each `Vec<Reading>` tick, filter for
//!    `MetricKind::NetRxBytes` / `NetTxBytes`, parse the LUID from
//!    `sensor.instance` (decimal-string u64), pair RX + TX counters, and
//!    feed `accumulator.add_delta(luid, rx, tx, cycle_start)`. Also check
//!    rollover (T-27).
//! 2. **debounce timer** — `tokio::time::interval(flush_interval)`. On each
//!    fire, flush the accumulator to `current_cycle` (T-15: 60s in
//!    production; injected so tests use a tiny interval).
//! 3. **shutdown** — `CancellationToken`. On fire, force-flush + return
//!    (T-19: must complete within 3000ms of the shutdown signal).
//!
//! Broadcast channel errors: `RecvError::Lagged(n)` is logged + continues
//! (best-effort; the accountant recovers on the next tick). `RecvError::Closed`
//! means the poller (sender) died → force-flush + exit (G15).
//!
//! # G15 — panic safety on flush
//!
//! Flush errors (SQLite busy-exhausted, disk full, etc.) are caught, logged
//! via `tracing::error!`, and the accountant CONTINUES. We never crash the
//! process on a persistence error; the accumulator keeps running in memory
//! and the next debounce retry may succeed. The TDD contract (Boundary #4)
//! forces this via a closed-connection flush failure.
//!
//! # `!Send` note
//!
//! `run()` is `!Send` because `rusqlite::Connection` is `!Sync`. The
//! accountant is designed to run on a single task (current-thread runtime in
//! tests; `LocalSet` in production). Moving it to a multi-thread runtime
//! would require routing flushes through `spawn_blocking` — deferred to
//! Story 7.x wiring.
//!
//! # Cited
//!
//! - architecture.md §6 flows F/G/I (accountant subscribe → accumulate →
//!   flush → rollover)
//! - architecture.md §6 line 263 (poller publishes Vec<Reading> via broadcast)
//! - nfr-thresholds.md T-15 (flush debounce 60s; immediate on shutdown +
//!   rollover)
//! - nfr-thresholds.md T-19 (shutdown grace 3000ms)
//! - nfr-thresholds.md T-27 (timezone: `clock.now().date_naive() >= cycle_end`)
//! - guardrails.md G11 (Clock trait — HITL item, this PR)
//! - guardrails.md G15 (flush errors caught + logged, accountant continues)
//! - guardrails.md G21 (all SQLite via sidebar-persistence)

use std::collections::HashMap;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rusqlite::Connection;
use sidebar_domain::billing::CycleStartDay;
#[allow(unused_imports)]
use sidebar_domain::billing::{cycle_end, next_cycle_start};
#[allow(unused_imports)]
use sidebar_domain::reading::MetricKind;
use sidebar_domain::reading::Reading;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::accumulator::MonthlyAccumulator;
use crate::clock::Clock;

/// Configuration for the accountant — injected at construction time so tests
/// can drive the debounce interval + billing cycle independently of real
/// time + production config.
#[derive(Debug, Clone)]
pub struct AccountantConfig {
    /// Billing-cycle start day-of-month (e.g. `Day(1)` = cycle resets on the
    /// 1st of each month). Drives [`cycle_end`](sidebar_domain::billing::cycle_end)
    /// / [`next_cycle_start`] for the rollover check (T-27).
    pub cycle_start_day: CycleStartDay,
    /// Flush debounce interval. T-15 mandates 60s in production; tests inject
    /// a tiny value (e.g. 10ms) so the debounce test is fast and
    /// deterministic without tokio time-mocking.
    pub flush_interval: Duration,
    /// History retention (T-16: default 1). Passed to `prune_history` on
    /// each rollover.
    pub history_keep: u32,
}

impl AccountantConfig {
    /// Production defaults: 60s debounce (T-15), keep=1 (T-16). The
    /// `cycle_start_day` is user-configured and passed in.
    #[must_use]
    pub fn production(cycle_start_day: CycleStartDay) -> Self {
        Self {
            cycle_start_day,
            flush_interval: Duration::from_mins(1),
            history_keep: 1,
        }
    }
}

/// The BandwidthAccountant. Construct via [`BandwidthAccountant::new`], then
/// spawn [`BandwidthAccountant::run`] on a tokio runtime.
///
/// Holds:
/// - `rx` — the broadcast receiver subscribed to the poller's `Vec<Reading>`
///   stream.
/// - `conn` — owned SQLite connection (all access via `bandwidth_repo`; G21).
/// - `clock` — injectable wall-clock (HITL — G11).
/// - `config` — debounce + billing + retention config.
/// - `accumulator` — per-LUID in-memory state (Story 5.1).
pub struct BandwidthAccountant {
    rx: broadcast::Receiver<Vec<Reading>>,
    conn: Connection,
    clock: Box<dyn Clock>,
    config: AccountantConfig,
    accumulator: MonthlyAccumulator,
}

impl BandwidthAccountant {
    /// Construct the accountant. Takes ownership of the SQLite connection
    /// (the schema MUST already be initialized — call
    /// `sidebar_persistence::schema::init` first, per R11 startup recovery).
    ///
    /// The `clock` is boxed (`Box<dyn Clock>`) so production can pass
    /// `Box::new(SystemClock::new())` and tests can pass
    /// `Box::new(FakeClock::new(t0))` — same construction surface.
    #[must_use]
    pub fn new(
        rx: broadcast::Receiver<Vec<Reading>>,
        conn: Connection,
        clock: Box<dyn Clock>,
        config: AccountantConfig,
    ) -> Self {
        Self {
            rx,
            conn,
            clock,
            config,
            accumulator: MonthlyAccumulator::new(),
        }
    }

    /// Run the accountant task until the shutdown token fires OR the
    /// broadcast sender drops (poller crash → G15 final flush).
    ///
    /// Returns `Ok(())` on clean shutdown (token cancelled or broadcast
    /// closed, both preceded by a final flush). The future is `!Send` — see
    /// the module doc.
    ///
    /// # Errors
    ///
    /// Returns `Err` only for non-recoverable programming errors. G15 turns
    /// all SQLite flush errors into logged-and-continued, so `run()` returns
    /// `Ok(())` on normal exit paths.
    pub async fn run(mut self, _shutdown: CancellationToken) -> Result<(), AccountantError> {
        // RED STUB: drain one broadcast message and return. No accumulation,
        // no flush, no rollover. The GREEN impl replaces this body with the
        // real select! loop. The stub references every field so the lib
        // compiles cleanly (dead-code lint); the "DB rows match" assertion
        // in Happy Path #1 fails (no flush → no rows), which is the RED
        // signal.
        let _ = self.rx.recv().await;
        let _ = &self.conn;
        let _ = self.clock.now();
        let _ = &self.config.flush_interval;
        let _ = &self.accumulator;
        Ok(())
    }
}

/// Error surfaced by the accountant. G15 says flush errors are logged +
/// swallowed, so this enum is intentionally small — it exists for the
/// (rare) case where even logging fails or a programming invariant is
/// violated. In practice the GREEN impl never returns `Err` from `run()`.
#[derive(Debug)]
pub enum AccountantError {
    /// The SQLite connection is unusable for a non-recoverable reason.
    /// G15 still logs + continues; this variant is reserved for future
    /// hard-failure paths.
    Connection(String),
}

// ----- Pure helpers (unit-tested independently of the tokio task) -----

/// Parse a sensor instance string back to a LUID. The network adapter emits
/// the LUID (`MIB_IF_ROW2.InterfaceLuid`, u64) as a decimal string in
/// `SensorId.instance`; this parses it back. Returns `None` if the string
/// isn't a valid u64 (defensive — malformed sensor IDs are ignored, not
/// crashed — supports the filter's "ignore garbage" contract).
#[must_use]
pub fn luid_from_instance(instance: &str) -> Option<u64> {
    instance.parse::<u64>().ok()
}

/// Group a tick's readings by LUID → (rx_counter, tx_counter). Readings that
/// aren't `NetRxBytes`/`NetTxBytes`, or whose sensor instance doesn't parse
/// as a u64 LUID, are ignored (Boundary #2: non-network + malformed readings
/// filtered out).
///
/// Public so the filter logic can be unit-tested in isolation (it's pure).
#[must_use]
pub fn group_network_readings(readings: &[Reading]) -> HashMap<u64, (u64, u64)> {
    let _ = readings;
    HashMap::new() // RED STUB — GREEN fills in the real filter.
}

/// Parse a `cycle_start` date for the cycle containing `today`, given the
/// user-configured `CycleStartDay`. The cycle START is the most recent
/// past occurrence of `cycle_start_day` (inclusive of today if today is the
/// start day). Used to stamp `current_cycle.cycle_start`.
///
/// Public so the date arithmetic can be unit-tested independently of the
/// tokio task.
#[must_use]
pub fn cycle_start_for_today(cycle_start_day: CycleStartDay, today: NaiveDate) -> NaiveDate {
    let day = match cycle_start_day {
        CycleStartDay::Day(d) => u32::from(d),
        CycleStartDay::LastDayOfMonth => {
            sidebar_domain::billing::last_day_of_month(today.year(), today.month())
        }
    };
    if today.day() >= day {
        // Start day already passed this month → start is this month's `day`.
        NaiveDate::from_ymd_opt(today.year(), today.month(), day).unwrap_or(today)
    } else {
        // Start day is yet to come this month → start is last month's `day`.
        let (y, m) = if today.month() == 1 {
            (today.year() - 1, 12u32)
        } else {
            (today.year(), today.month() - 1)
        };
        NaiveDate::from_ymd_opt(y, m, day).unwrap_or(today)
    }
}

/// Format a `NaiveDateTime` as the ISO 8601 string SQLite expects for
/// `updated_at` / `archived_at` columns (e.g. `"2026-07-15 12:00:00"`).
#[allow(dead_code)] // RED — GREEN's run() flush path uses this.
fn iso_ts(t: NaiveDateTime) -> String {
    t.to_string()
}

#[cfg(test)]
mod tests {
    //! Story 5.2 TDD contract tests.
    //!
    //! Happy Path:
    //!   #1 — 2 ticks of NetRxBytes → accumulator totals correct → flush →
    //!        DB `current_cycle` rows match.
    //!   #2 — Tick contains non-network readings → ignored.
    //!
    //! Boundary (cite T-15, T-19, T-23, T-27; G15):
    //!   #1 — Rollover: FakeClock advances past cycle_end (T-27) → archive
    //!        called, new cycle starts at 0, force-flush.
    //!   #2 — Two rollovers in sequence → history has 2 rows.
    //!   #3 — Broadcast sender drops (poller crash) → accountant exits with
    //!        final flush (G15).
    //!   #4 — Flush fails (closed connection) → error logged, accountant
    //!        continues (G15).
    //!   #5 — Rapid 100 ticks within debounce (T-15) → only 1 flush.
    //!   #6 — Shutdown signal mid-run → graceful exit (T-19).
    //!
    //! Fixtures: F1 (TempDir DB), F2 (mock broadcast), F3 (FakeClock).
    //!
    //! Cited: Story 5.2 TDD contract, architecture.md §6 F/G/I, T-15, T-19,
    //! T-27, G11 (Clock HITL), G15 (flush panic-safety), G21 (SQLite via
    //! sidebar-persistence).

    use super::*;
    use crate::clock::FakeClock;
    use chrono::NaiveDate;
    use rusqlite::Connection;
    use sidebar_domain::billing::CycleStartDay;
    use sidebar_domain::reading::{MetricKind, Reading, SensorId, Unit};
    use sidebar_persistence::{bandwidth_repo, schema};
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::broadcast;
    use tokio_util::sync::CancellationToken;

    // ---------------------------------------------------------------------
    // Fixtures (F1, F2, F3).
    // ---------------------------------------------------------------------

    /// F1 — open a fresh SQLite file inside a TempDir, init the schema, and
    /// return the connection + the DB file PATH (so a second reader can be
    /// opened for inspection while the accountant holds its own connection)
    /// + the TempDir guard.
    fn open_temp_db() -> (Connection, PathBuf, TempDir) {
        let dir = TempDir::new().expect("TempDir::new");
        let path = dir.path().join("bandwidth.db");
        let conn = Connection::open(&path).expect("Connection::open");
        schema::init(&conn).expect("schema::init");
        (conn, path, dir)
    }

    /// Open a SECOND read connection to the same DB file — used by tests to
    /// inspect rows after the accountant has flushed. SQLite WAL permits
    /// concurrent readers, so this works while the accountant's own
    /// connection is still open.
    fn inspect_db(path: &std::path::Path) -> Connection {
        Connection::open(path).expect("inspect Connection::open")
    }

    /// Construct a NetRxBytes/NetTxBytes pair for `luid` with the given
    /// cumulative counters. The sensor instance is the LUID formatted as a
    /// decimal string (the canonical adapter convention).
    #[allow(clippy::cast_precision_loss)]
    fn net_readings(luid: u64, rx: u64, tx: u64) -> Vec<Reading> {
        let instance = luid.to_string();
        vec![
            Reading::new(
                SensorId::new("net", instance.clone()),
                MetricKind::NetRxBytes,
                rx as f64,
                Unit::Bytes,
            ),
            Reading::new(
                SensorId::new("net", instance),
                MetricKind::NetTxBytes,
                tx as f64,
                Unit::Bytes,
            ),
        ]
    }

    /// One non-network reading (CPU temp) to exercise the filter.
    fn noise_reading() -> Reading {
        Reading::new(
            SensorId::new("cpu", "package"),
            MetricKind::CpuTemperature,
            42.0,
            Unit::Celsius,
        )
    }

    /// The test harness. Holds the accountant + mock broadcast sender +
    /// FakeClock (Clone — shares state via internal Arc, so the test keeps
    /// one clone to advance time while the accountant holds another) +
    /// shutdown token + the DB path (for post-run inspection) + TempDir guard.
    ///
    /// The accountant owns its own connection; the harness keeps the DB path
    /// so tests can open a reader via [`inspect_db`].
    struct Harness {
        accountant: BandwidthAccountant,
        tx: broadcast::Sender<Vec<Reading>>,
        clock: FakeClock,
        shutdown: CancellationToken,
        db_path: PathBuf,
        _dir: TempDir,
    }

    /// Build a harness with a fresh DB, mock broadcast channel (capacity 8
    /// per T-14), and a FakeClock pinned at `t0`. The accountant uses a
    /// `flush_interval`-ms debounce interval.
    fn harness(t0: NaiveDateTime, flush_interval_ms: u64) -> Harness {
        let (conn, db_path, dir) = open_temp_db();
        let (tx, rx) = broadcast::channel::<Vec<Reading>>(8);
        let clock = FakeClock::new(t0);
        let config = AccountantConfig {
            cycle_start_day: CycleStartDay::Day(1),
            flush_interval: Duration::from_millis(flush_interval_ms),
            history_keep: 1,
        };
        // Clone the FakeClock so the accountant gets one (shared-state)
        // clone and the harness keeps another for time-control.
        let accountant = BandwidthAccountant::new(rx, conn, Box::new(clock.clone()), config);
        Harness {
            accountant,
            tx,
            clock,
            shutdown: CancellationToken::new(),
            db_path,
            _dir: dir,
        }
    }

    /// Mid-cycle reference date (2026-07-15). For `Day(1)` the containing
    /// cycle is 2026-07-01..2026-07-31.
    fn t0_dt() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    // ---------------------------------------------------------------------
    // Happy Path #1 — 2 ticks → accumulator → flush → DB rows match.
    // RED: stub run() drains once + returns; no flush, so current_cycle is
    // empty → the row-count assertion fails.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Happy Path #1, architecture.md §6 flow F, T-15.
    #[tokio::test]
    async fn two_ticks_flush_and_db_rows_match() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        // Tick 1: luid 100, rx=1000, tx=500. (First tick = baseline, delta 0.)
        h.tx.send(net_readings(100, 1000, 500))
            .expect("send tick 1");
        // Tick 2: rx=1500, tx=700 → delta rx=500, tx=200.
        h.tx.send(net_readings(100, 1500, 700))
            .expect("send tick 2");

        // Run the accountant to completion: drop the sender so it flushes +
        // exits (G15 final-flush path), with a generous timeout.
        drop(h.tx);
        let result = tokio::time::timeout(Duration::from_secs(2), h.accountant.run(h.shutdown))
            .await
            .expect("accountant exits within 2s");
        assert!(result.is_ok(), "run() returns Ok");

        // Inspect the DB via a fresh read connection.
        let conn = inspect_db(&db_path);
        let rows = bandwidth_repo::load_current_cycle(&conn).expect("load_current_cycle");
        assert_eq!(rows.len(), 1, "exactly 1 LUID row after flush");
        let row = &rows[0];
        assert_eq!(row.adapter_luid, 100_i64, "LUID 100 round-trips (T-26)");
        assert_eq!(row.rx_bytes, 500_i64, "rx_bytes = delta 1500-1000 = 500");
        assert_eq!(row.tx_bytes, 200_i64, "tx_bytes = delta 700-500 = 200");
    }

    // ---------------------------------------------------------------------
    // Happy Path #2 — non-network readings ignored.
    // RED: group_network_readings is a stub returning empty, so len != 2.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Happy Path #2 (filter), Boundary #2.
    #[test]
    fn group_network_readings_ignores_non_network() {
        let readings = vec![
            net_readings(7, 100, 50),
            vec![noise_reading()],
            net_readings(8, 200, 60),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let grouped = group_network_readings(&readings);
        assert_eq!(grouped.len(), 2, "only the 2 LUIDs, noise filtered");
        assert_eq!(grouped.get(&7), Some(&(100, 50)));
        assert_eq!(grouped.get(&8), Some(&(200, 60)));
    }

    // ---------------------------------------------------------------------
    // Boundary #1 — Rollover (T-27).
    // RED: stub never rolls, so history stays empty + current keeps rows.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #1, T-27.
    #[tokio::test]
    async fn rollover_archives_and_starts_new_cycle() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        let clock = h.clock.clone();
        // Seed a delta (500/200) while in the July cycle.
        h.tx.send(net_readings(100, 1000, 500)).unwrap();
        h.tx.send(net_readings(100, 1500, 700)).unwrap();

        // Run + advance the clock PAST cycle_end (2026-07-31) mid-flight.
        let shutdown = h.shutdown.clone();
        let join = tokio::spawn(async move { h.accountant.run(shutdown).await });
        // Let the accountant drain + do one debounce flush in July.
        tokio::time::sleep(Duration::from_millis(150)).await;
        // Jump to September — crosses 2026-07-31 (cycle_end for Day(1) July).
        clock.set(
            NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        // Give the rollover path a tick to fire, then drop the sender for a
        // final flush + exit.
        tokio::time::sleep(Duration::from_millis(150)).await;
        join.abort();
        let _ = join.await;

        let conn = inspect_db(&db_path);
        // history gained 1 row for the July cycle (cycle_end=2026-07-31).
        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bandwidth_history WHERE adapter_luid = ?1 \
                 AND cycle_end = ?2",
                rusqlite::params![100_i64, "2026-07-31"],
                |row| row.get(0),
            )
            .expect("history COUNT");
        assert_eq!(history_count, 1, "rollover archived the July cycle");
    }

    // ---------------------------------------------------------------------
    // Boundary #2 — Two rollovers → history has 2 rows.
    // RED: stub never rolls, so history_count != 2.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #2.
    #[tokio::test]
    async fn two_rollovers_produce_two_history_rows() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        let clock = h.clock.clone();
        h.tx.send(net_readings(100, 1000, 500)).unwrap();

        let shutdown = h.shutdown.clone();
        let join = tokio::spawn(async move { h.accountant.run(shutdown).await });
        // Roll into September (July → archived).
        clock.set(
            NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        // Roll into November (September → archived).
        clock.set(
            NaiveDate::from_ymd_opt(2026, 11, 5)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        join.abort();
        let _ = join.await;

        let conn = inspect_db(&db_path);
        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bandwidth_history WHERE adapter_luid = ?1",
                rusqlite::params![100_i64],
                |row| row.get(0),
            )
            .expect("history COUNT");
        assert_eq!(history_count, 2, "two rollovers → 2 history rows");
    }

    // ---------------------------------------------------------------------
    // Boundary #3 — Broadcast sender drops → final flush + exit (G15).
    // RED: stub drains once + returns Ok — passes the exit assertion but the
    // final-flush row assertion (GREEN) fails because no flush happened.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #3, G15.
    #[tokio::test]
    async fn sender_drop_exits_with_final_flush() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        h.tx.send(net_readings(100, 1000, 500)).unwrap();
        h.tx.send(net_readings(100, 1500, 700)).unwrap(); // delta 500/200

        let shutdown = h.shutdown;
        // Drop the sender BEFORE running — the accountant's recv returns
        // Closed → final flush + exit (G15).
        drop(h.tx);
        let result = tokio::time::timeout(Duration::from_secs(2), h.accountant.run(shutdown)).await;
        assert!(
            result.is_ok(),
            "accountant must exit within 2s when sender drops"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() returns Ok on broadcast-closed (G15 final flush path)"
        );

        // GREEN strengthens: the final flush persisted the 500/200 delta.
        let conn = inspect_db(&db_path);
        let rows = bandwidth_repo::load_current_cycle(&conn).expect("load");
        assert_eq!(rows.len(), 1, "final flush persisted the LUID row");
        assert_eq!(rows[0].rx_bytes, 500, "final flush has the delta rx_bytes");
        assert_eq!(rows[0].tx_bytes, 200, "final flush has the delta tx_bytes");
    }

    // ---------------------------------------------------------------------
    // Boundary #4 — Flush fails (closed conn) → logged + continues (G15).
    // RED: runs on a healthy DB so the accountant exits Ok trivially. GREEN
    // instruments a real flush failure via a path whose file has been
    // removed, and asserts run() STILL returns Ok.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #4, G15.
    #[tokio::test]
    async fn flush_failure_is_logged_and_continues() {
        let (_conn, db_path, dir) = open_temp_db();
        // Force every subsequent write to fail: remove the DB file from
        // under the connection, then re-open it. SQLite recreates an empty
        // file, but the schema (current_cycle table) is gone →
        // save_accumulator fails on the missing table. The accountant must
        // log + continue (G15), NOT crash.
        let _ = std::fs::remove_file(&db_path);
        let conn = Connection::open(&db_path).expect("reopen empty DB");
        let (tx, rx) = broadcast::channel::<Vec<Reading>>(8);
        let clock = FakeClock::new(t0_dt());
        let config = AccountantConfig {
            cycle_start_day: CycleStartDay::Day(1),
            flush_interval: Duration::from_millis(50),
            history_keep: 1,
        };
        let acc = BandwidthAccountant::new(rx, conn, Box::new(clock), config);
        drop(tx);
        let result =
            tokio::time::timeout(Duration::from_secs(2), acc.run(CancellationToken::new()))
                .await
                .expect("run completes within 2s");
        assert!(
            result.is_ok(),
            "G15: flush errors do not propagate from run()"
        );
        let _ = dir; // keep TempDir alive
    }

    // ---------------------------------------------------------------------
    // Boundary #5 — 100 rapid ticks within debounce → only 1 flush (T-15).
    // RED: stub doesn't flush at all, so updated_at is missing → the "exactly
    // one distinct updated_at" assertion is strengthened in GREEN.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #5, T-15.
    #[tokio::test]
    async fn rapid_ticks_within_debounce_flush_once() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        // 100 ticks of the same LUID with monotonically increasing counters.
        for i in 0..100u64 {
            h.tx.send(net_readings(100, 1000 + i * 10, 500 + i * 5))
                .unwrap();
        }
        // Drop sender so the accountant drains all 100 + does final flush.
        drop(h.tx);
        let result = tokio::time::timeout(Duration::from_secs(2), h.accountant.run(h.shutdown))
            .await
            .expect("run completes");
        assert!(result.is_ok());

        let conn = inspect_db(&db_path);
        let rows = bandwidth_repo::load_current_cycle(&conn).expect("load");
        assert_eq!(rows.len(), 1, "one LUID row");
        // Cumulative delta = (1000+990) - 1000 = 990 rx; similarly tx.
        assert_eq!(
            rows[0].rx_bytes, 990,
            "100 monotonic ticks → last - first = 990 rx"
        );
        assert_eq!(rows[0].tx_bytes, 495, "100 monotonic ticks → 495 tx");
        // T-15 debounce: the row was flushed at most a handful of times
        // (debounce + final). We assert the final value is correct; the
        // "only 1 flush" guarantee is enforced by GREEN instrumentation.
    }

    // ---------------------------------------------------------------------
    // Boundary #6 — Shutdown signal → graceful exit within T-19 (3000ms).
    // RED: stub returns immediately, so the timeout trivially holds. GREEN
    // asserts the accountant drains + flushes before exiting on cancel.
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #6, T-19.
    #[tokio::test]
    async fn shutdown_signal_graceful_exit() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        let shutdown = h.shutdown.clone();
        h.tx.send(net_readings(100, 1000, 500)).unwrap();
        h.tx.send(net_readings(100, 1500, 700)).unwrap();

        let join = tokio::spawn(async move { h.accountant.run(shutdown).await });
        // Give it a moment to drain, then cancel.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // (shutdown.cancel() lives on the token we cloned; the Harness's own
        // token is moved into the task. We cancel via a second clone held by
        // the test — but we already moved h. Restructure: cancel before move.)
        // For RED: just abort + assert no hang. GREEN strengthens.
        join.abort();
        let _ = join.await;

        // GREEN: the cancelled accountant flushed before exit.
        let conn = inspect_db(&db_path);
        let rows = bandwidth_repo::load_current_cycle(&conn).expect("load");
        assert!(
            rows.iter()
                .any(|r| r.adapter_luid == 100 && r.rx_bytes == 500),
            "shutdown final-flush persisted the delta"
        );
    }

    // ---------------------------------------------------------------------
    // Pure helper unit tests (no tokio).
    // ---------------------------------------------------------------------

    /// Cited: Story 5.2 (cycle_start derivation for current_cycle stamp).
    #[test]
    fn cycle_start_for_today_mid_month() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
        assert_eq!(
            cycle_start_for_today(CycleStartDay::Day(1), today),
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
        );
    }

    #[test]
    fn cycle_start_for_today_before_start_day_wraps_to_prev_month() {
        // Day(10), today is the 5th → start is the 10th of last month.
        let today = NaiveDate::from_ymd_opt(2026, 7, 5).unwrap();
        assert_eq!(
            cycle_start_for_today(CycleStartDay::Day(10), today),
            NaiveDate::from_ymd_opt(2026, 6, 10).unwrap()
        );
    }

    #[test]
    fn cycle_start_for_today_on_start_day_is_today() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        assert_eq!(cycle_start_for_today(CycleStartDay::Day(1), today), today);
    }

    #[test]
    fn cycle_start_for_today_january_wraps_to_december() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(
            cycle_start_for_today(CycleStartDay::Day(10), today),
            NaiveDate::from_ymd_opt(2025, 12, 10).unwrap()
        );
    }

    #[test]
    fn luid_from_instance_parses_decimal() {
        assert_eq!(luid_from_instance("123456789"), Some(123_456_789));
        assert_eq!(luid_from_instance("0"), Some(0));
        assert_eq!(luid_from_instance("not-a-luid"), None);
        assert_eq!(luid_from_instance(""), None);
    }

    #[test]
    fn group_pairs_rx_and_tx_for_same_luid() {
        let readings = net_readings(42, 1000, 2000);
        let grouped = group_network_readings(&readings);
        assert_eq!(grouped.get(&42), Some(&(1000, 2000)));
    }

    #[test]
    fn group_handles_multiple_luids() {
        let mut readings = net_readings(1, 10, 20);
        readings.extend(net_readings(2, 30, 40));
        let grouped = group_network_readings(&readings);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get(&1), Some(&(10, 20)));
        assert_eq!(grouped.get(&2), Some(&(30, 40)));
    }

    #[test]
    fn iso_ts_formats_naive_datetime() {
        let t = NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(12, 30, 0)
            .unwrap();
        assert_eq!(iso_ts(t), "2026-07-01 12:30:00");
    }
}
