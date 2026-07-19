//! `sidebar-adapter-pdh` — per-drive disk R/W throughput via Win32 PDH (Story 3.4).
//!
//! Emits `DiskReadBytesPerSec` + `DiskWriteBytesPerSec` readings, one pair per
//! physical-disk instance reported by the Performance Data Helper (PDH). The
//! adapter is `Tier::Basic`, `CostClass::Lightweight`.
//!
//! ## Architecture
//!
//! All Win32 PDH FFI lives behind a [`backend::PdhBackend`] trait so the
//! adapter is unit-testable with `mockall`. Production wires
//! [`backend::RealPdhBackend`] (owns an open PDH query + read/write counters);
//! tests inject a `MockPdhBackend` returning canned [`backend::PdhSnapshot`]s.
//!
//! The adapter holds a `Mutex<B>` because `refresh_*` requires `&mut` and
//! `SensorProvider::read_all` is `&self` (Story 2.1).
//!
//! ## Cited
//!
//! - Story 3.4 TDD contract (Happy Path #1, Boundary #1-#4)
//! - architecture.md §7.2 (Lightweight adapter surface)
//! - nfr-thresholds.md T-20 (finite values only)
//! - guardrails.md G2 (unsafe requires SAFETY comment — all unsafe is in
//!   `backend.rs`, concentrated behind the trait)
//!
//! NOTE: This is the RED-phase stub. `readings_from_snapshot` returns an
//! empty `Vec` so all positive-assertion TDD contract tests fail. The GREEN
//! commit fills in the translation. Cited: guardrails.md G1 (RED before GREEN).

use std::sync::Mutex;

use sidebar_domain::reading::{finite, MetricKind, Reading, SensorId, Unit};
use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
use sidebar_sensor::provider::SensorProvider;

pub mod backend;

use crate::backend::PdhBackend;

/// Descriptor declared once. `Tier::Basic` + `CostClass::Lightweight` per
/// Story 3.4 Technical Context.
const PDH_METRICS: &[MetricKind] = &[
    MetricKind::DiskReadBytesPerSec,
    MetricKind::DiskWriteBytesPerSec,
];

/// Descriptor for the PDH adapter.
const DESCRIPTOR: SensorDescriptor = SensorDescriptor::new(
    "pdh-disk",
    CostClass::Lightweight,
    PDH_METRICS,
    ProviderTier::Basic,
);

/// PDH-backed adapter. Generic over `B: PdhBackend` so tests can substitute a
/// mock. The production alias [`PdhAdapter`] fixes `B = RealPdhBackend`.
pub struct PdhAdapterGeneric<B: PdhBackend> {
    backend: Mutex<B>,
}

/// Production adapter wired to real Win32 PDH counters.
///
/// Construction returns the adapter even when PDH is unavailable — the
/// backend's `refresh_and_snapshot` simply returns empty snapshots in that
/// case (Boundary #1: PDH unavailable → empty, `debug!`, no panic). This
/// keeps the poller's provider registry uniform; no adapter is ever `None`.
pub type PdhAdapter = PdhAdapterGeneric<backend::RealPdhBackend>;

impl PdhAdapter {
    /// Construct the production PDH adapter. If PDH is unavailable on this
    /// machine, the adapter still constructs but yields empty readings every
    /// tick.
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(backend::RealPdhBackend::new().unwrap_or_default())
    }
}

impl Default for PdhAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: PdhBackend> PdhAdapterGeneric<B> {
    /// Construct an adapter wrapping the given backend. The backend is
    /// wrapped in a `Mutex` so `read_all` (which is `&self`) can call
    /// `&mut self` refresh methods.
    #[must_use]
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend: Mutex::new(backend),
        }
    }
}

impl<B: PdhBackend + Send> SensorProvider for PdhAdapterGeneric<B> {
    fn descriptor(&self) -> &SensorDescriptor {
        &DESCRIPTOR
    }

