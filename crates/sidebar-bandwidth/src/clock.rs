//! Injectable wall-clock for the BandwidthAccountant (Story 5.2, fixture F3).
//!
//! The accountant needs "what is the current date/time?" to decide when to
//! roll the billing cycle (`clock.now().date_naive() >= cycle_end`, T-27).
//! Reading `chrono::Local::now()` directly inside the task makes the
//! rollover path untestable without real time. F3 mandates an injectable
//! clock; this module is it.
//!
//! # Contract (HITL — G11)
//!
//! ```ignore
//! pub trait Clock: Send + Sync {
//!     fn now(&self) -> chrono::NaiveDateTime;
//! }
//! ```
//!
//! Returns `NaiveDateTime` (timezone-free per T-27). `Send + Sync` so it can
//! live behind an `Arc<dyn Clock>` shared with other tasks if the wiring ever
//! needs it. Production code uses [`SystemClock`]; tests use [`FakeClock`]
//! which is advanced explicitly to drive the rollover boundary.
//!
//! Cited: Story 5.2 spec (Clock trait — HITL item), nfr-thresholds.md T-27
//! (timezone contract), tdd-fixtures.md F3 (FakeClock), guardrails.md G11.

use chrono::NaiveDateTime;

#[cfg(test)]
use std::sync::Mutex;

/// Injectable wall-clock. Production: [`SystemClock`]. Tests: [`FakeClock`].
///
/// `now()` is the single method. The accountant calls it on every tick
/// (to check rollover) and at flush time (to stamp `updated_at` /
/// `archived_at`). Both values flow into SQLite as ISO 8601 strings via
/// `NaiveDateTime::to_string()`, so the trait returns the structured
/// `NaiveDateTime` rather than a pre-formatted string.
pub trait Clock: Send + Sync {
    /// Return the current wall-clock time, timezone-free (T-27).
    fn now(&self) -> NaiveDateTime;
}

/// Production clock — reads `chrono::Local::now().naive_local()`.
///
/// `SystemClock` is a zero-sized marker; it performs no state and is cheap
/// to construct. `Send + Sync` trivially (no interior state).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl SystemClock {
    /// Construct a `SystemClock`. (`Default` also works.)
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn now(&self) -> NaiveDateTime {
        // T-27: "Today" is `Local::now().date_naive()`; we return the full
        // naive datetime so callers that need the time-of-day (flush
        // timestamps) get it for free. `.naive_local()` drops the offset
        // without conversion (we want wall-clock-local, not UTC).
        chrono::Local::now().naive_local()
    }
}

/// Test double for [`Clock`] (fixture F3). Holds a mutable timestamp behind
/// an `Arc<Mutex<…>>` so clones share the same underlying time — tests keep
/// one clone to drive [`FakeClock::set`] / [`FakeClock::advance`] while the
/// accountant reads the time through the trait on its own clone.
///
/// `Clone` (cheap — bumps the Arc refcount) so the test harness can hand a
/// clone to the accountant and retain one for time-control. `Send + Sync`
/// via the `Mutex`.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct FakeClock {
    now: std::sync::Arc<Mutex<NaiveDateTime>>,
}

#[cfg(test)]
impl FakeClock {
    /// Construct a FakeClock pinned at `t0`. The accountant sees this
    /// initial value on its first tick until the test advances it.
    #[must_use]
    pub fn new(t0: NaiveDateTime) -> Self {
        Self {
            now: std::sync::Arc::new(Mutex::new(t0)),
        }
    }

    /// Overwrite the current time to `t`.
    ///
    /// Tests call this to cross the billing-cycle boundary: set the clock
    /// to `cycle_end + 1 day` and the accountant's next tick fires rollover.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned (another thread panicked
    /// while holding the lock). Test-only code never triggers this in
    /// practice.
    pub fn set(&self, t: NaiveDateTime) {
        *self.now.lock().expect("FakeClock mutex poisoned") = t;
    }

    /// Advance the clock by a `chrono::Duration`. Convenience for
    /// day/hour increments in rollover tests.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn advance(&self, delta: chrono::Duration) {
        let mut guard = self.now.lock().expect("FakeClock mutex poisoned");
        *guard = guard.checked_add_signed(delta).unwrap_or(*guard); // overflow → leave unchanged (defensive)
    }

    /// Read the current fake time (test introspection; the accountant uses
    /// the trait method).
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn peek(&self) -> NaiveDateTime {
        *self.now.lock().expect("FakeClock mutex poisoned")
    }
}

#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> NaiveDateTime {
        *self.now.lock().expect("FakeClock mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    //! Clock-trait smoke tests. Cited: Story 5.2 (Clock contract — HITL),
    //! T-27, F3.

    use super::*;
    use chrono::{Duration, NaiveDate};

    /// SystemClock returns a time close to now (within a minute). We can't
    /// assert exact equality against `Local::now()` (race), so we bound it.
    #[test]
    fn system_clock_returns_recent_time() {
        let clock = SystemClock::new();
        let t = clock.now();
        let wall = chrono::Local::now().naive_local();
        let drift = wall.signed_duration_since(t);
        assert!(
            drift.abs() < Duration::minutes(1),
            "SystemClock drift > 1min: {drift:?}"
        );
    }

    /// FakeClock returns whatever it was set to (F3).
    #[test]
    fn fake_clock_returns_set_value() {
        let t0 = NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let clock = FakeClock::new(t0);
        assert_eq!(clock.now(), t0);
        let t1 = t0 + Duration::days(40); // cross cycle boundary
        clock.set(t1);
        assert_eq!(clock.now(), t1);
    }

    /// FakeClock::advance adds the delta (convenience helper).
    #[test]
    fn fake_clock_advance_adds_delta() {
        let t0 = NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let clock = FakeClock::new(t0);
        clock.advance(Duration::days(5));
        assert_eq!(clock.peek(), t0 + Duration::days(5));
    }
}
