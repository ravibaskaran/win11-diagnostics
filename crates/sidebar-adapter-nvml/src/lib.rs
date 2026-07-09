//! `sidebar-adapter-nvml` — NVIDIA GPU telemetry adapter (Story 3.2).
//!
//! Provides GPU utilization % and GPU temperature via the `nvml-wrapper`
//! crate. v1 emits these two metrics per GPU; memory util, power, fan, and
//! frequency are later stories (see Story 3.2b).
//!
//! ## Architecture
//!
//! Mirrors the sysinfo-adapter pattern (Story 3.1): a `NvmlBackend` trait
//! abstracts the concrete `nvml_wrapper::Nvml` so the adapter is unit-testable
//! without NVIDIA hardware. Production code wires the real `Nvml` via
//! [`backend::RealNvmlBackend`]; tests inject a `mockall`-generated mock.
//!
//! The adapter holds a `Mutex<B>` (per Story 3.2 Technical Context) so
//! `read_all` can take `&self` and still refresh. `SensorProvider::read_all`
//! is `&self` (Story 2.1), so interior mutability is required.
//!
//! ## NVML-unavailable safety
//!
//! On machines without an NVIDIA driver (e.g. the AMD Ryzen AI dev laptop),
//! `Nvml::init()` fails. The adapter handles this gracefully:
//! [`backend::RealNvmlBackend::new`] stores `Option<Nvml>` and logs a single
//! `debug!` on failure (no panic). Every `read_all` thereafter returns an
//! empty `Vec<Reading>`. This is the Story 3.2 Unit Test #2 contract
//! ("NVML init fails → empty vec, debug!, no panic").
//!
//! ## T-13 timeout (NOT enforced here)
//!
//! NFR-thresholds T-13 says each NVML call should be wrapped in
//! `tokio::time::timeout(100ms, spawn_blocking(...))`. Per architecture AD-6,
//! the **poller** owns the async runtime + timeout wrapping; the adapter's
//! `read_all` is synchronous. This adapter therefore performs no timeout
//! enforcement. See [`backend`] module docs for the full rationale.
//!
//! ## Cited
//!
//! - Story 3.2 TDD contract (Happy Path #1-#2, Boundary #1-#4)
//! - architecture.md §5.1 (Reading/MetricKind/Unit spec), §5.2 (gauges),
//!   §7.2, AD-4 (SensorProvider), AD-6 (async runtime ownership)
//! - nfr-thresholds.md T-13 (upstream timeout), T-20 (finite values only)

use std::sync::Mutex;

use sidebar_domain::reading::{MetricKind, Reading};
// `SensorId` + `Unit` are used inside `readings_from_snapshot` once GREEN
// lands; the RED stub returns `Vec::new()` so they're unused for now. Import
// them under `#[allow(unused_imports)]` to keep the GREEN commit minimal.
#[allow(unused_imports)]
use sidebar_domain::reading::{SensorId, Unit};
use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
use sidebar_sensor::provider::SensorProvider;

pub mod backend;

use crate::backend::NvmlBackend;

/// Descriptor declared once; cloned cheaply on each `descriptor()` call.
/// `&'static [MetricKind]` keeps the descriptor `const`-constructible.
const NVML_METRICS: &[MetricKind] = &[MetricKind::GpuUtilization, MetricKind::GpuTemperature];

/// Descriptor for the NVML adapter — `Tier::Basic`, `CostClass::Lightweight`.
///
/// NVML is `Lightweight`: it is a userspace library doing a few syscalls per
/// query (no driver compilation, no LHM subprocess). Story 3.2 Technical
/// Context pins this classification; Story 10.1 benches it in CI.
const DESCRIPTOR: SensorDescriptor = SensorDescriptor::new(
    "nvml",
    CostClass::Lightweight,
    NVML_METRICS,
    ProviderTier::Basic,
);