    fn read_all(&self) -> Vec<Reading> {
        // Lock once per tick. Poison is recovered (G15) rather than
        // propagated — the poller wraps `read_all` in `catch_unwind` but the
        // adapter must not re-panic on the next tick after a prior panic.
        let mut guard = self
            .backend
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = guard.refresh_and_snapshot();
        readings_from_snapshot(&snapshot)
    }
}

/// Translate a [`backend::PdhSnapshot`] into the canonical `Vec<Reading>`.
///
/// Each drive yields a `DiskReadBytesPerSec` + `DiskWriteBytesPerSec` pair,
/// both with `SensorId` category `"drive"` and instance = the PDH drive name
/// (e.g. `"0 C:"`). Both readings are emitted even at 0.0 — Boundary #2
/// contract: zero-activity drives are reported, not omitted.
///
/// # Finite-value policy (T-20)
///
/// Any reading whose `value` is `NaN` or `±Inf` is OMITTED. PDH cannot
/// produce NaN in practice (it reports `largeValue: i64`), but the defense
/// documents the policy and guards against a future format change.
fn readings_from_snapshot(s: &backend::PdhSnapshot) -> Vec<Reading> {
    let mut out = Vec::with_capacity(s.drives.len() * 2);
    for d in &s.drives {
        if let Some(read) = finite(d.read_bytes_per_sec) {
            out.push(Reading::new(
                SensorId::new("drive", d.instance.clone()),
                MetricKind::DiskReadBytesPerSec,
                read,
                Unit::Bytes,
            ));
        }
        if let Some(write) = finite(d.write_bytes_per_sec) {
            out.push(Reading::new(
                SensorId::new("drive", d.instance.clone()),
                MetricKind::DiskWriteBytesPerSec,
                write,
                Unit::Bytes,
            ));
        }
    }
    out
}

// Re-export key snapshot types so the crate's tests (and any future consumer)
// can name them without reaching into `backend::` directly.
pub use backend::{DiskSnapshot, PdhSnapshot, RealPdhBackend};

#[cfg(test)]
mod tests {
    //! Story 3.4 TDD contract tests.
    //!
    //! These tests exercise `readings_from_snapshot` + the adapter via a mock
    //! backend. The real PDH path is exercised by the `#[ignore]`'d
    //! integration smoke test below (run via `cargo test --ignored`).
    //!
    //! Cited:
    //!   - Story 3.4 TDD contract (Happy Path #1, Boundary #1-#4)
    //!   - architecture.md §7.2 (Lightweight adapter)
    //!   - nfr-thresholds.md T-20 (finite values only)
    //!   - guardrails.md G1 (RED before GREEN), G15 (mutex poison recovery),
    //!     G2 (unsafe requires SAFETY — all in backend.rs)
    //!   - tdd-fixtures.md F4 (mockall), F11 (unsafe FFI contract)

    use super::*;
    use mockall::mock;
    use sidebar_domain::reading::{MetricKind, Unit};
    use sidebar_sensor::provider::SensorProvider;
    use std::sync::Arc;

    // Auto-mock the `PdhBackend` trait so tests inject canned snapshots.
    mock! {
        pub FakeBackend {}
        impl PdhBackend for FakeBackend {
            fn refresh_and_snapshot(&mut self) -> PdhSnapshot;
        }
    }

    /// Helper: snapshot with one drive reading 1 MB/s, writing 2 MB/s.
    fn one_drive() -> PdhSnapshot {
        PdhSnapshot {
            drives: vec![DiskSnapshot {
                instance: "0 C:".to_string(),
                read_bytes_per_sec: 1_048_576.0,  // 1 MB/s
                write_bytes_per_sec: 2_097_152.0, // 2 MB/s
            }],
        }
    }

    // ----- Happy Path #1: mock PDH C: read 1 MB/s, write 2 MB/s → 2 readings -----

