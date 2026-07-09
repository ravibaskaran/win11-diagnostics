//! `sidebar-adapter-net` — per-NIC RX/TX raw byte counters via GetIfEntry2 (Story 3.5).
//!
//! Emits `NetRxBytes` + `NetTxBytes` readings, one pair per non-loopback
//! "up" network adapter reported by the Win32 IpHelper (`GetIfTable2`). The
//! adapter is `Tier::Both`, `CostClass::Lightweight`.
//!
//! ## RAW counter design (architecture §5.2 v2 note + G9)
//!
//! This adapter emits RAW CUMULATIVE counters (`InOctets` / `OutOctets`),
//! not per-tick deltas or rates. Delta-and-divide happens DOWNSTREAM in the
//! `BandwidthAccountant` (Story 5.x): the accountant snapshots this adapter
//! every poll tick, subtracts the prior reading (handling wraparound T-23),
//! and accumulates per-LUID monthly totals. Keeping the raw counter here
//! makes the adapter stateless + idempotent — the same call always returns
//! the same current cumulative value, regardless of poll cadence.
//!
//! ## Architecture
//!
//! All Win32 IpHelper FFI lives behind a [`backend::NetBackend`] trait so the
//! adapter is unit-testable with `mockall`. Production wires
//! [`backend::RealNetBackend`] (stateless; re-enumerates NICs each tick);
//! tests inject a `MockNetBackend` returning canned [`backend::NetSnapshot`]s.
//!
//! The adapter holds a `Mutex<B>` because `refresh_*` requires `&mut` and
//! `SensorProvider::read_all` is `&self` (Story 2.1).
//!
//! ## Cited
//!
//! - Story 3.5 TDD contract (Happy Path #1, Boundary #1-#5)
//! - architecture.md §5.2 (raw cumulative counters), §7.2 (Lightweight surface)
//! - architecture.md AD-12 (LUID stability — HITL G11, fallback R10 = MAC)
//! - nfr-thresholds.md T-20 (finite values only), T-23 (counter wraparound),
//!   T-24 (LUID stability)
//! - guardrails.md G1 (RED before GREEN), G2 (unsafe requires SAFETY comment —
//!   all unsafe is in `backend.rs`, concentrated behind the trait),
//!   G9 (raw counters here; deltas downstream), G15 (mutex poison recovery),
//!   G16 (no panic on missing data), G19 (HITL on new unsafe)
//!
//! NOTE: This is the RED-phase stub. `readings_from_snapshot` returns an
//! empty `Vec` so all positive-assertion TDD contract tests fail. The GREEN
//! commit fills in the translation. Cited: guardrails.md G1.

use std::sync::Mutex;

#[allow(unused_imports)]
// SensorId + Unit are unused during RED (stub returns empty); GREEN uses them.
use sidebar_domain::reading::{MetricKind, Reading, SensorId, Unit};
use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
use sidebar_sensor::provider::SensorProvider;

pub mod backend;

use crate::backend::NetBackend;

/// Descriptor declared once. `Tier::Both` + `CostClass::Lightweight` per
/// Story 3.5 Technical Context (network runs in both Basic + Full modes).
const NET_METRICS: &[MetricKind] = &[MetricKind::NetRxBytes, MetricKind::NetTxBytes];

/// Descriptor for the net adapter.
const DESCRIPTOR: SensorDescriptor = SensorDescriptor::new(
    "net-nic",
    CostClass::Lightweight,
    NET_METRICS,
    ProviderTier::Both,
);

/// Net-backed adapter. Generic over `B: NetBackend` so tests can substitute a
/// mock. The production alias [`NetAdapter`] fixes `B = RealNetBackend`.
pub struct NetAdapterGeneric<B: NetBackend> {
    backend: Mutex<B>,
}

/// Production adapter wired to real Win32 IpHelper counters.
///
/// Construction never fails — `RealNetBackend` is stateless. If IpHelper is
/// unavailable (no NICs reported / GetIfTable2 errors), the adapter yields
/// empty snapshots every tick (Boundary #1: zero NICs → empty, no panic).
pub type NetAdapter = NetAdapterGeneric<backend::RealNetBackend>;