/// NVML-backed adapter. Holds a `Mutex` around the backend because the
/// underlying NVML refresh requires `&mut` and `SensorProvider::read_all` is
/// `&self`.
///
/// Generic over `B: NvmlBackend` so tests can substitute a mock. The
/// production convenience alias [`NvmlAdapter`] fixes `B =
/// RealNvmlBackend`.
pub struct NvmlAdapterGeneric<B: NvmlBackend> {
    backend: Mutex<B>,
}

/// Production adapter wired to a real `nvml_wrapper::Nvml`.
///
/// Equivalent to `NvmlAdapterGeneric<RealNvmlBackend>` but with a `new()`
/// constructor that initializes the real NVML backend (which is NVML-unavailable
/// safe — see [`backend::RealNvmlBackend::new`]).
pub type NvmlAdapter = NvmlAdapterGeneric<backend::RealNvmlBackend>;

impl<B: NvmlBackend> NvmlAdapterGeneric<B> {
    /// Construct an adapter wrapping the given backend. The backend is wrapped
    /// in a `Mutex` so `read_all` (which is `&self` per the trait) can still
    /// call `&mut self` refresh methods.
    #[must_use]
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }
}

impl NvmlAdapter {
    /// Construct the production adapter backed by a real `nvml_wrapper::Nvml`.
    ///
    /// On a machine without NVIDIA hardware/driver, this constructs an adapter
    /// whose `read_all` always returns empty — no panic. See
    /// [`backend::RealNvmlBackend::new`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(backend::RealNvmlBackend::new())
    }
}

impl Default for NvmlAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: NvmlBackend + Send> SensorProvider for NvmlAdapterGeneric<B> {
    fn descriptor(&self) -> &SensorDescriptor {
        &DESCRIPTOR
    }

    fn read_all(&self) -> Vec<Reading> {
        // Lock once per tick. The lock is held only for the duration of the
        // NVML queries (microseconds on any healthy GPU). The poller is
        // single-threaded per adapter in v1; the Mutex exists only to satisfy
        // the `&self` signature.
        //
        // Mutex poison recovery per G15: if a prior `read_all` panicked
        // mid-lock (it can't here — `refresh_and_snapshot` is infallible for
        // the Real backend — but a mock could), we recover the inner guard
        // rather than propagating the poison.
        let mut guard = self
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = guard.refresh_and_snapshot();
        readings_from_snapshot(&snapshot)
    }
}

/// Translate an [`backend::NvmlSnapshot`] into the canonical `Vec<Reading>`.
///
/// Pulled out of `read_all` so unit tests can exercise the translation without
/// standing up a backend — this is where the TDD contract's "42% util, 65°C →
/// 2 readings" assertions live.
///
/// # Finite-value policy (T-20)
///
/// Any reading whose `value` is `NaN` or `±Inf` is OMITTED. The
/// [`backend::RealNvmlBackend`] uses `f64::NAN` as a per-metric failure
/// sentinel (e.g. when `temperature()` errors but `utilization_rates()`
/// succeeds) — this filter drops the failed metric and keeps the succeeded
/// one. This is Boundary #4 ("NVML error mid-poll → partial readings,
/// logged").
fn readings_from_snapshot(s: &backend::NvmlSnapshot) -> Vec<Reading> {
    // STUB — Story 3.2 RED phase. Returns empty so the mock-based positive
    // tests fail (RED). GREEN phase implements per-GPU GpuUtilization +
    // GpuTemperature with the T-20 finite filter.
    let _ = s;
    Vec::new()
}

// Re-export key types for downstream consumers (provider registry, poller).
pub use backend::{GpuSnapshot, NvmlSnapshot, RealNvmlBackend};

