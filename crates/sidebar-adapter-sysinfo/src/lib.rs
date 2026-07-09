//! `sidebar-adapter-sysinfo` — sysinfo-backed telemetry adapter (Story 3.1).
//!
//! Provides CPU utilization + frequency, RAM used/total, disk used/total,
//! per-process CPU%/memory, and system uptime via the `sysinfo` crate.
//!
//! ## Architecture
//!
//! The real `sysinfo::System` is a concrete type requiring `&mut self` to
//! refresh. To make the adapter unit-testable per the TDD contract (Story 3.1
//! calls for "mock sysinfo 8 cores → 8 + 1 aggregate" tests), we abstract
//! behind a `SysinfoBackend` trait. Production code wires the real
//! `sysinfo::System` via `RealSysinfoBackend`; tests inject a mock.
//!
//! The adapter holds a `Mutex<System>` (per Story 3.1 Technical Context) so
//! `read_all` can take `&self` and still refresh. `SensorProvider::read_all`
//! is `&self` (Story 2.1), so interior mutability is required.
//!
//! ## Cited
//!
//! - Story 3.1 TDD contract (Happy Path #1-#2, Boundary #1-#5)
//! - architecture.md §5.2 (counter vs gauge — these are all gauges), §7.2
//! - nfr-thresholds.md T-1 (Lightweight), T-20 (finite values only)
//!
//! NOTE: This is the RED-phase stub. `readings_from_snapshot` is unimplemented
//! (returns an empty `Vec`) so all TDD contract tests fail. The GREEN commit
//! fills in the translation. Cited: guardrails.md G1 (RED before GREEN).

use std::sync::Mutex;

use sidebar_domain::reading::{MetricKind, Reading, SensorId, Unit};
use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
use sidebar_sensor::provider::SensorProvider;

pub mod backend;

use crate::backend::SysinfoBackend;

/// Descriptor declared once; cloned cheaply on each `descriptor()` call.
/// `&'static [MetricKind]` keeps the descriptor `const`-constructible.
const SYSINFO_METRICS: &[MetricKind] = &[
    MetricKind::CpuUtilization,
    MetricKind::CpuFrequency,
    MetricKind::MemoryUsed,
    MetricKind::MemoryTotal,
    MetricKind::DiskUsed,
    MetricKind::DiskTotal,
    MetricKind::ProcessCpuPercent,
    MetricKind::ProcessMemoryBytes,
    MetricKind::UptimeSeconds,
];

/// Descriptor for the sysinfo adapter — `Tier::Basic`, `CostClass::Lightweight`.
const DESCRIPTOR: SensorDescriptor = SensorDescriptor::new(
    "sysinfo",
    CostClass::Lightweight,
    SYSINFO_METRICS,
    ProviderTier::Basic,
);

/// sysinfo-backed adapter. Holds a `Mutex` around the backend because
/// `refresh_*` requires `&mut` and `SensorProvider::read_all` is `&self`.
///
/// Generic over `B: SysinfoBackend` so tests can substitute a mock. The
/// production convenience alias [`SysinfoAdapter`] fixes `B =
/// RealSysinfoBackend`.
pub struct SysinfoAdapterGeneric<B: SysinfoBackend> {
    backend: Mutex<B>,
}

/// Production adapter wired to the real `sysinfo::System`.
///
/// Equivalent to `SysinfoAdapterGeneric<RealSysinfoBackend>` but with a
/// `new()` constructor that initializes the real sysinfo backend.
pub type SysinfoAdapter = SysinfoAdapterGeneric<backend::RealSysinfoBackend>;

impl<B: SysinfoBackend> SysinfoAdapterGeneric<B> {
    /// Construct an adapter wrapping the given backend. The backend is
    /// wrapped in a `Mutex` so `read_all` (which is `&self` per the trait)
    /// can still call `&mut self` refresh methods.
    #[must_use]
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }
}

impl SysinfoAdapter {
    /// Construct the production adapter backed by a real `sysinfo::System`.
    ///
    /// The first `read_all` call refreshes CPU/memory/processes/disk; there
    /// is no eager refresh here (keeps cold-start cheap, NFR-3).
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(backend::RealSysinfoBackend::new())
    }
}

impl Default for SysinfoAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: SysinfoBackend + Send> SensorProvider for SysinfoAdapterGeneric<B> {
    fn descriptor(&self) -> &SensorDescriptor {
        &DESCRIPTOR
    }

