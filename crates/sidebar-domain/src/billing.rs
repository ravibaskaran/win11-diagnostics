//! Story 1.4 — Billing cycle pure functions.
//!
//! Defines `CycleStartDay` (the user-configurable billing-cycle start
//! day-of-month) and pure functions for computing cycle end dates and
//! next-cycle start dates. All arithmetic uses `chrono::NaiveDate` (no
//! timezone per T-27).
//!
//! Cited: Story 1.4, PRD §5.5 (Monthly Bandwidth Tracking),
//! nfr-thresholds.md T-25/T-26/T-27.

/// User-configurable billing-cycle start day-of-month.
///
/// `Day(d)` accepts 1–28 (T-26). Days 29–31 are excluded because they don't
/// exist in every month (February has 28 or 29). Users wanting a cycle that
/// starts at month-end use `LastDayOfMonth`.
///
/// Construction with `day(0)` or `day(29+)` panics in debug builds; in
/// release builds it clamps to `Day(1)` or `Day(28)` respectively and logs
/// a warning (per T-26 contract, approved by the user 2026-07-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CycleStartDay(CycleStartDayKind);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CycleStartDayKind {
    Day(u8),
    LastDayOfMonth,
}

impl CycleStartDay {
    /// Start on the last day of the month (28th in Feb non-leap, 31st in Jan,
    /// etc.). This associated constant preserves the original call syntax
    /// while keeping the representation private.
    #[allow(non_upper_case_globals)]
    pub const LastDayOfMonth: Self = Self(CycleStartDayKind::LastDayOfMonth);

    /// Construct a `Day(d)` by clamping untrusted configuration input to the
    /// T-26 range. Unlike [`Self::day`], this path never panics in debug builds.
    #[must_use]
    pub fn clamped_day(d: u8) -> Self {
        let clamped = d.clamp(1, 28);
        if clamped != d {
            tracing::warn!(original = d, clamped, "T-26: Day out of range; clamped");
        }
        Self(CycleStartDayKind::Day(clamped))
    }

    /// Construct a `Day(d)` with T-26 invariant enforcement.
    ///
    /// In debug builds: panics if `d < 1 || d > 28`.
    /// In release builds: clamps to [1, 28] + logs via `tracing::warn!`.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `d` is not in `[1, 28]`.
    #[must_use]
    #[cfg(debug_assertions)]
    pub fn day(d: u8) -> Self {
        assert!(
            (1..=28).contains(&d),
            "T-26: CycleStartDay::Day({d}) out of range [1, 28]. \
             In release builds this clamps to [1, 28]."
        );
        Self(CycleStartDayKind::Day(d))
    }

    /// Construct a `Day(d)` with T-26 invariant enforcement (release build).
    #[must_use]
    #[cfg(not(debug_assertions))]
    pub fn day(d: u8) -> Self {
        Self::clamped_day(d)
    }

    /// Return the validated fixed day, or `None` for month-end cycles.
    #[must_use]
    pub fn day_value(self) -> Option<u8> {
        match self {
            Self(CycleStartDayKind::Day(day)) => Some(day),
            Self(CycleStartDayKind::LastDayOfMonth) => None,
        }
    }
}

/// Compute the end date of a billing cycle.
///
/// A cycle starts on `cycle_start_day` in `year`/`month`. The cycle END is
/// the day BEFORE the start day of the NEXT cycle.
#[must_use]
pub fn cycle_end(start: CycleStartDay, year: i32, month: u32) -> Option<chrono::NaiveDate> {
    let next_start = cycle_start_of_next_month(start, year, month)?;
    next_start.pred_opt()
}

/// Compute the start date of the NEXT billing cycle. Private — only
/// `cycle_end` (this module) consumes it; no external callers (cert iter-2).
fn cycle_start_of_next_month(
    start: CycleStartDay,
    year: i32,
    month: u32,
) -> Option<chrono::NaiveDate> {
    use chrono::NaiveDate;
    let (ny, nm) = if month == 12 {
        (year + 1, 1u32)
    } else {
        (year, month + 1)
    };
    if let Some(d) = start.day_value() {
        let d = u32::from(d);
        NaiveDate::from_ymd_opt(ny, nm, d)
            .or_else(|| NaiveDate::from_ymd_opt(ny, nm, last_day_of_month(ny, nm)))
    } else {
        NaiveDate::from_ymd_opt(ny, nm, last_day_of_month(ny, nm))
    }
}

/// Given a cycle end date, compute the start date of the NEXT cycle.
#[must_use]
pub fn next_cycle_start(end: chrono::NaiveDate) -> chrono::NaiveDate {
    end.succ_opt().unwrap_or(end)
}