    /// Story 3.4 Happy Path #1. Cited: Story 3.4 TDD contract.
    #[test]
    fn one_drive_yields_read_and_write_readings() {
        let snap = one_drive();
        let readings = readings_from_snapshot(&snap);
        let r = readings
            .iter()
            .find(|x| x.kind == MetricKind::DiskReadBytesPerSec)
            .expect("read reading present");
        let w = readings
            .iter()
            .find(|x| x.kind == MetricKind::DiskWriteBytesPerSec)
            .expect("write reading present");
        assert!((r.value - 1_048_576.0).abs() < f64::EPSILON);
        assert!((w.value - 2_097_152.0).abs() < f64::EPSILON);
        assert_eq!(r.unit, Unit::Bytes);
        assert_eq!(w.unit, Unit::Bytes);
        // Both readings share the same SensorId instance (the drive name).
        assert_eq!(r.sensor.instance, "0 C:");
        assert_eq!(w.sensor.instance, "0 C:");
        assert_eq!(r.sensor.category, "drive");
    }

    // ----- Boundary #1: PDH unavailable → empty, no panic -----

    /// Story 3.4 Boundary #1. Cited: Story 3.4 TDD contract.
    #[test]
    fn empty_snapshot_emits_no_readings() {
        let snap = PdhSnapshot { drives: Vec::new() };
        let readings = readings_from_snapshot(&snap);
        assert!(readings.is_empty(), "PDH unavailable → empty readings");
    }

    // ----- Boundary #2: zero-activity drive → value 0.0 (not omitted) -----

    /// Story 3.4 Boundary #2. Cited: Story 3.4 TDD contract.
    #[test]
    fn zero_activity_drive_emits_zero_not_omitted() {
        let snap = PdhSnapshot {
            drives: vec![DiskSnapshot {
                instance: "0 C:".to_string(),
                read_bytes_per_sec: 0.0,
                write_bytes_per_sec: 0.0,
            }],
        };
        let readings = readings_from_snapshot(&snap);
        // Both readings present, both 0.0 — zero-activity is still reported.
        assert_eq!(readings.len(), 2, "zero-activity drive → 2 readings at 0.0");
        assert!(readings.iter().all(|r| r.value == 0.0));
    }

    // ----- Boundary #3: multiple drives → per-instance readings -----

    /// Story 3.4 Boundary #3 (multiple physical disks). Cited: Story 3.4 TDD
    /// contract. Two drives → 4 readings (2 per drive), each instance-correct.
    #[test]
    fn two_drives_yield_four_readings_with_distinct_instances() {
        let snap = PdhSnapshot {
            drives: vec![
                DiskSnapshot {
                    instance: "0 C:".to_string(),
                    read_bytes_per_sec: 100.0,
                    write_bytes_per_sec: 200.0,
                },
                DiskSnapshot {
                    instance: "1 D:".to_string(),
                    read_bytes_per_sec: 300.0,
                    write_bytes_per_sec: 400.0,
                },
            ],
        };
        let readings = readings_from_snapshot(&snap);
        assert_eq!(readings.len(), 4, "2 drives × 2 directions = 4 readings");
        let c_read = readings
            .iter()
            .find(|r| r.sensor.instance == "0 C:" && r.kind == MetricKind::DiskReadBytesPerSec)
            .expect("C: read present");
        assert!((c_read.value - 100.0).abs() < f64::EPSILON);
        let d_write = readings
            .iter()
            .find(|r| r.sensor.instance == "1 D:" && r.kind == MetricKind::DiskWriteBytesPerSec)
            .expect("D: write present");
        assert!((d_write.value - 400.0).abs() < f64::EPSILON);
    }

    // ----- Boundary #4: adapter wiring via mock backend -----

