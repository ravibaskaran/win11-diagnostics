//! `sidebar-domain` — Pure domain types and logic.
//!
//! The domain layer holds the canonical `Reading`, `SensorId`, `MetricKind`,
//! `Unit`, `Snapshot` types and pure functions (smoothing, formatting, billing,
//! aggregation). It has ZERO OS dependencies and ZERO I/O — that's the
//! contract that makes strict TDD feasible for ~80% of the codebase
//! (architecture.md AD-4).
//!
//! Story 0.6 also places the shared `Error` enum here (rather than in a
//! separate crate) to preserve the G17 12-package workspace cap.

pub mod adapter_metadata;
pub mod aggregate;
pub mod alert;
pub mod billing;
pub mod config;
pub mod error;
pub mod event;
pub mod format;
pub mod graph;
pub mod reading;
pub mod reposition;
pub mod smooth;
pub mod snapshot;

/// Story 0.1 smoke marker — proves the crate is reachable via `cargo test`.
/// Returns `true` unconditionally; real functionality lands in subsequent stories.
#[must_use]
pub fn crate_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    //! Unit tests for `sidebar-domain`. Story 0.1 ships only the smoke marker.

    use super::crate_present;

    /// Story 0.1 Happy Path #1 — `crate_present()` returns true.
    /// Cited: Story 0.1 TDD contract, G17 (no empty stubs).
    #[test]
    fn crate_present_returns_true() {
        assert!(crate_present(), "crate_present() must return true");
    }

    /// Story 0.1 idempotency check — calling twice yields the same result.
    /// Cited: fixture F6 (idempotency harness pattern).
    #[test]
    fn crate_present_is_idempotent() {
        let first = crate_present();
        let second = crate_present();
        assert_eq!(first, second, "crate_present() must be deterministic");
    }
}