    fn read_all(&self) -> Vec<Reading> {
        // Lock once per tick. The lock is held for the duration of refresh +
        // snapshot extraction (microseconds on any modern machine). The
        // poller is single-threaded per adapter in v1, so contention is not
        // a concern; the Mutex exists only to satisfy the `&self` signature.
        let mut guard = self
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = guard.refresh_and_snapshot();
        readings_from_snapshot(&snapshot)
    }
}

/// Translate a `backend::SysinfoSnapshot` into the canonical `Vec<Reading>`.
///
/// # RED-phase stub
///
/// Returns an empty `Vec` — deliberately wrong so every TDD contract test
/// that asserts on emitted readings fails. The GREEN commit replaces this
/// body with the real translation (per-core + aggregate CPU, RAM, disks,
/// processes, uptime) with the T-20 finite-value filter. Cited: G1.
fn readings_from_snapshot(_s: &backend::SysinfoSnapshot) -> Vec<Reading> {
    // Stub: reference the imported types so the compiler + clippy treat them
    // as used (avoids `unused_import` under `-D warnings`). The empty Vec
    // makes every assertion-on-readings test fail — that's the RED state.
    let _ = (SensorId::new("stub", ""), MetricKind::CpuUtilization, Unit::Percent);
    Vec::new()
}

// Re-export key types for downstream consumers (provider registry, poller).
pub use backend::{
    CpuSnapshot, DiskSnapshot, ProcessSnapshot, RealSysinfoBackend, SysinfoSnapshot,
};

#[cfg(test)]
mod tests {
    //! Story 3.1 TDD contract tests.
    //!
    //! These tests exercise `readings_from_snapshot` + the adapter via a
    //! mock backend. The real `sysinfo::System` is exercised by the
    //! integration tests (#[cfg(target_os = "windows")] smoke below).
    //!
    //! Cited:
    //!   - Story 3.1 TDD contract (Happy Path #1-#2, Boundary #1-#5)
    //!   - architecture.md §5.1 (Reading/MetricKind/Unit spec), §5.2 (gauges)
    //!   - nfr-thresholds.md T-1 (Lightweight), T-20 (finite only)
    //!   - guardrails.md G1 (RED test committed before GREEN impl)
    //!   - tdd-fixtures.md F4 (mockall for SensorProvider-side trait)

    use super::*;
    use mockall::mock;
    use sidebar_domain::reading::{MetricKind, Unit};
    use sidebar_sensor::provider::SensorProvider;
    use std::sync::Arc;

    // Auto-mock the `SysinfoBackend` trait so tests can inject canned
    // snapshots without standing up a real `sysinfo::System`.
    mock! {
        pub FakeBackend {}
        impl SysinfoBackend for FakeBackend {
            fn refresh_and_snapshot(&mut self) -> SysinfoSnapshot;
        }
    }

    /// Helper: 8-core CPU snapshot with uniform 25% utilization + 3.0 GHz.
    fn eight_cores() -> Vec<CpuSnapshot> {
        (0..8)
            .map(|_| CpuSnapshot {
                cpu_usage: 25.0,
                frequency: 3_000_000_000.0,
            })
            .collect()
    }

    // ----- Happy Path #1: 8 cores → 8 + 1 aggregate CpuUtilization -----

    /// Story 3.1 Happy Path #1. Cited: Story 3.1 TDD contract.
    /// Fixture: in-process mock. Threshold T-1 (Lightweight).
    #[test]
    fn eight_cores_yields_eight_plus_aggregate_cpu_utilization() {
        let snap = SysinfoSnapshot {
            cpus: eight_cores(),
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let cpu_utils: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuUtilization)
            .collect();
        // 8 per-core + 1 aggregate = 9.
        assert_eq!(cpu_utils.len(), 9, "expected 8 cores + 1 aggregate");
        // Aggregate is the mean of per-core: 25.0.
        let agg = cpu_utils
            .iter()
            .find(|r| r.sensor.category == "cpu" && r.sensor.instance == "package")
            .expect("aggregate cpu present");
        assert!((agg.value - 25.0).abs() < f64::EPSILON);
    }

    // ----- Happy Path #1b: per-core frequency readings emitted too -----

