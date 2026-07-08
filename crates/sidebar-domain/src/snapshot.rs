//! Story 1.2 — Snapshot type + runtime Tier enum.
//!
//! `Snapshot` is the timestamped bundle of readings that the poller
//! publishes via broadcast and the GUI consumes. `Tier` is the runtime
//! mode (Basic/Full) distinct from `SensorDescriptor::requires_tier` in
//! sidebar-sensor (which is a *requirement* on a provider, not the active
//! runtime state).
//!
//! Cited: Story 1.2, architecture.md §4 + §6.

use std::time::Instant;

use crate::reading::Reading;

/// Runtime mode the sidebar is operating in.
///
/// Set by the two-tier auto-detect probe (Story 7.3 / architecture.md
/// AD-7) on every launch. Changes mid-session (e.g. LHM crash) are
/// broadcast on the Event channel (Story 7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// No admin privileges; telemetry from sysinfo/nvml/battery/PDH/net
    /// only. No CPU temp, fan speeds, voltages, non-NVIDIA GPU sensors.
    Basic,
    /// Bundled LHM subprocess running; full telemetry coverage.
    Full,
}

/// A timestamped bundle of sensor readings + the active tier.
///
/// Published by the poller (Story 7.2) via `tokio::sync::broadcast` and
/// consumed by the GUI's `AppState`. The `tier` snapshot lets the GUI
/// render the status pill without a separate channel lookup.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// All readings from this poll tick.
    pub readings: Vec<Reading>,
    /// When the snapshot was taken.
    pub timestamp: Instant,
    /// The runtime tier active when the readings were taken.
    pub tier: Tier,
}

impl Snapshot {
    /// Construct a new `Snapshot` with `timestamp = Instant::now()`.
    #[must_use]
    pub fn new(readings: Vec<Reading>, tier: Tier) -> Self {
        Self {
            readings,
            timestamp: Instant::now(),
            tier,
        }
    }

    /// Construct an empty snapshot (no readings). Useful for the initial
    /// GUI state before the first poll tick.
    #[must_use]
    pub fn empty(tier: Tier) -> Self {
        Self {
            readings: Vec::new(),
            timestamp: Instant::now(),
            tier,
        }
    }

    /// Number of readings in the snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.readings.len()
    }

    /// Whether the snapshot has zero readings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.readings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::{MetricKind, SensorId, Unit};

    #[test]
    fn snapshot_new_stores_readings_and_tier() {
        let r = Reading::new(
            SensorId::new("cpu", "package"),
            MetricKind::CpuTemperature,
            62.0,
            Unit::Celsius,
        );
        let snap = Snapshot::new(vec![r], Tier::Full);
        assert_eq!(snap.len(), 1);
        assert!(!snap.is_empty());
        assert_eq!(snap.tier, Tier::Full);
    }

    #[test]
    fn snapshot_empty_is_empty() {
        let snap = Snapshot::empty(Tier::Basic);
        assert!(snap.is_empty());
        assert_eq!(snap.len(), 0);
        assert_eq!(snap.tier, Tier::Basic);
    }

    #[test]
    fn tier_equality() {
        assert_eq!(Tier::Basic, Tier::Basic);
        assert_ne!(Tier::Basic, Tier::Full);
    }
}
