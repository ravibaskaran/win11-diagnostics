//! `BatteryBackend` trait + `RealBatteryBackend` (starship-battery 0.10 adapter).
//!
//! This module isolates the concrete `starship_battery` API behind a trait so
//! the adapter ([`crate::BatteryAdapterGeneric`]) can be unit-tested with a
//! mock. The trait is intentionally small: one method, `refresh_and_snapshot`,
//! that refreshes the underlying source and returns a plain-data snapshot.
//!
//! ## Why a trait (not `dyn Manager`)
//!
//! `starship_battery::Manager` is a concrete struct, not a trait, and its
//! iteration over batteries returns `Result<Battery>` items (each with a
//! lifetime tied to the manager). We cannot mock it directly. Abstracting
//! behind `BatteryBackend` lets `mockall` generate a mock that returns canned
//! [`BatterySnapshot`]s — this is how the Story 3.3 TDD contract's "mock
//! battery 78% charging" test is satisfied without real hardware.
//!
//! ## starship-battery 0.10 API notes
//!
//! - `Manager::new()` → `Result<Manager>`.
//! - `manager.batteries()` → `Result<Batteries>` where `Batteries: Iterator<Item = Result<Battery>>`.
//! - `battery.state_of_charge()` → `Ratio` (uom typed); `.value` is the raw
//!   `f32` in `0.0..=1.0` (multiply by 100.0 for percent).
//! - `battery.energy_rate()` → `Power` (uom typed); `.value` is the raw `f32`
//!   in watts. Sign convention: positive when discharging (draining), negative
//!   when charging on most platforms. Story 3.3 Boundary #3 documents this.
//! - `battery.state()` → `battery::State` enum (`Charging, Discharging, Empty,
//!   Full, Unknown`).
//!
//! ## No-battery handling
//!
//! Desktops without a battery return an empty iterator from `batteries()`. The
//! backend produces an empty `Vec<BatterySnapshot>` in that case — the adapter
//! then emits zero readings (Story 3.3 Boundary #1: "No battery → empty").
//!
//! Cited: Story 3.3 Technical Context, architecture.md §7.2.

use starship_battery::{Battery, Manager, State};

/// A plain-data snapshot of everything the battery adapter needs from one
/// refresh cycle. Translation to `Reading`s happens in
/// [`crate::readings_from_snapshot`].
///
/// One `BatterySnapshot` represents a single physical battery cell. Machines
/// with multiple batteries produce multiple snapshots per refresh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatterySnapshot {
    /// Battery index in the manager's iteration order (0-based). Used as the
    /// `SensorId.instance` discriminator (`battery/0`, `battery/1`, ...).
    pub index: usize,
    /// State of charge as a percentage in `0.0..=100.0`. `NaN` if the
    /// underlying driver returned no reading (rare; T-20 filters it).
    pub percent: f64,
    /// Operational state, mapped from `starship_battery::State`.
    pub state: BatterySnapshotState,
    /// Power flow rate in watts. Positive = discharging (draining battery),
    /// negative = charging on most platforms (Story 3.3 Boundary #3). `NaN` if
    /// unavailable.
    pub energy_rate_watts: f64,
}

/// Plain-data mirror of `starship_battery::State` so the snapshot is fully
/// mock-friendly (no crate types leak into the test surface).
///
/// Mapping:
/// - `State::Charging` → `Charging`
/// - `State::Discharging` → `Discharging`
/// - `State::Full` / `State::Empty` → `Idle` (per Story 3.3 Boundary #2: a
///   100% Full battery is reported as `Idle`).
/// - `State::Unknown` → `Unknown`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatterySnapshotState {
    /// Charging from AC.
    Charging,
    /// Discharging on battery power.
    Discharging,
    /// Idle (AC connected, not actively charging — includes Full/Empty).
    Idle,
    /// State could not be determined.
    #[default]
    Unknown,
}

/// Convert a `starship_battery::State` into our plain-data mirror.
///
/// `Full` and `Empty` map to `Idle`: Story 3.3 Boundary #2 specifies that a
/// 100% idle battery MUST report `BatteryState::Idle`, and the GUI surfaces
/// `Idle` rather than a separate Full/Empty variant (architecture §5.1 only
/// has Charging/Discharging/Idle/Unknown).
fn state_from_battery(s: State) -> BatterySnapshotState {
    match s {
        State::Charging => BatterySnapshotState::Charging,
        State::Discharging => BatterySnapshotState::Discharging,
        State::Full | State::Empty => BatterySnapshotState::Idle,
        State::Unknown => BatterySnapshotState::Unknown,
    }
}

/// Abstraction over the battery data source. The production impl wraps a real
/// `starship_battery::Manager`; tests substitute a mock.
///
/// Implementations need NOT be `Send + Sync` themselves — the adapter wraps
/// the backend in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait BatteryBackend {
    /// Refresh the underlying source and return one `BatterySnapshot` per
    /// physical battery. Returns an empty vec on desktops with no battery.
    fn refresh_and_snapshot(&mut self) -> Vec<BatterySnapshot>;
}

/// Production backend wrapping a real `starship_battery::Manager`. The manager
/// is created lazily — if the OS exposes no battery API (rare on Windows but
/// possible on servers), `refresh_and_snapshot` logs at debug and returns an
/// empty vec.
pub struct RealBatteryBackend {
    manager: Option<Manager>,
}

impl RealBatteryBackend {
    /// Construct a backend, attempting to initialize the starship-battery
    /// `Manager`. If manager creation fails (no battery subsystem on the OS),
    /// the backend stays in `None` state and emits zero readings forever —
    /// matching the Story 3.3 Boundary #1 contract ("No battery → empty").
    #[must_use]
    pub fn new() -> Self {
        Self {
            manager: Manager::new()
                .map_err(|e| {
                    tracing::debug!("starship-battery Manager::new failed: {e:?}");
                    e
                })
                .ok(),
        }
    }
}

impl Default for RealBatteryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl BatteryBackend for RealBatteryBackend {
    fn refresh_and_snapshot(&mut self) -> Vec<BatterySnapshot> {
        let Some(manager) = self.manager.as_ref() else {
            // Manager never initialized — no battery subsystem. Return empty
            // rather than panicking (Boundary #1).
            return Vec::new();
        };
        let batteries = match manager.batteries() {
            Ok(it) => it,
            Err(e) => {
                tracing::debug!("manager.batteries() failed: {e:?}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for (i, maybe_battery) in batteries.enumerate() {
            match maybe_battery {
                Ok(battery) => out.push(snapshot_from_battery(i, &battery)),
                Err(e) => {
                    // One battery iteration failed — skip it but keep going.
                    // Most platforms never hit this; it's a defensive guard.
                    tracing::debug!("battery {i} iteration failed: {e:?}");
                }
            }
        }
        out
    }
}

/// Translate a `starship_battery::Battery` into our plain-data snapshot.
///
/// `.value` extracts the raw `f32` from the uom-typed `Ratio`/`Power` wrappers
/// (see the starship consumer code for the same pattern). NaN propagation is
/// possible if the driver returns no value; the adapter's `finite()` filter
/// (T-20) drops those downstream.
fn snapshot_from_battery(index: usize, battery: &Battery) -> BatterySnapshot {
    // state_of_charge().value is 0.0..=1.0; multiply by 100 for percent.
    let percent = f64::from(battery.state_of_charge().value) * 100.0;
    let energy_rate_watts = f64::from(battery.energy_rate().value);
    let state = state_from_battery(battery.state());
    BatterySnapshot {
        index,
        percent,
        state,
        energy_rate_watts,
    }
}