    #[test]
    fn eight_cores_yields_eight_cpu_frequency_readings() {
        let snap = SysinfoSnapshot {
            cpus: eight_cores(),
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let freqs: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuFrequency)
            .collect();
        assert_eq!(freqs.len(), 8, "one frequency per core");
        assert_eq!(freqs[0].unit, Unit::Hertz);
    }

    // ----- Happy Path #2: RAM 8/16 GB → 2 readings -----

    /// Story 3.1 Happy Path #2. Cited: Story 3.1 TDD contract.
    #[test]
    fn ram_eight_of_sixteen_gb_yields_two_readings() {
        let snap = SysinfoSnapshot {
            cpus: Vec::new(),
            memory_used_bytes: 8 * 1024_u64.pow(3),
            memory_total_bytes: 16 * 1024_u64.pow(3),
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let mem: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::MemoryUsed || r.kind == MetricKind::MemoryTotal)
            .collect();
        assert_eq!(mem.len(), 2, "used + total");
        let used = readings
            .iter()
            .find(|r| r.kind == MetricKind::MemoryUsed)
            .unwrap();
        // f64 comparison: this is an exact integer (8 GiB = 8 * 2^30), well
        // within f64's 2^53 exact-integer range, so strict equality is safe.
        assert!((used.value - 8.0 * 1024f64.powi(3)).abs() < f64::EPSILON);
    }

    // ----- Boundary #1: 0 processes → no process readings -----

    /// Story 3.1 Boundary #1. Cited: Story 3.1 TDD contract.
    #[test]
    fn zero_processes_emits_no_process_readings() {
        let snap = SysinfoSnapshot {
            cpus: Vec::new(),
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let procs: Vec<_> = readings
            .iter()
            .filter(|r| {
                r.kind == MetricKind::ProcessCpuPercent || r.kind == MetricKind::ProcessMemoryBytes
            })
            .collect();
        assert!(procs.is_empty(), "zero processes → zero process readings");
    }

    // ----- Boundary #2: empty disk list → no DiskUsed/DiskTotal -----

    /// Story 3.1 Boundary #2. Cited: Story 3.1 TDD contract.
    #[test]
    fn empty_disk_list_emits_no_disk_readings() {
        let snap = SysinfoSnapshot {
            cpus: Vec::new(),
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let disks: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::DiskUsed || r.kind == MetricKind::DiskTotal)
            .collect();
        assert!(disks.is_empty());
    }

    // ----- Boundary #3: CPU usage exactly 100.0 → reading value 100.0 -----

    /// Story 3.1 Boundary #3. Cited: Story 3.1 TDD contract.
    #[test]
    fn cpu_usage_exactly_one_hundred_is_emitted_not_clamped() {
        let snap = SysinfoSnapshot {
            cpus: vec![CpuSnapshot {
                cpu_usage: 100.0,
                frequency: 0.0,
            }],
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        let core = readings
            .iter()
            .find(|r| r.kind == MetricKind::CpuUtilization && r.sensor.category == "cpu/core")
            .unwrap();
        assert!((core.value - 100.0).abs() < f64::EPSILON);
    }

    // ----- Boundary #4: two rapid read_all calls → Mutex allows both -----

    /// Story 3.1 Boundary #4. Cited: Story 3.1 TDD contract. Fixture F6
    /// (idempotency: the Mutex survives repeated acquisition).
    #[test]
    fn two_rapid_read_all_calls_both_succeed() {
        let mut mock = MockFakeBackend::new();
        mock.expect_refresh_and_snapshot()
            .returning(|| SysinfoSnapshot {
                cpus: vec![CpuSnapshot {
                    cpu_usage: 42.0,
                    frequency: 0.0,
                }],
                memory_used_bytes: 0,
                memory_total_bytes: 0,
                disks: Vec::new(),
                processes: Vec::new(),
                uptime_seconds: 1,
            });
        let adapter = SysinfoAdapterGeneric::with_backend(mock);
        let r1 = adapter.read_all();
        let r2 = adapter.read_all();
        // Both calls must succeed (Mutex recovers between calls).
        assert!(!r1.is_empty());
        assert!(!r2.is_empty());
        // Both calls return the same canned snapshot.
        assert_eq!(r1.len(), r2.len());
    }

    // ----- Boundary #5: NaN-typed value is skipped (T-20) -----

    /// Story 3.1 Boundary #5. Cited: Story 3.1 TDD contract, T-20.
    /// sysinfo cannot produce NaN in practice; this test documents the
    /// policy: non-finite values are OMITTED, never emitted.
    #[test]
    fn nan_cpu_usage_is_skipped_per_t20() {
        let snap = SysinfoSnapshot {
            cpus: vec![
                CpuSnapshot {
                    cpu_usage: 50.0,
                    frequency: 1.0,
                },
                CpuSnapshot {
                    cpu_usage: f64::NAN,
                    frequency: f64::NAN,
                },
            ],
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            disks: Vec::new(),
            processes: Vec::new(),
            uptime_seconds: 0,
        };
        let readings = readings_from_snapshot(&snap);
        // The NaN-core contributed neither utilization nor frequency.
        let utils = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuUtilization)
            .count();
        // 1 finite per-core + 1 aggregate (mean over 1 finite core = 50).
        assert_eq!(utils, 2);
        let freqs = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuFrequency)
            .count();
        assert_eq!(freqs, 1, "NaN frequency skipped");
        // No reading value is NaN.
        assert!(readings.iter().all(|r| r.value.is_finite()));
    }