/// Return the last day (1-indexed) of the given month, accounting for leap years.
///
/// v1.0 audit 3 (stdlib parity): rewritten from a 12-arm match + hand-rolled
/// `is_leap_year` to chrono's calendar — `NaiveDate::from_ymd_opt(year,
/// month+1, 1).pred()` yields the last day of the target month, leap-year-
/// correct, in one call. The prior `_ => 30` defensive arm silently swallowed
/// any `month > 12`; chrono surfaces invalid input as a fallback to day 28
/// (the safest lower bound — produces a valid cycle for any sane caller).
#[must_use]
pub fn last_day_of_month(year: i32, month: u32) -> u32 {
    use chrono::Datelike;
    // Compute the first day of the NEXT month, then step back one day.
    // December wraps to January of the next year.
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map_or(28, |d| d.day())
}

/// Standard leap-year check.
#[must_use]
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn cycle_end_day7_july_2026() {
        let end = cycle_end(CycleStartDay::day(7), 2026, 7);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2026, 8, 6).unwrap()));
    }

    #[test]
    fn next_cycle_start_from_aug6() {
        let end = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();
        assert_eq!(
            next_cycle_start(end),
            NaiveDate::from_ymd_opt(2026, 8, 7).unwrap()
        );
    }

    #[test]
    fn cycle_end_year_boundary() {
        let end = cycle_end(CycleStartDay::day(15), 2026, 12);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2027, 1, 14).unwrap()));
    }

    #[test]
    fn last_day_of_month_january() {
        let end = cycle_end(CycleStartDay::LastDayOfMonth, 2026, 1);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2026, 2, 27).unwrap()));
    }

    #[test]
    fn last_day_of_month_february_leap() {
        let end = cycle_end(CycleStartDay::LastDayOfMonth, 2024, 2);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2024, 3, 30).unwrap()));
    }

    #[test]
    fn last_day_of_month_february_non_leap() {
        let end = cycle_end(CycleStartDay::LastDayOfMonth, 2023, 2);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2023, 3, 30).unwrap()));
    }

    #[test]
    fn last_day_of_month_december() {
        let end = cycle_end(CycleStartDay::LastDayOfMonth, 2026, 12);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2027, 1, 30).unwrap()));
    }

    #[test]
    fn day28_in_february_leap() {
        let end = cycle_end(CycleStartDay::day(28), 2024, 2);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2024, 3, 27).unwrap()));
    }

    #[test]
    fn day28_in_february_non_leap() {
        let end = cycle_end(CycleStartDay::day(28), 2023, 2);
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2023, 3, 27).unwrap()));
    }

    #[test]
    fn day_valid_values_unchanged() {
        for d in 1..=28u8 {
            assert_eq!(CycleStartDay::day(d).day_value(), Some(d));
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "out of range")]
    fn day_zero_panics_in_debug() {
        let _ = CycleStartDay::day(0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "out of range")]
    fn day_29_panics_in_debug() {
        let _ = CycleStartDay::day(29);
    }

    #[test]
    fn last_day_of_month_all_months_2026() {
        assert_eq!(last_day_of_month(2026, 1), 31);
        assert_eq!(last_day_of_month(2026, 2), 28);
        assert_eq!(last_day_of_month(2026, 3), 31);
        assert_eq!(last_day_of_month(2026, 4), 30);
        assert_eq!(last_day_of_month(2026, 5), 31);
        assert_eq!(last_day_of_month(2026, 6), 30);
        assert_eq!(last_day_of_month(2026, 7), 31);
        assert_eq!(last_day_of_month(2026, 8), 31);
        assert_eq!(last_day_of_month(2026, 9), 30);
        assert_eq!(last_day_of_month(2026, 10), 31);
        assert_eq!(last_day_of_month(2026, 11), 30);
        assert_eq!(last_day_of_month(2026, 12), 31);
    }

    #[test]
    fn is_leap_year_cases() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn cycle_length_invariant_27_to_31_days() {
        for year in [2023, 2024, 2025, 2026] {
            for month in 1u32..=12 {
                for d in [1u8, 7, 15, 28] {
                    let start = CycleStartDay::day(d);
                    let start_date = NaiveDate::from_ymd_opt(year, month, u32::from(d))
                        .unwrap_or_else(|| {
                            NaiveDate::from_ymd_opt(year, month, last_day_of_month(year, month))
                                .unwrap()
                        });
                    if let Some(end) = cycle_end(start, year, month) {
                        let len = (end - start_date).num_days();
                        assert!(
                            (27..=31).contains(&len),
                            "T-25: cycle len {len} for Day({d}), {year}-{month}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn next_cycle_start_dec31() {
        let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        assert_eq!(
            next_cycle_start(end),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
    }

    #[test]
    fn next_cycle_start_feb28_non_leap() {
        let end = NaiveDate::from_ymd_opt(2023, 2, 28).unwrap();
        assert_eq!(
            next_cycle_start(end),
            NaiveDate::from_ymd_opt(2023, 3, 1).unwrap()
        );
    }

    #[test]
    fn next_cycle_start_feb29_leap() {
        let end = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(
            next_cycle_start(end),
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()
        );
    }
}
