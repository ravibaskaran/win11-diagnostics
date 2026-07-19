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