    // ----- Descriptor correctness -----

    /// The descriptor is Tier::Basic + Lightweight (Story 3.1 Technical
    /// Context). Cited: Story 2.2/2.3 classifier contract.
    #[test]
    fn descriptor_is_basic_tier_lightweight() {
        use sidebar_sensor::descriptor::{CostClass, ProviderTier};
        let adapter = SysinfoAdapter::new();
        let d = adapter.descriptor();
        assert_eq!(d.name, "sysinfo");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.requires_tier, ProviderTier::Basic);
        // Declared metrics cover the contract surface.
        assert!(d.metrics.contains(&MetricKind::CpuUtilization));
        assert!(d.metrics.contains(&MetricKind::MemoryUsed));
        assert!(d.metrics.contains(&MetricKind::UptimeSeconds));
    }

    // ----- Mutex poisoning recovery (G15) -----

    /// The adapter MUST NOT propagate Mutex *poison* errors to the poller.
    /// Cited: guardrails.md G15. If a previous `read_all` panicked mid-lock,
    /// the next call MUST recover via `PoisonError::into_inner` rather than
    /// propagating the poison.
    #[test]
    fn read_all_recovers_from_mutex_poison() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;
        let poisoned = StdArc::new(AtomicBool::new(false));
        let backend = OncePanicBackend {
            poisoned: poisoned.clone(),
        };
        let adapter = StdArc::new(SysinfoAdapterGeneric::with_backend(backend));
        // First call: panics inside the lock, poisoning the Mutex. The panic
        // propagates out of `read_all` (the poller catches it via
        // catch_unwind per G15 — that's the poller's responsibility, not the
        // adapter's).
        let a1 = adapter.clone();
        let h = std::thread::spawn(move || a1.read_all());
        let _ = h.join(); // thread panicked — that's expected.
        assert!(poisoned.load(Ordering::SeqCst), "first call must have run");
        // Second call: the Mutex is poisoned, but `read_all` recovers via
        // `unwrap_or_else(|e| e.into_inner())`. The backend's panic-flag is
        // already set, so it returns a default (zero-filled) snapshot this
        // time. Recovery succeeded without re-panic; all readings are finite.
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
    impl SysinfoBackend for OncePanicBackend {
        fn refresh_and_snapshot(&mut self) -> backend::SysinfoSnapshot {
            if !self
                .poisoned
                .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                panic!("poison on first call");
            }
            backend::SysinfoSnapshot::default()
        }
    }

    // ----- Real backend smoke (Windows + any platform that runs sysinfo) -----

    /// Smoke test that the production `RealSysinfoBackend` constructs and
    /// returns SOME readings on the dev machine. Not a contract test — just
    /// a "did the wiring compile + run" guard.
    #[test]
    fn real_backend_returns_some_readings() {
        let adapter = SysinfoAdapter::new();
        let readings = adapter.read_all();
        // Uptime is always present on a real machine; CPU may be 0 on the
        // very first call (sysinfo needs two refresh cycles for CPU%).
        assert!(
            readings.iter().any(|r| r.kind == MetricKind::UptimeSeconds),
            "uptime reading expected from real backend"
        );
    }
}