impl NetAdapter {
    /// Construct the production net adapter.
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(backend::RealNetBackend::new())
    }
}

impl Default for NetAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: NetBackend> NetAdapterGeneric<B> {
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

impl<B: NetBackend + Send> SensorProvider for NetAdapterGeneric<B> {
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

/// Translate a [`backend::NetSnapshot`] into the canonical `Vec<Reading>`.
///
/// Each live NIC yields a `NetRxBytes` + `NetTxBytes` pair, both with
/// `SensorId` category `"nic"` and instance = the LUID rendered as a decimal
/// string. Both readings are emitted even at 0 — Boundary #4 contract: a
/// zero-traffic NIC is still reported (the cumulative counter is 0). The
/// downstream accountant treats wraparound as a reset (T-23, handled there).
///
/// # Finite-value policy (T-20)
///
/// Any reading whose `value` is `NaN` or `±Inf` is OMITTED. Cumulative byte
/// counters are `u64` and cannot be NaN, but the cast to `f64` is applied
/// consistently with other adapters for defense-in-depth. (Counters above
/// 2^53 lose integer precision in the f64; the downstream accountant reads
/// them as `u64` directly from the snapshot — see AD-12 — so this is only a
/// display-channel concern.)
fn readings_from_snapshot(s: &backend::NetSnapshot) -> Vec<Reading> {
    // RED-phase stub: returns empty Vec. GREEN commit fills in the per-NIC
    // NetRxBytes + NetTxBytes translation. Cited: guardrails.md G1.
    let _ = s;
    Vec::new()
}

/// Returns `Some(v)` only when `v` is finite; `None` otherwise. T-20: adapters
/// MUST omit non-finite readings rather than emit `NaN`/`±Inf`.
#[inline]
#[allow(dead_code)] // RED-phase stub does not call this yet; GREEN commit uses it.
fn finite(v: f64) -> Option<f64> {
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

// Re-export key types for downstream consumers.
pub use backend::{NetSnapshot, NicSnapshot, RealNetBackend};

#[cfg(test)]
mod tests {
    //! Story 3.5 TDD contract tests.
    //!
    //! These tests exercise `readings_from_snapshot` + the adapter via a mock
    //! backend. The real IpHelper path is exercised by the `#[ignore]`'d
    //! integration smoke test below (run via `cargo test --ignored`).
    //!
    //! Cited:
    //!   - Story 3.5 TDD contract (Happy Path #1, Boundary #1-#5)
    //!   - architecture.md §7.2 (Lightweight adapter), AD-12 (LUID tracking)
    //!   - nfr-thresholds.md T-20 (finite values only), T-23 (wraparound),
    //!     T-24 (LUID stability)
    //!   - guardrails.md G1 (RED before GREEN), G9 (raw counters here),
    //!     G15 (mutex poison recovery), G16 (no panic on missing data),
    //!     G2 (unsafe requires SAFETY — all in backend.rs)
    //!   - tdd-fixtures.md F4 (mockall), F11 (unsafe FFI contract)

    use super::*;
    use mockall::mock;
    use sidebar_domain::reading::{MetricKind, Unit};
    use sidebar_sensor::descriptor::{CostClass, ProviderTier};
    use sidebar_sensor::provider::SensorProvider;
    use std::sync::Arc;

    // Auto-mock the `NetBackend` trait so tests inject canned snapshots.
    mock! {
        pub FakeBackend {}
        impl NetBackend for FakeBackend {
            fn refresh_and_snapshot(&mut self) -> NetSnapshot;
        }
    }

    /// Helper: snapshot with two NICs (LUID 1, LUID 2), each RX=1000, TX=2000.
    fn two_nics() -> NetSnapshot {
        NetSnapshot {
            nics: vec![
                NicSnapshot {
                    luid: 1,
                    rx_bytes: 1000,
                    tx_bytes: 2000,
                },
                NicSnapshot {
                    luid: 2,
                    rx_bytes: 1000,
                    tx_bytes: 2000,
                },
            ],
        }
    }

    // ----- Happy Path #1: mock two NICs (LUID 1, LUID 2) → 4 readings -----

    /// Story 3.5 Happy Path #1. Two NICs × 2 directions = 4 readings; each
    /// reading's `SensorId.instance` is the LUID rendered as a decimal string.
    /// Cited: Story 3.5 TDD contract.
    #[test]
    fn two_nics_yield_four_readings_with_luid_instance() {
        let snap = two_nics();
        let readings = readings_from_snapshot(&snap);
        assert_eq!(readings.len(), 4, "2 NICs × 2 directions = 4 readings");
        // Every reading's instance is "1" or "2" (LUID-as-decimal-string).
        for r in &readings {
            assert!(
                r.sensor.instance == "1" || r.sensor.instance == "2",
                "instance must be the LUID-as-string, got {}",
                r.sensor.instance
            );
            assert_eq!(r.sensor.category, "nic");
        }
        // Per-NIC: one NetRxBytes + one NetTxBytes.
        let luid1_rx = readings
            .iter()
            .find(|r| r.sensor.instance == "1" && r.kind == MetricKind::NetRxBytes)
            .expect("LUID 1 RX reading present");
        assert!((luid1_rx.value - 1000.0).abs() < f64::EPSILON);
        assert_eq!(luid1_rx.unit, Unit::Bytes);
        let luid2_tx = readings
            .iter()
            .find(|r| r.sensor.instance == "2" && r.kind == MetricKind::NetTxBytes)
            .expect("LUID 2 TX reading present");
        assert!((luid2_tx.value - 2000.0).abs() < f64::EPSILON);
    }

    // ----- Boundary #1: zero NICs → empty -----

    /// Story 3.5 Boundary #4 (zero NICs). Cited: Story 3.5 TDD contract.
    #[test]
    fn empty_snapshot_emits_no_readings() {
        let snap = NetSnapshot { nics: Vec::new() };
        let readings = readings_from_snapshot(&snap);
        assert!(readings.is_empty(), "zero NICs → empty readings");
    }

    // ----- Boundary: single NIC → 2 readings -----

    #[test]
    fn single_nic_yields_two_readings() {
        let snap = NetSnapshot {
            nics: vec![NicSnapshot {
                luid: 42,
                rx_bytes: 500,
                tx_bytes: 700,
            }],
        };
        let readings = readings_from_snapshot(&snap);
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].sensor.instance, "42");
        assert!(readings.iter().any(|r| r.kind == MetricKind::NetRxBytes));
        assert!(readings.iter().any(|r| r.kind == MetricKind::NetTxBytes));
    }

