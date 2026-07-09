//! `BandwidthView` — pure-domain DTO the GUI renders bandwidth against
//! (Story 5.3).
//!
//! The GUI needs a single value-object holding (a) current-cycle totals per
//! NIC, (b) the last archived history row per NIC (for the "previous cycle"
//! comparison strip), (c) `days_until_reset`, and (d) the next reset date.
//! It MUST NOT touch SQLite, the network, or unsafe code — it's a pure
//! transformation of an in-memory accumulator + a borrowed history slice.
//!
//! # Why a DTO + builder
//!
//! - The GUI lives in `sidebar-ui` (a Tauri window) and shouldn't reach
//!   into either the accountant's accumulator or the persistence layer's
//!   connection handle directly (architecture.md layering).
//! - `build_view` is a single fn the GUI calls once per paint; it folds
//!   the accumulator + history into a value with no further IO.
//! - `days_until_reset` is the cycle-end countdown the user sees. We compute
//!   it here (clamped at 0) so the GUI can stay free of date arithmetic.
//!
//! # Cited
//!
//! - Story 5.3 spec (`docs/backlog/epics-and-stories.md`)
//! - architecture.md §6 flow I (accountant → DTO → GUI)
//! - nfr-thresholds.md T-27 (timezone: `clock.now().date_naive()`)
//! - tdd-fixtures.md F3 (FakeClock drives `days_until_reset`)

use chrono::NaiveDate;

use crate::accumulator::MonthlyAccumulator;
use crate::clock::Clock;

/// Per-NIC totals carried by [`BandwidthView`]. One entry per tracked LUID
/// for `current`, and one entry per archived cycle in `history`.
///
/// `friendly_name` is `None` in Story 5.3 — the GUI's NIC-name cache
/// (MIB_IF_ROW2.InterfaceAlias keyed by LUID) is a later integration. The
/// `Option<String>` keeps the field stable so adding the name later is a
/// non-breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NICtotals {
    /// Adapter LUID (64-bit, stable within a boot session — T-24).
    pub luid: u64,
    /// Friendly adapter name. `None` in 5.3; populated by a future NIC-name
    /// cache (the GUI side maps LUID → InterfaceAlias).
    pub friendly_name: Option<String>,
    /// Cumulative RX bytes for this entry.
    pub rx_bytes: u64,
    /// Cumulative TX bytes for this entry.
    pub tx_bytes: u64,
}

/// History-row source for [`build_view`]. Mirrors the columns of the
/// `bandwidth_history` table (AD-11) the persistence layer produces; we keep
/// only the fields the GUI cares about (LUID + byte totals) so the DTO does
/// not depend on rusqlite types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    /// Adapter LUID (reinterpret-cast back from i64 at the persistence
    /// boundary — T-26).
    pub luid: u64,
    /// Cumulative RX bytes for the archived cycle.
    pub rx_bytes: u64,
    /// Cumulative TX bytes for the archived cycle.
    pub tx_bytes: u64,
}

/// Pure-domain DTO the GUI renders bandwidth against.
///
/// - `current` — per-NIC totals for the in-progress billing cycle (one
///   entry per tracked LUID).
/// - `history` — per-NIC totals for archived cycles (typically one entry
///   per LUID; `history_keep` controls retention — T-16 default 1).
/// - `days_until_reset` — calendar days from today (inclusive of `today`)
///   to `next_reset_date`. Clamped at 0 once today is past the reset date.
/// - `next_reset_date` — the cycle_end date the countdown targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandwidthView {
    /// Per-NIC totals for the current cycle.
    pub current: Vec<NICtotals>,
    /// Per-NIC totals for archived cycles.
    pub history: Vec<NICtotals>,
    /// Days from today to `next_reset_date`, clamped at 0.
    pub days_until_reset: u32,
    /// The cycle-end date the countdown targets.
    pub next_reset_date: NaiveDate,
}