#[cfg(test)]
mod tests {
    //! Story 3.2 TDD contract tests.
    //!
    //! These tests exercise `readings_from_snapshot` + the adapter via a mock
    //! backend. The real `nvml_wrapper::Nvml` is exercised by an
    //! `#[ignore]`-gated integration test below (it produces empty readings
    //! on non-NVIDIA machines, which is the correct behavior — that test
    //! exists to verify the real-backend wiring compiles + runs without
    //! panic, and is intended for `cargo test --ignored` on CI runners with
    //! NVIDIA hardware).
    //!
    //! Cited:
    //!   - Story 3.2 TDD contract (Happy Path #1-#2, Boundary #1-#4)
    //!   - architecture.md §5.1 (Reading/MetricKind/Unit spec), §5.2 (gauges)
    //!   - nfr-thresholds.md T-13 (upstream timeout), T-20 (finite only)

    use super::*;
    use mockall::mock;
    use sidebar_domain::reading::{MetricKind, Unit};
    use sidebar_sensor::descriptor::{CostClass, ProviderTier};
    use sidebar_sensor::provider::SensorProvider;
    use std::sync::Arc;

    // Auto-mock the `NvmlBackend` trait so tests can inject canned snapshots
    // without standing up a real NVML instance (and without NVIDIA hardware).
    mock! {
        pub FakeBackend {}
        impl NvmlBackend for FakeBackend {
            fn refresh_and_snapshot(&mut self) -> NvmlSnapshot;
        }
    }

    // ----- Happy Path #1: Mock NVML 42% util, 65°C → 2 readings -----

    /// Story 3.2 Happy Path #1. Cited: Story 3.2 TDD contract.
    /// Fixture: in-process mock. Threshold T-1 (Lightweight).
    #[test]
    fn one_gpu_42_util_65c_yields_two_readings() {
        let snap = NvmlSnapshot {
            gpus: vec![GpuSnapshot {
                utilization_pct: 42.0,
                temperature_c: 65.0,
            }],
        };
        let readings = readings_from_snapshot(&snap);
        assert_eq!(readings.len(), 2, "1 GPU → util + temp = 2 readings");
        let util = readings
            .iter()
            .find(|r| r.kind == MetricKind::GpuUtilization)
            .expect("GpuUtilization present");
        assert!((util.value - 42.0).abs() < f64::EPSILON);
        assert_eq!(util.unit, Unit::Percent);
        let temp = readings
            .iter()
            .find(|r| r.kind == MetricKind::GpuTemperature)
            .expect("GpuTemperature present");
        assert!((temp.value - 65.0).abs() < f64::EPSILON);
        assert_eq!(temp.unit, Unit::Celsius);
    }

    // ----- Happy Path #1b: per-GPU SensorId uses "gpu" category, "0" instance -----

    #[test]
    fn gpu_readings_use_gpu_category_with_index_instance() {
        let snap = NvmlSnapshot {
            gpus: vec![GpuSnapshot {
                utilization_pct: 50.0,
                temperature_c: 70.0,
            }],
        };
        let readings = readings_from_snapshot(&snap);
        let util = readings
            .iter()
            .find(|r| r.kind == MetricKind::GpuUtilization)
            .unwrap();
        assert_eq!(util.sensor.category, "gpu");
        assert_eq!(util.sensor.instance, "0");
    }

    // ----- Happy Path #2: NVML init fails → empty vec, no panic -----

    /// Story 3.2 Happy Path #2. Cited: Story 3.2 TDD contract. The real
    /// backend handles init failure in `RealNvmlBackend::new` (stores `None`,
    /// logs `debug!`); this test exercises the adapter via a mock that returns
    /// an empty snapshot — the contract behavior for NVML-unavailable.
    #[test]
    fn nvml_init_fails_yields_empty_vec_no_panic() {
        let mut mock = MockFakeBackend::new();
        mock.expect_refresh_and_snapshot()
            .returning(NvmlSnapshot::default);
        let adapter = NvmlAdapterGeneric::with_backend(mock);
        let readings = adapter.read_all();
        assert!(readings.is_empty(), "NVML unavailable → empty readings");
    }

    // ----- Boundary #1: 0 GPUs → empty -----

    /// Story 3.2 Boundary #1. Cited: Story 3.2 TDD contract.
    #[test]
    fn zero_gpus_emits_no_readings() {
        let snap = NvmlSnapshot::default();
        let readings = readings_from_snapshot(&snap);
        assert!(readings.is_empty(), "0 GPUs → 0 readings");
    }