    // ----- Boundary: cumulative counters preserved (raw, not delta) -----

    /// The adapter emits RAW cumulative counters, not per-tick deltas (G9).
    /// A NIC with InOctets=1_000_000_000 emits exactly that value — the
    /// downstream accountant computes deltas later.
    #[test]
    fn raw_cumulative_counter_is_preserved_not_delta() {
        let snap = NetSnapshot {
            nics: vec![NicSnapshot {
                luid: 7,
                rx_bytes: 1_000_000_000,
                tx_bytes: 2_000_000_000,
            }],
        };
        let readings = readings_from_snapshot(&snap);
        let rx = readings
            .iter()
            .find(|r| r.kind == MetricKind::NetRxBytes)
            .expect("RX present");
        let tx = readings
            .iter()
            .find(|r| r.kind == MetricKind::NetTxBytes)
            .expect("TX present");
        assert!(
            (rx.value - 1_000_000_000.0).abs() < f64::EPSILON,
            "raw cumulative value preserved"
        );
        assert!(
            (tx.value - 2_000_000_000.0).abs() < f64::EPSILON,
            "raw cumulative value preserved"
        );
    }

    // ----- Adapter wiring via mock backend (F4) -----

    /// The adapter correctly delegates to the backend and translates the
    /// returned snapshot. Cited: Story 3.5 TDD contract, F4.
    #[test]
    fn adapter_translates_backend_snapshot() {
        let mut mock = MockFakeBackend::new();
        mock.expect_refresh_and_snapshot().returning(two_nics);
        let adapter = NetAdapterGeneric::with_backend(mock);
        let readings = adapter.read_all();
        assert_eq!(
            readings
                .iter()
                .filter(|r| r.kind == MetricKind::NetRxBytes)
                .count(),
            2,
            "two RX readings (one per NIC)"
        );
        assert_eq!(
            readings
                .iter()
                .filter(|r| r.kind == MetricKind::NetTxBytes)
                .count(),
            2,
            "two TX readings (one per NIC)"
        );
    }