    /// The adapter correctly delegates to the backend and translates the
    /// returned snapshot. Cited: Story 3.4 TDD contract, F4.
    #[test]
    fn adapter_translates_backend_snapshot() {
        let mut mock = MockFakeBackend::new();
        mock.expect_refresh_and_snapshot().returning(one_drive);
        let adapter = PdhAdapterGeneric::with_backend(mock);
        let readings = adapter.read_all();
        assert_eq!(
            readings
                .iter()
                .filter(|r| r.kind == MetricKind::DiskReadBytesPerSec)
                .count(),
            1,
            "one read reading"
        );
    }

    // ----- Descriptor correctness -----

    /// The descriptor is Tier::Basic + Lightweight (Story 3.4 Technical
    /// Context). Cited: Story 2.2/2.3 classifier contract.
    #[test]
    fn descriptor_is_basic_tier_lightweight() {
        use sidebar_sensor::descriptor::{CostClass, ProviderTier};
        let adapter = PdhAdapter::new();
        let d = adapter.descriptor();
        assert_eq!(d.name, "pdh-disk");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.requires_tier, ProviderTier::Basic);
        assert!(d.metrics.contains(&MetricKind::DiskReadBytesPerSec));
        assert!(d.metrics.contains(&MetricKind::DiskWriteBytesPerSec));
    }

    // ----- Mutex poisoning recovery (G15) -----

    /// The adapter MUST NOT propagate mutex poison to the poller. Cited:
    /// guardrails.md G15. If a prior `read_all` panicked mid-lock, the next
    /// call recovers via `PoisonError::into_inner`.
    #[test]
    fn read_all_recovers_from_mutex_poison() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let poisoned = Arc::new(AtomicBool::new(false));
        let backend = OncePanicBackend {
            poisoned: poisoned.clone(),
        };
        let adapter = Arc::new(PdhAdapterGeneric::with_backend(backend));
        // First call: panics inside the lock, poisoning the Mutex. The panic
        // propagates out of read_all (the poller's catch_unwind catches it).
        let a1 = adapter.clone();
        let h = std::thread::spawn(move || a1.read_all());
        let _ = h.join(); // thread panicked — expected.
        assert!(poisoned.load(Ordering::SeqCst), "first call must have run");
        // Second call: the Mutex is poisoned, but read_all recovers. The
        // backend's panic-flag is set, so it returns an empty snapshot this
        // time. All readings are finite (empty → trivially all finite).
        let v = adapter.read_all();
        assert!(
            v.iter().all(|r| r.value.is_finite()),
            "poison recovery yields finite readings"
        );
    }

    /// Backend that panics on the first call, then returns empty on
    /// subsequent calls. Used to poison the adapter Mutex.
    struct OncePanicBackend {
        poisoned: Arc<std::sync::atomic::AtomicBool>,
    }
    impl PdhBackend for OncePanicBackend {
        fn refresh_and_snapshot(&mut self) -> PdhSnapshot {
            // First call: poisoned is false → swap returns false → assert fails → panic.
            // Second+ call: poisoned is true → swap returns true → assert passes.
            assert!(
                self.poisoned
                    .swap(true, std::sync::atomic::Ordering::SeqCst),
                "poison on first call"
            );
            PdhSnapshot::default()
        }
    }

    // ----- Real backend smoke (Windows; #[ignore] requires PDH installed) -----

    /// Smoke that the production `RealPdhBackend` constructs and returns
    /// SOME structure without panicking. Marked `#[ignore]` because the
    /// first sample returns empty (PDH needs two samples) and the exact
    /// drive set is machine-dependent. Run via `cargo test --ignored`.
    #[test]
    #[ignore = "PDH first-sample returns empty; drive set is machine-specific"]
    fn real_backend_constructs_without_panic() {
        let adapter = PdhAdapter::new();
        // First call primes PDH (returns empty). We only assert it does not
        // panic and that any readings produced are finite (T-20).
        let readings = adapter.read_all();
        assert!(
            readings.iter().all(|r| r.value.is_finite()),
            "all real-backend readings must be finite"
        );
    }
}