    // ----- Boundary #2: 2 GPUs → instance "0" and "1" -----

    /// Story 3.2 Boundary #2. Cited: Story 3.2 TDD contract.
    #[test]
    fn two_gpus_yields_instance_zero_and_one() {
        let snap = NvmlSnapshot {
            gpus: vec![
                GpuSnapshot {
                    utilization_pct: 10.0,
                    temperature_c: 40.0,
                },
                GpuSnapshot {
                    utilization_pct: 90.0,
                    temperature_c: 80.0,
                },
            ],
        };
        let readings = readings_from_snapshot(&snap);
        // 2 GPUs × 2 metrics = 4 readings.
        assert_eq!(readings.len(), 4, "2 GPUs → 4 readings");
        let instances: std::collections::BTreeSet<String> =
            readings.iter().map(|r| r.sensor.instance.clone()).collect();
        assert_eq!(
            instances.iter().collect::<Vec<_>>(),
            vec![&"0".to_string(), &"1".to_string()],
            "instances must be exactly \"0\" and \"1\""
        );
    }

    // ----- Boundary #3: T-13 timeout — documented as upstream (AD-6) -----

    /// Story 3.2 Boundary #3. Cited: Story 3.2 TDD contract, nfr-thresholds
    /// T-13, architecture AD-6.
    ///
    /// Per AD-6, T-13 timeout wrapping is the POLLER's job — the adapter's
    /// `read_all` is synchronous. This test documents that contract: the
    /// adapter has no `tokio::time::timeout` call and no async surface; a
    /// hung NVML call would be torn down by the poller's
    /// `tokio::time::timeout(100ms, spawn_blocking(...))` wrapper. We can't
    /// meaningfully unit-test a hung NVML call without NVIDIA hardware, so
    /// this test simply asserts the adapter type is `Send + Sync` (the
    /// prerequisite for the poller's `spawn_blocking` wiring) and that
    /// `read_all` is sync (no `.await`).
    #[test]
    fn t13_timeout_is_upstream_per_ad6() {
        // The adapter must be Send + Sync so the poller can hold it behind
        // `Arc<dyn SensorProvider>` and dispatch via `spawn_blocking`.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NvmlAdapter>();
        // `read_all` returns Vec<Reading>, not a Future — sync by signature.
        let mut mock = MockFakeBackend::new();
        mock.expect_refresh_and_snapshot()
            .returning(|| NvmlSnapshot {
                gpus: vec![GpuSnapshot {
                    utilization_pct: 1.0,
                    temperature_c: 2.0,
                }],
            });
        let adapter = NvmlAdapterGeneric::with_backend(mock);
        let _readings: Vec<Reading> = adapter.read_all();
    }

    // ----- Boundary #4: NVML error mid-poll → partial readings, logged -----

    /// Story 3.2 Boundary #4. Cited: Story 3.2 TDD contract, T-20.
    ///
    /// The Real backend uses `f64::NAN` as a per-metric failure sentinel —
    /// when `temperature()` errors but `utilization_rates()` succeeds (or
    /// vice versa), the failed metric is NaN and `readings_from_snapshot`
    /// MUST drop it (T-20 finite filter) while keeping the succeeded one.
    /// This test verifies that partial-success behavior at the translation
    /// layer.
    #[test]
    fn nan_metric_is_dropped_per_t20_keeps_sibling() {
        let snap = NvmlSnapshot {
            gpus: vec![GpuSnapshot {
                utilization_pct: 42.0,   // succeeded
                temperature_c: f64::NAN, // failed → dropped
            }],
        };
        let readings = readings_from_snapshot(&snap);
        // Only the util reading survives — partial readings per Boundary #4.
        assert_eq!(readings.len(), 1, "partial failure → 1 reading kept");
        assert_eq!(readings[0].kind, MetricKind::GpuUtilization);
        assert!((readings[0].value - 42.0).abs() < f64::EPSILON);
        // No reading value is NaN.
        assert!(readings.iter().all(|r| r.value.is_finite()));
    }

