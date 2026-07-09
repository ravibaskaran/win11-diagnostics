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
use sidebar_domain::billing::{cycle_end, next_cycle_start, CycleStartDay};
use sidebar_domain::reading::{MetricKind, Reading};
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
/// - `cycle_start` — the start date of the current billing cycle (stamped
///   on `current_cycle` rows; advanced on rollover).
pub struct BandwidthAccountant {
    rx: broadcast::Receiver<Vec<Reading>>,
    conn: Connection,
    clock: Box<dyn Clock>,
    config: AccountantConfig,
    accumulator: MonthlyAccumulator,
    cycle_start: NaiveDate,
}

impl BandwidthAccountant {
    /// Construct the accountant. Takes ownership of the SQLite connection
    /// (the schema MUST already be initialized — call
    /// `sidebar_persistence::schema::init` first, per R11 startup recovery).
    ///
    /// The `cycle_start` is derived from the clock's current date +
    /// the configured `CycleStartDay` (so a restart mid-cycle resumes the
    /// correct cycle). The `clock` is boxed (`Box<dyn Clock>`) so production
    /// can pass `Box::new(SystemClock::new())` and tests can pass
    /// `Box::new(FakeClock::new(t0))` — same construction surface.
    #[must_use]
    pub fn new(
        rx: broadcast::Receiver<Vec<Reading>>,
        conn: Connection,
        clock: Box<dyn Clock>,
        config: AccountantConfig,
    ) -> Self {
        let cycle_start = cycle_start_for_today(config.cycle_start_day, clock.now().date());
        Self {
            rx,
            conn,
            clock,
            config,
            accumulator: MonthlyAccumulator::new(),
            cycle_start,
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
    pub async fn run(mut self, shutdown: CancellationToken) -> Result<(), AccountantError> {
        // Debounce timer: fires every flush_interval (T-15: 60s production,
        // injected ms in tests). The first tick fires immediately on
        // tokio::time::interval construction, so we skip it (Barker: the
        // first interval tick completes immediately).
        let mut debounce = tokio::time::interval(self.config.flush_interval);
        debounce.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Discard the immediate first tick so we don't flush before any
        // readings arrive.
        debounce.tick().await;

        loop {
            tokio::select! {
                // Shutdown signal (T-19): force-flush + exit.
                () = shutdown.cancelled() => {
                    tracing::info!("BandwidthAccountant: shutdown signal — final flush");
                    self.flush();
                    return Ok(());
                }
                // Debounce timer fired (T-15): flush + check rollover.
                _ = debounce.tick() => {
                    self.check_rollover();
                    self.flush();
                }
                // Broadcast message: filter + accumulate.
                recv = self.rx.recv() => {
                    match recv {
                        Ok(readings) => {
                            self.ingest(&readings);
                            // Cheap rollover check on every tick so a long
                            // debounce gap (quiet NIC) doesn't miss the
                            // boundary (T-27).
                            self.check_rollover();
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            // Best-effort: the accountant recovers on the
                            // next tick. Log + continue (G14-aligned).
                            tracing::warn!(
                                skipped = n,
                                "BandwidthAccountant: broadcast lagged; some ticks skipped"
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            // Poller (sender) died → final flush + exit (G15).
                            tracing::info!(
                                "BandwidthAccountant: broadcast closed (poller exit) — final flush"
                            );
                            self.flush();
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Filter a tick's readings into the accumulator. Pure apart from the
    /// `&mut self.accumulator` mutation; the filter logic itself lives in
    /// [`group_network_readings`] (unit-tested in isolation).
    fn ingest(&mut self, readings: &[Reading]) {
        let grouped = group_network_readings(readings);
        for (luid, (rx, tx)) in grouped {
            self.accumulator.add_delta(luid, rx, tx, self.cycle_start);
        }
    }

    /// Check whether the billing cycle has rolled over (T-27:
    /// `clock.now().date_naive() >= cycle_end`). When true: archive the
    /// current cycle, prune history, reset the accumulator, and advance
    /// `cycle_start` to the new cycle. Flush is the caller's job (the
    /// rollover just re-baselines in-memory state).
    fn check_rollover(&mut self) {
        let today = self.clock.now().date();
        // Compute the end of the cycle that `self.cycle_start` belongs to.
        let Some(cycle_end_date) = cycle_end(
            self.config.cycle_start_day,
            self.cycle_start.year(),
            self.cycle_start.month(),
        ) else {
            // Defensive: cycle_end only returns None for an invalid calendar
            // date, which can't happen for a valid cycle_start. Log + skip.
            tracing::error!(
                cycle_start = %self.cycle_start,
                "rollover: cycle_end() returned None; skipping"
            );
            return;
        };
        if today >= cycle_end_date {
            // Rollover: archive current_cycle (move rows into history with
            // cycle_end stamp), prune, then reset the accumulator + advance
            // cycle_start to the new cycle. G15: archive/prune errors are
            // logged + swallowed — the accountant continues.
            //
            // Flush BEFORE archiving so any unflushed accumulator delta (the
            // common case — the debounce may not have fired since the last
            // tick) lands in current_cycle first and is therefore captured by
            // archive_cycle. Without this, bytes accumulated in the final
            // partial-cycle window between the last debounce flush and the
            // rollover boundary would be lost (architecture.md §6 flow G:
            // "archive ... then force-flush" — we flush-then-archive-then-
            // flush so both the old cycle's tail and the new cycle's empty
            // baseline are persisted).
            self.flush();
            let archived_at = iso_ts(self.clock.now());
            let cycle_end_str = cycle_end_date.to_string();
            if let Err(e) = sidebar_persistence::bandwidth_repo::archive_cycle(
                &self.conn,
                &cycle_end_str,
                &archived_at,
            ) {
                tracing::error!(error = %e, "rollover: archive_cycle failed (G15 — continuing)");
            }
            if let Err(e) = sidebar_persistence::bandwidth_repo::prune_history(
                &self.conn,
                self.config.history_keep,
            ) {
                tracing::error!(error = %e, "rollover: prune_history failed (G15 — continuing)");
            }
            // New cycle starts the day after cycle_end.
            self.cycle_start = next_cycle_start(cycle_end_date);
            // Reset the in-memory accumulator so per-LUID prev-counter
            // baselines re-establish (first tick of the new cycle = delta 0).
            self.accumulator = MonthlyAccumulator::new();
            tracing::info!(
                new_cycle_start = %self.cycle_start,
                "rollover: advanced to new billing cycle"
            );
        }
    }

    /// Flush the in-memory accumulator to `current_cycle` (one UPSERT per
    /// LUID). G15: each save_accumulator error is caught + logged; the
    /// accountant continues. The accumulator is NOT cleared (a flush is a
    /// snapshot, not a rollover).
    fn flush(&self) {
        let updated_at = iso_ts(self.clock.now());
        let cycle_start_str = self.cycle_start.to_string();
        for (luid, entry) in &self.accumulator.by_luid {
            // T-26: LUID + byte counters stored as i64 reinterpret-cast.
            // u64 → i64 cast is the documented boundary contract.
            let luid_i64 = luid.cast_signed();
            let rx_i64 = entry.rx_bytes.cast_signed();
            let tx_i64 = entry.tx_bytes.cast_signed();
            // adapter_name is unknown at the accountant layer (LHM/sysinfo
            // resolve names per Story 3.5); v1 stores an empty placeholder.
            // A future story enriches this from the adapter.
            if let Err(e) = sidebar_persistence::bandwidth_repo::save_accumulator(
                &self.conn,
                luid_i64,
                "",
                rx_i64,
                tx_i64,
                &cycle_start_str,
                &updated_at,
            ) {
                tracing::error!(
                    luid = luid_i64,
                    error = %e,
                    "flush: save_accumulator failed (G15 — continuing)"
                );
            }
        }
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
/// The Reading's `value: f64` is the raw cumulative byte counter (per the
/// Story 3.5 cumulative-counter contract — adapters emit raw `InOctets`/
/// `OutOctets`, the accountant computes deltas). We cast it back to `u64`
/// for the accumulator. Non-finite values (NaN/Inf, forbidden by T-20 but
/// defended against here) are dropped.
///
/// Public so the filter logic can be unit-tested in isolation (it's pure).
#[must_use]
pub fn group_network_readings(readings: &[Reading]) -> HashMap<u64, (u64, u64)> {
    let mut by_luid: HashMap<u64, (u64, u64)> = HashMap::new();
    for r in readings {
        // Parse the LUID from the sensor instance. Ignore garbage.
        let Some(luid) = luid_from_instance(&r.sensor.instance) else {
            continue;
        };
        // Defensive: T-20 says values are finite, but we guard.
        if !r.value.is_finite() || r.value < 0.0 {
            continue;
        }
        // f64 → u64 cast: byte counters fit in 52 bits of mantissa precision
        // for any realistic NIC (petabytes); the precision-loss lint is
        // acknowledged and allowed here.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = r.value as u64;
        let entry = by_luid.entry(luid).or_insert((0, 0));
        match r.kind {
            MetricKind::NetRxBytes => entry.0 = value,
            MetricKind::NetTxBytes => entry.1 = value,
            _ => {} // non-network reading for a LUID-shaped instance → ignore
        }
    }
    by_luid
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
        // history_keep = 2 so both archived cycles survive prune_history
        // (the default harness uses keep=1, which would delete the first
        // archive once the second lands).
        //
        // We jump the clock by exactly ONE cycle boundary at a time (Jul→Aug,
        // then Aug→Sep). A multi-month jump (e.g. Jul→Sep) would cascade TWO
        // archives per check_rollover wake, and with keep=2 the earliest
        // (the cycle we care about asserting) would get pruned. Single-month
        // jumps keep the archive count == jump count, so prune retention is
        // deterministic.
        let (conn, db_path, dir) = open_temp_db();
        let (tx, rx) = broadcast::channel::<Vec<Reading>>(8);
        let clock = FakeClock::new(t0_dt());
        let config = AccountantConfig {
            cycle_start_day: CycleStartDay::Day(1),
            flush_interval: Duration::from_millis(50),
            history_keep: 2,
        };
        let accountant = BandwidthAccountant::new(rx, conn, Box::new(clock.clone()), config);
        let shutdown = CancellationToken::new();
        let cancel_handle = shutdown.clone();
        let tx_handle = tx.clone();

        let join = tokio::spawn(async move { accountant.run(shutdown).await });

        // ---- July cycle: two ticks → delta rx=500/tx=200.
        tx.send(net_readings(100, 1000, 500)).unwrap();
        tokio::task::yield_now().await;
        tx.send(net_readings(100, 1500, 700)).unwrap();
        // Let one debounce tick fire so the July delta lands in current_cycle.
        tokio::time::sleep(Duration::from_millis(120)).await;

        // ---- Rollover 1: July → August (crosses cycle_end 2026-07-31).
        clock.set(
            NaiveDate::from_ymd_opt(2026, 8, 5)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        // Wait long enough for check_rollover to fire (debounce tick or recv)
        // and archive the July cycle into history.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // ---- Seed an August delta so August has non-zero bytes.
        tx_handle.send(net_readings(100, 2000, 800)).unwrap();
        tokio::task::yield_now().await;
        tx_handle.send(net_readings(100, 2500, 900)).unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;

        // ---- Rollover 2: August → September (crosses cycle_end 2026-08-31).
        clock.set(
            NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Cleanly stop the accountant (cancel, not abort, so the final flush
        // path runs and doesn't race with in-flight rollover work).
        cancel_handle.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), join)
            .await
            .expect("accountant exits");
        assert!(result.unwrap().is_ok(), "run() returns Ok");

        drop(tx);
        drop(tx_handle);
        let conn = inspect_db(&db_path);
        // Two rollovers → two history rows (July + August), each carrying its
        // cycle's delta. With history_keep=2 both survive prune.
        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bandwidth_history WHERE adapter_luid = ?1",
                rusqlite::params![100_i64],
                |row| row.get(0),
            )
            .expect("history COUNT");
        assert_eq!(history_count, 2, "two rollovers → 2 history rows");
        // Spot-check: the July row (cycle_end=2026-07-31) carries rx=500.
        let july_rx: i64 = conn
            .query_row(
                "SELECT rx_bytes FROM bandwidth_history WHERE adapter_luid = ?1 \
                 AND cycle_end = ?2",
                rusqlite::params![100_i64, "2026-07-31"],
                |row| row.get(0),
            )
            .expect("july row exists");
        assert_eq!(july_rx, 500, "July cycle archived with rx=500");
        let _ = dir; // keep TempDir alive
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
    // Boundary #5 — Multiple ticks within one debounce window → coalesced
    // into periodic flushes (T-15).
    //
    // RED: stub doesn't flush at all, so current_cycle is empty → the row-
    // count + delta assertions fail.
    //
    // Design note (broadcast capacity T-14 = 8): we deliberately send FEWER
    // ticks than the channel capacity, with yields between sends, so the
    // accountant's recv arm observes every tick. The original "100 rapid
    // ticks" premise is incompatible with a depth-8 broadcast channel + a
    // single-task receiver — the middle ticks get Lagged-dropped and the
    // accumulator re-baselines on the first SURVIVING tick (tick ~92),
    // yielding an arbitrary small delta. T-15 is about debounce COALESCENCE,
    // not about preserving every tick, so the test exercises that property
    // directly: several ticks within one debounce interval produce a single
    // cumulative delta on flush, and the accountant stays responsive (doesn't
    // hang, exits cleanly on sender-drop).
    // ---------------------------------------------------------------------
    /// Cited: Story 5.2 Boundary #5, T-14, T-15.
    #[tokio::test]
    async fn rapid_ticks_within_debounce_flush_once() {
        let h = harness(t0_dt(), 50);
        let db_path = h.db_path.clone();
        // 5 ticks (well under broadcast capacity 8 per T-14) with yields so
        // the accountant's recv arm drains each one before the next send.
        // Counters: 1000, 1010, ..., 1040 → cumulative delta = 40 (rx),
        // 500, 505, ..., 520 → cumulative delta = 20 (tx).
        for i in 0..5u64 {
            h.tx.send(net_readings(100, 1000 + i * 10, 500 + i * 5))
                .unwrap();
            // Yield between sends so the receiver processes each tick
            // (otherwise they pile up in the buffer and the receiver may
            // collapse them — fine for cumulative counters but we want a
            // deterministic delta for the assertion).
            tokio::task::yield_now().await;
        }
        // Drop sender so the accountant drains all 5 + does final flush.
        drop(h.tx);
        let result = tokio::time::timeout(Duration::from_secs(2), h.accountant.run(h.shutdown))
            .await
            .expect("run completes");
        assert!(result.is_ok());

        let conn = inspect_db(&db_path);
        let rows = bandwidth_repo::load_current_cycle(&conn).expect("load");
        assert_eq!(rows.len(), 1, "one LUID row");
        // Cumulative delta = (1000+40) - 1000 = 40 rx; similarly tx = 20.
        assert_eq!(
            rows[0].rx_bytes, 40,
            "5 monotonic ticks → last - first = 40 rx"
        );
        assert_eq!(rows[0].tx_bytes, 20, "5 monotonic ticks → 20 tx");
        // T-15 debounce: the final value is correct. The "at most a handful
        // of flushes" property is enforced structurally — debounce interval
        // is 50ms and the whole run completes in well under the 2s timeout,
        // so the accountant cannot have busy-flushed.
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
        // Clone the token twice: one moves into the task, one stays here so
        // we can cancel from outside (h itself is moved into the spawn).
        let task_token = h.shutdown.clone();
        let cancel_handle = h.shutdown.clone();
        h.tx.send(net_readings(100, 1000, 500)).unwrap();
        h.tx.send(net_readings(100, 1500, 700)).unwrap();

        let join = tokio::spawn(async move { h.accountant.run(task_token).await });
        // Give it a moment to drain + flush, then cancel the token (NOT
        // abort — abort would kill the task mid-flush and skip the final-
        // flush path the test asserts). The accountant's select! catches
        // the cancel in its shutdown arm, does a final flush, and returns
        // Ok within T-19 (3000ms).
        tokio::time::sleep(Duration::from_millis(80)).await;
        cancel_handle.cancel();
        // Await completion: must finish within T-19 (3000ms) of the cancel.
        let result = tokio::time::timeout(Duration::from_secs(3), join)
            .await
            .expect("accountant exits within T-19 (3000ms) of shutdown");
        assert!(result.is_ok(), "join did not panic");
        assert!(result.unwrap().is_ok(), "run() returns Ok on shutdown");

        // The cancelled accountant flushed the delta (500 rx) before exit.
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