/// Build a [`BandwidthView`] from the accumulator + a borrowed history slice.
///
/// `days_until_reset` is `(cycle_end - clock.now().date_naive()).num_days()`
/// clamped to ≥ 0 (today past the reset date → 0, never negative).
/// `next_reset_date = cycle_end`.
///
/// `friendly_name` is left `None` per the 5.3 scope (NIC-name cache is a
/// later integration).
///
/// # Arguments
///
/// * `accumulator` — the accountant's in-memory state for the current cycle.
/// * `history` — borrowed slice of archived-cycle rows (typically produced
///   by `sidebar_persistence::bandwidth_repo` and reinterpreted here as
///   `HistoryRow` to keep the DTO free of rusqlite types).
/// * `cycle_end` — the current cycle's end date (the next reset date).
/// * `clock` — injectable wall-clock (F3); tests pass a `FakeClock`.
#[must_use]
pub fn build_view(
    accumulator: &MonthlyAccumulator,
    history: &[HistoryRow],
    cycle_end: NaiveDate,
    clock: &dyn Clock,
) -> BandwidthView {
    // Current-cycle entries: one NICtotals per tracked LUID. friendly_name
    // is None per 5.3 scope (NIC-name cache integration is a later story).
    let current: Vec<NICtotals> = accumulator
        .by_luid
        .iter()
        .map(|(&luid, entry)| NICtotals {
            luid,
            friendly_name: None,
            rx_bytes: entry.rx_bytes,
            tx_bytes: entry.tx_bytes,
        })
        .collect();

    // History entries: passthrough from the borrowed HistoryRow slice.
    // Disconnected NICs (present in history, absent in current) are
    // retained — the GUI shows the previous cycle even if the NIC is
    // dark this cycle (Story 5.3 Boundary #4).
    let history: Vec<NICtotals> = history
        .iter()
        .map(|row| NICtotals {
            luid: row.luid,
            friendly_name: None,
            rx_bytes: row.rx_bytes,
            tx_bytes: row.tx_bytes,
        })
        .collect();

    // days_until_reset = (cycle_end - today).num_days(), clamped to ≥ 0.
    // T-27: "today" is clock.now().date_naive(); the signed delta lets us
    // detect "today is past cycle_end" (negative) and clamp to 0.
    let today = clock.now().date();
    let delta_days = cycle_end.signed_duration_since(today).num_days();
    let days_until_reset: u32 = if delta_days <= 0 {
        0
    } else {
        u32::try_from(delta_days).unwrap_or(u32::MAX)
    };

    BandwidthView {
        current,
        history,
        days_until_reset,
        next_reset_date: cycle_end,
    }
}

#[cfg(test)]
mod tests {
    //! Story 5.3 TDD contract tests.
    //!
    //! One happy-path test + four boundary tests. Cited:
    //!   - Story 5.3 spec (`docs/backlog/epics-and-stories.md`)
    //!   - tdd-fixtures.md F3 (FakeClock drives `days_until_reset`)
    //!   - guardrails.md G1 (RED before GREEN)

    use super::*;
    use crate::accumulator::MonthlyAccumulator;
    use crate::clock::FakeClock;
    use chrono::NaiveDateTime;

    const GB: u64 = 1024 * 1024 * 1024;

    /// 2026-07-15T00:00:00 — fixture mid-cycle time.
    fn t(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    // ----- Happy Path #1: build view from accumulator + history -----

    /// Story 5.3 Happy Path. Cited: Story 5.3 TDD contract. Build from
    /// accumulator {luid=1, rx=1GB, tx=2GB} + history [{luid=1, rx=5GB,
    /// tx=6GB}] → 1 current entry + 1 history entry; `days_until_reset`
    /// via FakeClock. Cycle is 2026-07-01 → 2026-07-31 (Day(1)); today is
    /// 2026-07-15 → 31 - 15 = 16 days until reset.
    #[test]
    fn build_view_happy_path_one_current_one_history() {
        let mut acc = MonthlyAccumulator::new();
        // One tick with rx=1GB, tx=2GB → baseline (first tick = 0).
        // Need a SECOND tick to accumulate; or directly set. We use two
        // ticks: baseline at 0, then 1GB/2GB deltas.
        acc.add_delta(1, 0, 0, NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        acc.add_delta(1, GB, 2 * GB, NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());

        let history = vec![HistoryRow {
            luid: 1,
            rx_bytes: 5 * GB,
            tx_bytes: 6 * GB,
        }];

        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 7, 15));

        let view = build_view(&acc, &history, cycle_end, &clock);

        assert_eq!(view.current.len(), 1, "one current NIC entry");
        let cur = &view.current[0];
        assert_eq!(cur.luid, 1);
        assert_eq!(cur.rx_bytes, GB, "1GB rx");
        assert_eq!(cur.tx_bytes, 2 * GB, "2GB tx");
        assert_eq!(cur.friendly_name, None, "5.3 leaves name None");

        assert_eq!(view.history.len(), 1, "one history entry");
        let hist = &view.history[0];
        assert_eq!(hist.luid, 1);
        assert_eq!(hist.rx_bytes, 5 * GB);
        assert_eq!(hist.tx_bytes, 6 * GB);

        assert_eq!(view.next_reset_date, cycle_end);
        assert_eq!(view.days_until_reset, 16, "Jul 15 → Jul 31 = 16 days");
    }