    // ----- Descriptor correctness (Tier::Both + Lightweight) -----

    /// The descriptor is Tier::Both + Lightweight (Story 3.5 Technical
    /// Context). Cited: Story 2.2/2.3 classifier contract.
    #[test]
    fn descriptor_is_both_tier_lightweight() {
        let adapter = NetAdapter::new();
        let d = adapter.descriptor();
        assert_eq!(d.name, "net-nic");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.requires_tier, ProviderTier::Both);
        assert!(d.metrics.contains(&MetricKind::NetRxBytes));
        assert!(d.metrics.contains(&MetricKind::NetTxBytes));
    }

    // ----- Mutex poisoning recovery (G15) -----

    /// The adapter MUST NOT propagate mutex poison to the poller. Cited:
    /// guardrails.md G15. If a prior `read_all` panicked mid-lock, the next
    /// call recovers via `PoisonError::into_inner`.
    #[test]
    fn read_all_recovers_from_mutex_poison() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let poisoned = Arc::new(AtomicBool::new(false));
        let backend = OncePanicBackend {
            poisoned: poisoned.clone(),
        };
        let adapter = Arc::new(NetAdapterGeneric::with_backend(backend));
        // First call: panics inside the lock, poisoning the Mutex. The panic
        // propagates out of read_all (the poller's catch_unwind catches it).
        let a1 = adapter.clone();
        let h = std::thread::spawn(move || a1.read_all());
        let _ = h.join(); // thread panicked — expected.
        assert!(poisoned.load(Ordering::SeqCst), "first call must have run");
        // Second call: the Mutex is poisoned, but read_all recovers.
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
    impl NetBackend for OncePanicBackend {
        fn refresh_and_snapshot(&mut self) -> NetSnapshot {
            // First call: poisoned is false → swap returns false → assert fails → panic.
            // Second+ call: poisoned is true → swap returns true → assert passes.
            assert!(
                self.poisoned
                    .swap(true, std::sync::atomic::Ordering::SeqCst),
                "poison on first call"
            );
            NetSnapshot::default()
        }
    }

    // ----- Real backend smoke (Windows; #[ignore] NIC set is machine-specific) -----

    /// Smoke that the production `RealNetBackend` constructs and returns SOME
    /// structure without panicking. Marked `#[ignore]` because the exact NIC
    /// set is machine-dependent (count + LUIDs vary) — we only assert no panic
    /// + finite values (T-20). Run via `cargo test --ignored`.
    #[test]
    #[ignore = "real NIC set is machine-specific; run via cargo test --ignored"]
    fn real_backend_constructs_without_panic() {
        let adapter = NetAdapter::new();
        let readings = adapter.read_all();
        assert!(
            readings.iter().all(|r| r.value.is_finite()),
            "all real-backend readings must be finite"
        );
    }

    // ----- Silent unused-warning guard (RED phase only) -----

    /// During the RED phase `finite` is intentionally unused — keep it
    /// referenced so the workspace lint does not deny the build. GREEN
    /// removes this test along with the `_ = s` placeholder.
    #[test]
    fn finite_helper_is_sane() {
        assert_eq!(finite(0.0), Some(0.0));
        assert_eq!(finite(f64::NAN), None);
        assert_eq!(finite(f64::INFINITY), None);
    }
}
