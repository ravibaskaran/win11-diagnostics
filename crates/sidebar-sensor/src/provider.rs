//! Story 2.1 — SensorProvider trait.
//!
//! The keystone abstraction (AD-4). Every adapter implements this trait.
//! Domain logic operates on `Vec<Reading>` produced by the trait, never on
//! concrete adapter types — this is what makes strict TDD feasible for ~80%
//! of the codebase.
//!
//! `mockall::automock` generates `MockSensorProvider` for unit tests.
//!
//! Cited: architecture.md §5.2, Story 2.1.

use sidebar_domain::reading::Reading;

use crate::descriptor::SensorDescriptor;

/// One telemetry source. Implementations live in sidebar-adapter-*.
///
/// Implementations MUST be `Send + Sync` so the poller can hold them behind
/// `Arc<dyn SensorProvider>`. The `read_all` method is synchronous; adapters
/// that perform blocking syscalls should use `spawn_blocking` in the poller
/// (which wraps calls in `catch_unwind` per G15).
///
/// ## Counter vs. gauge semantics (architecture §5.2 v2 note)
///
/// Two flavors of `Reading` flow through this trait:
/// - **Gauge readings** (most sensors): `value` is the current instantaneous
///   measurement (CPU util %, temperature °C). Displayed directly.
/// - **Cumulative-counter readings** (network byte/packet/error counts):
///   `value` is the raw OS counter (e.g. `InOctets`). NOT displayed directly.
///   The `BandwidthAccountant` consumes these to produce deltas + monthly totals.
#[cfg_attr(test, mockall::automock)]
pub trait SensorProvider: Send + Sync {
    /// Return this provider's descriptor (name, cost class, metrics, tier).
    fn descriptor(&self) -> &SensorDescriptor;

    /// Poll this source once. Called on every tick.
    ///
    /// Implementations should be cheap (NFR-1, T-1) and non-blocking; if the
    /// underlying call is sync-syscall-heavy, the poller wraps it in
    /// `spawn_blocking`.
    fn read_all(&self) -> Vec<Reading>;
}

// Compile-time proof that the trait object is Send + Sync.
static_assertions::assert_impl_all!(dyn SensorProvider: Send, Sync);

#[cfg(test)]
mod tests {
    use super::*;
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};
    use std::sync::Arc;

    fn test_descriptor() -> SensorDescriptor {
        SensorDescriptor::new(
            "test",
            crate::descriptor::CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            crate::descriptor::ProviderTier::Basic,
        )
    }

    fn test_reading() -> Reading {
        Reading::gauge(
            SensorId::new("cpu", "0"),
            MetricKind::CpuUtilization,
            42.0,
            Unit::Percent,
        )
    }

    #[test]
    fn mock_returns_canned_readings() {
        let mut mock = MockSensorProvider::new();
        let canned = [test_reading()];
        let expected_value = canned[0].value;
        mock.expect_read_all().returning(move || {
            vec![Reading::gauge(
                SensorId::new("cpu", "0"),
                MetricKind::CpuUtilization,
                expected_value,
                Unit::Percent,
            )]
        });
        mock.expect_descriptor().return_const(test_descriptor());

        let readings = mock.read_all();
        assert_eq!(readings.len(), 1);
        assert!((readings[0].value - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mock_empty_readings_handled() {
        let mut mock = MockSensorProvider::new();
        mock.expect_read_all().returning(Vec::new);
        mock.expect_descriptor().return_const(test_descriptor());
        assert!(mock.read_all().is_empty());
    }

    #[test]
    fn mock_call_count_is_one() {
        let mut mock = MockSensorProvider::new();
        mock.expect_read_all().times(1).returning(Vec::new);
        mock.expect_descriptor().return_const(test_descriptor());
        let _ = mock.read_all();
        // If read_all were called twice, mockall would panic on drop.
    }

    #[test]
    fn trait_object_crosses_threads() {
        let mut mock = MockSensorProvider::new();
        mock.expect_read_all().returning(Vec::new);
        mock.expect_descriptor().return_const(test_descriptor());
        let provider: Arc<dyn SensorProvider> = Arc::new(mock);

        let handle = std::thread::spawn(move || {
            // If Send + Sync weren't satisfied, this wouldn't compile.
            provider.read_all()
        });
        let readings = handle.join().unwrap();
        assert!(readings.is_empty());
    }

    #[test]
    fn multiple_mocks_independent() {
        let mut mock1 = MockSensorProvider::new();
        mock1.expect_read_all().returning(|| vec![test_reading()]);
        mock1.expect_descriptor().return_const(test_descriptor());

        let mut mock2 = MockSensorProvider::new();
        mock2.expect_read_all().returning(Vec::new);
        mock2.expect_descriptor().return_const(test_descriptor());

        assert_eq!(mock1.read_all().len(), 1);
        assert_eq!(mock2.read_all().len(), 0);
    }
}
