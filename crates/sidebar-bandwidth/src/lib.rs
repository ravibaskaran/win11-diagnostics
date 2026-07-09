//! `sidebar-bandwidth` — BandwidthAccountant — accumulate + flush + rollover (Stories 5.1-5.3).
//!
//! Story 5.1 delivers the in-memory [`accumulator::MonthlyAccumulator`] with
//! T-23 counter-wraparound handling. Story 5.2 adds the tokio accountant
//! task ([`accountant::BandwidthAccountant`]) + the injectable
//! [`clock::Clock`] trait, which together subscribe to the poller's reading
//! stream, filter network counters, accumulate per-LUID deltas, flush to
//! SQLite (via `sidebar-persistence`), and roll over the billing cycle.

pub mod accountant;
pub mod accumulator;
pub mod clock;
pub mod view;

/// Story 0.1 smoke marker — proves the crate is reachable via `cargo test`.
#[must_use]
pub fn crate_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::crate_present;

    /// Story 0.1 Happy Path #1. Cited: G17 (no empty stubs).
    #[test]
    fn crate_present_returns_true() {
        assert!(crate_present(), "crate_present() must return true");
    }

    /// Story 0.1 idempotency. Cited: fixture F6.
    #[test]
    fn crate_present_is_idempotent() {
        assert_eq!(crate_present(), crate_present());
    }
}