    // ----- Boundary #1: empty accumulator + empty history → empty vecs,
    // days_until_reset = full cycle. -----

    /// Story 5.3 Boundary #1. Cited: Story 5.3 TDD contract. Today is the
    /// cycle_start (Jul 1); cycle_end is Jul 31 → 30 days until reset.
    #[test]
    fn empty_inputs_yield_empty_vecs_and_full_cycle_countdown() {
        let acc = MonthlyAccumulator::new();
        let history: Vec<HistoryRow> = Vec::new();
        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 7, 1));

        let view = build_view(&acc, &history, cycle_end, &clock);

        assert!(view.current.is_empty(), "empty current");
        assert!(view.history.is_empty(), "empty history");
        assert_eq!(
            view.days_until_reset, 30,
            "Jul 1 → Jul 31 = 30 days = full cycle"
        );
        assert_eq!(view.next_reset_date, cycle_end);
    }

    // ----- Boundary #2: today == cycle_end - 1 → 1. -----

    /// Story 5.3 Boundary #2. Cited: Story 5.3 TDD contract.
    #[test]
    fn days_until_reset_is_one_when_today_is_cycle_end_minus_one() {
        let acc = MonthlyAccumulator::new();
        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 7, 30)); // cycle_end - 1

        let view = build_view(&acc, &[], cycle_end, &clock);

        assert_eq!(view.days_until_reset, 1, "Jul 30 → Jul 31 = 1 day");
    }

    // ----- Boundary #3: today == cycle_end → 0. -----

    /// Story 5.3 Boundary #3. Cited: Story 5.3 TDD contract.
    #[test]
    fn days_until_reset_is_zero_when_today_is_cycle_end() {
        let acc = MonthlyAccumulator::new();
        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 7, 31)); // == cycle_end

        let view = build_view(&acc, &[], cycle_end, &clock);

        assert_eq!(view.days_until_reset, 0, "today == cycle_end → 0");
    }

    // ----- Boundary #4: NIC in history not in current (disconnected) →
    // history retained. -----

    /// Story 5.3 Boundary #4. Cited: Story 5.3 TDD contract. A NIC that was
    /// active last cycle but is now disconnected (not in the accumulator)
    /// MUST still appear in the history strip.
    #[test]
    fn disconnected_nic_retained_in_history() {
        let acc = MonthlyAccumulator::new();
        // No accumulator entry for luid=99 (disconnected this cycle).
        let history = vec![HistoryRow {
            luid: 99,
            rx_bytes: 3 * GB,
            tx_bytes: 4 * GB,
        }];
        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 7, 15));

        let view = build_view(&acc, &history, cycle_end, &clock);

        assert!(view.current.is_empty(), "no current NICs");
        assert_eq!(view.history.len(), 1, "disconnected NIC retained");
        assert_eq!(view.history[0].luid, 99);
        assert_eq!(view.history[0].rx_bytes, 3 * GB);
        assert_eq!(view.history[0].tx_bytes, 4 * GB);
    }

    // ----- Defensive clamp: today past cycle_end → 0 (never negative). -----

    /// Extra boundary: today is PAST cycle_end (rollover hasn't fired yet).
    /// Cited: Story 5.3 spec "clamped to ≥ 0".
    #[test]
    fn days_until_reset_clamped_at_zero_when_today_past_cycle_end() {
        let acc = MonthlyAccumulator::new();
        let cycle_end = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        let clock = FakeClock::new(t(2026, 8, 5)); // past cycle_end

        let view = build_view(&acc, &[], cycle_end, &clock);

        assert_eq!(
            view.days_until_reset, 0,
            "today past cycle_end → clamped to 0 (never negative)"
        );
    }
}