    // ----- Descriptor correctness -----

    /// The descriptor is Tier::Basic + Lightweight (Story 3.2 Technical
    /// Context). Cited: Story 2.2/2.3 classifier contract.
    #[test]
    fn descriptor_is_basic_tier_lightweight() {
        let adapter = NvmlAdapter::new();
        let d = adapter.descriptor();
        assert_eq!(d.name, "nvml");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.requires_tier, ProviderTier::Basic);
        assert!(d.metrics.contains(&MetricKind::GpuUtilization));
        assert!(d.metrics.contains(&MetricKind::GpuTemperature));
    }

    // ----- Mutex poisoning recovery (G15) -----

    /// The adapter MUST NOT propagate Mutex *poison* errors to the poller.
    /// Cited: guardrails.md G15. Mirror of the sysinfo-adapter test.
    #[test]
    fn read_all_recovers_from_mutex_poison() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;
        let poisoned = StdArc::new(AtomicBool::new(false));
        let backend = OncePanicBackend {
            poisoned: poisoned.clone(),
        };
        let adapter = StdArc::new(NvmlAdapterGeneric::with_backend(backend));
        // First call: panics inside the lock, poisoning the Mutex. The panic
        // propagates out of `read_all` (the poller catches it via
        // `catch_unwind` per G15).
        let a1 = adapter.clone();
        let h = std::thread::spawn(move || a1.read_all());
        let _ = h.join(); // thread panicked — expected.
        assert!(poisoned.load(Ordering::SeqCst), "first call must have run");
        // Second call: Mutex is poisoned, but `read_all` recovers via
        // `unwrap_or_else(|e| e.into_inner())`.
        let v = adapter.read_all();
        assert!(
            v.iter().all(|r| r.value.is_finite()),
            "poison recovery yields finite readings"
        );
    }

    /// Backend that panics on the first `refresh_and_snapshot` call, then
    /// returns an empty snapshot on subsequent calls. Used to poison the
    /// adapter Mutex and verify recovery.
    struct OncePanicBackend {
        poisoned: Arc<std::sync::atomic::AtomicBool>,
    }
    impl NvmlBackend for OncePanicBackend {
        fn refresh_and_snapshot(&mut self) -> NvmlSnapshot {
            if !self
                .poisoned
                .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("poison on first call");
            }
            NvmlSnapshot::default()
        }
    }

    // ----- Real-backend smoke test (#[ignore]: needs NVIDIA HW for real data) -----
    //
    // This test verifies the real-backend wiring COMPILES and runs without
    // panicking. On machines WITHOUT NVIDIA hardware (like LAPTOP-PLN56DNU),
    // `Nvml::init()` fails and `RealNvmlBackend` returns empty — which we
    // assert as the correct NVML-unavailable behavior. On CI runners WITH
    // NVIDIA hardware, this test would produce non-empty readings from real
    // GPUs; either outcome is acceptable. The test is `#[ignore]`'d so the
    // default `cargo test` run on dev machines is hermetic.
    //
    // Run via: `cargo test -p sidebar-adapter-nvml -- --ignored`.

    /// Real-backend integration smoke. Cited: Story 3.2 Local-test caveat.
    #[test]
    #[ignore = "requires NVIDIA hardware for non-empty data; verifies wiring compiles + NVML-unavailable safety on AMD"]
    fn real_backend_smoke_no_panic() {
        let adapter = NvmlAdapter::new();
        let readings = adapter.read_all();
        // On a non-NVIDIA machine this is empty (correct). On an NVIDIA
        // machine this is non-empty (also correct). Either way: no panic,
        // all values finite per T-20.
        assert!(
            readings.iter().all(|r| r.value.is_finite()),
            "all real-backend readings must be finite (T-20)"
        );
    }
}
