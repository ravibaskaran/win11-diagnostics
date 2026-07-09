//! `MonthlyAccumulator` — in-memory bandwidth accumulator keyed on LUID
//! (Story 5.1).
//!
//! Holds cumulative RX/TX byte totals per adapter LUID for the current
//! billing cycle. The accountant task (Story 5.2) feeds per-tick raw counter
//! readings in via [`MonthlyAccumulator::add_delta`]; the accumulator applies
//! the T-23 wraparound contract and tracks cumulative bytes.
//!
//! ## T-23 wraparound contract
//!
//! If a counter reading goes *backwards* relative to the previous tick
//! (`current < previous`), the adapter/source has reset (reboot, driver
//! reload, or genuine 64-bit wrap — the latter is theoretical on Win11). We
//! treat the reset as a fresh baseline: `delta = current` (not
//! `current - previous`). This keeps the cumulative total monotonic and
//! avoids negative/huge deltas.
//!
//! Cited: Story 5.1 Technical Context, nfr-thresholds.md T-23,
//! tdd-fixtures.md F7 (proptest for wraparound arithmetic).

use std::collections::HashMap;

use chrono::NaiveDate;

/// One LUID's accumulator state within the current cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccEntry {
    /// The cycle this entry belongs to (set on first sighting of the LUID
    /// this cycle; the accountant rolls over when the date crosses
    /// `cycle_end`).
    pub cycle_start: NaiveDate,
    /// Cumulative RX bytes accumulated this cycle.
    pub rx_bytes: u64,
    /// Cumulative TX bytes accumulated this cycle.
    pub tx_bytes: u64,
    /// Previous-tick raw RX counter (None on the very first tick → delta 0).
    pub prev_rx_counter: Option<u64>,
    /// Previous-tick raw TX counter.
    pub prev_tx_counter: Option<u64>,
}

impl AccEntry {
    /// Construct a fresh entry at the start of a cycle. Counters are `None`
    /// so the first `add_delta` establishes a baseline (delta 0).
    #[must_use]
    pub fn new(cycle_start: NaiveDate) -> Self {
        Self {
            cycle_start,
            rx_bytes: 0,
            tx_bytes: 0,
            prev_rx_counter: None,
            prev_tx_counter: None,
        }
    }
}

/// In-memory bandwidth accumulator keyed on adapter LUID. One entry per
/// tracked NIC for the current billing cycle.
#[derive(Debug, Default, Clone)]
pub struct MonthlyAccumulator {
    /// `adapter_luid → AccEntry`. HashMap because LUIDs are sparse 64-bit
    /// identifiers.
    pub by_luid: HashMap<u64, AccEntry>,
}

impl MonthlyAccumulator {
    /// Construct an empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one tick of raw counters for `luid`, applying the T-23 wraparound
    /// contract. On the first sighting of `luid` this cycle, an entry is
    /// created with `cycle_start` and the delta is 0 (baseline).
    ///
    /// # Arguments
    ///
    /// * `luid` — the adapter LUID (stable within a boot session, T-24).
    /// * `rx_counter` — the raw cumulative RX byte counter for this tick.
    /// * `tx_counter` — the raw cumulative TX byte counter for this tick.
    /// * `cycle_start` — the date the current billing cycle began. Only used
    ///   to initialize a fresh entry; an existing entry's cycle_start is NOT
    ///   overwritten (rollover is the accountant's job, Story 5.2).
    pub fn add_delta(
        &mut self,
        luid: u64,
        rx_counter: u64,
        tx_counter: u64,
        cycle_start: NaiveDate,
    ) {
        let entry = self
            .by_luid
            .entry(luid)
            .or_insert_with(|| AccEntry::new(cycle_start));

        // RX delta with T-23 wraparound.
        let rx_delta = match entry.prev_rx_counter {
            None => 0,                                     // first tick: baseline, no accumulation
            Some(prev) if rx_counter < prev => rx_counter, // T-23 reset
            Some(prev) => rx_counter - prev,               // normal forward delta
        };
        // TX delta, same logic.
        let tx_delta = match entry.prev_tx_counter {
            None => 0,
            Some(prev) if tx_counter < prev => tx_counter,
            Some(prev) => tx_counter - prev,
        };

        entry.rx_bytes = entry.rx_bytes.saturating_add(rx_delta);
        entry.tx_bytes = entry.tx_bytes.saturating_add(tx_delta);
        entry.prev_rx_counter = Some(rx_counter);
        entry.prev_tx_counter = Some(tx_counter);
    }

    /// Look up the cumulative entry for `luid`, if present.
    #[must_use]
    pub fn get(&self, luid: u64) -> Option<&AccEntry> {
        self.by_luid.get(&luid)
    }
}

#[cfg(test)]
mod tests {
    //! Story 5.1 TDD contract tests.
    //!
    //! Cited:
    //!   - Story 5.1 TDD contract (Happy Path #1-#2, Boundary #1-#4)
    //!   - nfr-thresholds.md T-23 (counter wraparound)
    //!   - tdd-fixtures.md F7 (proptest for wraparound arithmetic)
    //!   - guardrails.md G1 (RED before GREEN)

    use super::*;
    use chrono::NaiveDate;
    use proptest::prelude::*;

    /// Fixed cycle_start for tests (billing cycles start on a stable date).
    const CYCLE: NaiveDate = date(2026, 7, 1);

    const fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        // NaiveDate::from_ymd_opt is not const in chrono 0.4; use the
        // unwrap in a helper. (Tests only.)
        match NaiveDate::from_ymd_opt(y, m, d) {
            Some(d) => d,
            None => panic!("invalid test date"),
        }
    }

    // ----- Happy Path #1: two add_delta calls accumulate the delta -----

    /// Story 5.1 Happy Path #1. Cited: Story 5.1 TDD contract.
    #[test]
    fn two_ticks_accumulate_the_delta() {
        let mut acc = MonthlyAccumulator::new();
        acc.add_delta(1, 100, 50, CYCLE);
        acc.add_delta(1, 150, 70, CYCLE);
        let e = acc.get(1).expect("luid 1 present");
        assert_eq!(e.rx_bytes, 50, "150 - 100 = 50 RX");
        assert_eq!(e.tx_bytes, 20, "70 - 50 = 20 TX");
    }

    // ----- Happy Path #2: two LUIDs accumulate independently -----

    /// Story 5.1 Happy Path #2. Cited: Story 5.1 TDD contract.
    #[test]
    fn two_luids_accumulate_independently() {
        let mut acc = MonthlyAccumulator::new();
        acc.add_delta(1, 100, 10, CYCLE);
        acc.add_delta(2, 200, 20, CYCLE);
        acc.add_delta(1, 150, 30, CYCLE);
        acc.add_delta(2, 300, 40, CYCLE);
        assert_eq!(acc.get(1).unwrap().rx_bytes, 50);
        assert_eq!(acc.get(2).unwrap().rx_bytes, 100);
        assert_eq!(acc.get(1).unwrap().tx_bytes, 20);
        assert_eq!(acc.get(2).unwrap().tx_bytes, 20);
    }

    // ----- Boundary #1: T-23 wraparound -----

    /// Story 5.1 Boundary #1. Cited: T-23. prev=2e9, current=1000 → the
    /// counter went backwards → treated as reset → delta = 1000.
    #[test]
    fn wraparound_t23_treats_backwards_counter_as_reset() {
        let mut acc = MonthlyAccumulator::new();
        acc.add_delta(1, 2_000_000_000, 2_000_000_000, CYCLE); // baseline
        acc.add_delta(1, 1000, 1000, CYCLE); // wraparound
        let e = acc.get(1).unwrap();
        assert_eq!(e.rx_bytes, 1000, "T-23: backwards → delta = current");
        assert_eq!(e.tx_bytes, 1000);
    }

    // ----- Boundary #2: first call (prev=None) → delta 0 -----

    /// Story 5.1 Boundary #2. Cited: Story 5.1 TDD contract. The first tick
    /// establishes a baseline; no bytes are accumulated yet.
    #[test]
    fn first_call_accumulates_zero() {
        let mut acc = MonthlyAccumulator::new();
        acc.add_delta(1, 1_000_000, 500_000, CYCLE);
        let e = acc.get(1).unwrap();
        assert_eq!(e.rx_bytes, 0, "first tick = baseline, delta 0");
        assert_eq!(e.tx_bytes, 0);
        // prev_counter is now set so the next tick produces a real delta.
        assert_eq!(e.prev_rx_counter, Some(1_000_000));
    }

    // ----- Boundary #3: rx=0, tx=0 → no accumulation, no panic -----

    /// Story 5.1 Boundary #3. Cited: Story 5.1 TDD contract.
    #[test]
    fn zero_counters_do_not_accumulate_or_panic() {
        let mut acc = MonthlyAccumulator::new();
        acc.add_delta(1, 0, 0, CYCLE);
        acc.add_delta(1, 0, 0, CYCLE);
        let e = acc.get(1).unwrap();
        assert_eq!(e.rx_bytes, 0);
        assert_eq!(e.tx_bytes, 0);
    }

    // ----- Boundary #4: F7 proptest — cumulative rx_bytes == sum of deltas -----

    // Story 5.1 Boundary #4. Cited: T-23, F7. For any sequence of
    // monotonically-non-decreasing counters, the cumulative rx_bytes must
    // equal the sum of the per-tick deltas (= last - first, since monotonic).
    // (Wraparound sequences are covered by the explicit T-23 test above.)
    proptest! {
        #[test]
        fn cumulative_rx_equals_sum_of_monotonic_deltas(
            first in 0u64..100_000,
            steps in prop::collection::vec(0u64..50_000, 1..20)
        ) {
            let mut acc = MonthlyAccumulator::new();
            let mut cur = first;
            acc.add_delta(1, cur, 0, CYCLE); // baseline
            for step in &steps {
                cur = cur.saturating_add(*step);
                acc.add_delta(1, cur, 0, CYCLE);
            }
            // Monotonic non-decreasing → cumulative = last - first.
            let expected = cur.saturating_sub(first);
            prop_assert_eq!(acc.get(1).unwrap().rx_bytes, expected);
        }
    }
}
