//! Story 1.1 — Core reading types (`MetricKind`, `Unit`, `SensorId`,
//! `Reading`, `BatteryState`).
//!
//! This module defines the canonical vocabulary every adapter, formatter,
//! and the bandwidth accountant shares. Defined exactly per architecture.md
//! §5.1; cross-checked against PRD §7 telemetry matrix.
//!
//! **TDD note:** this file contains BOTH the type definitions AND the
//! `#[cfg(test)] mod tests` block. For Story 1.1 the RED→GREEN split is
//! intra-file because the types and their tests are tightly coupled — a
//! standalone test file can't reference the types before they exist in
//! the same compilation unit. The commits are split: RED adds only the
//! test module referencing stub-typed placeholders, GREEN adds the real
//! type defs above the tests.
//!
//! Cited:
//!   - Story 1.1 TDD contract (Happy Path #1-#3, Boundary #1-#3)
//!   - architecture.md §5.1 (exact type spec)
//!   - PRD §7 (telemetry coverage matrix — every row maps to a MetricKind)
//!   - nfr-thresholds.md T-20 (Reading value must be finite)

use std::time::Instant;

use serde::{Deserialize, Serialize};

// ===========================================================================
// MetricKind — 35 variants per architecture.md §5.1.
// Adding/removing a variant is a contract change; the exhaustive-match
// test below catches drift at compile time.
// ===========================================================================

/// Canonical classification of a sensor reading.
///
/// Every telemetry source (Story 3.x adapters) emits `Reading` values
/// tagged with one of these variants. The GUI (Story 8.x) dispatches on
/// `MetricKind` to pick the right formatter + UI row.
///
/// Count: 35 variants (architecture.md §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    // --- CPU ---
    /// CPU utilization %, per-core or aggregate.
    CpuUtilization,
    /// CPU frequency (per-core).
    CpuFrequency,
    /// CPU package temperature (Full mode via LHM).
    CpuTemperature,
    /// CPU package power draw (Full mode).
    CpuPower,
    /// CPU bus/base clock (BCLK) — Full mode via LHM (v1.0 parity).
    CpuBusClock,
    /// Fan speed (RPM) — CPU fan or chassis fans (Full mode).
    FanSpeed,
    /// Voltage rail (VCORE, +3.3V, +5V, +12V, etc. — Full mode).
    Voltage,

    // --- GPU ---
    /// GPU utilization %.
    GpuUtilization,
    /// GPU temperature.
    GpuTemperature,
    /// GPU memory utilization % / VRAM used.
    GpuMemoryUtilization,
    /// GPU power draw.
    GpuPower,
    /// GPU fan speed (RPM) — Full mode.
    GpuFanSpeed,
    /// GPU clock frequency — Full mode.
    GpuFrequency,

    // --- Memory ---
    /// RAM used (bytes).
    MemoryUsed,
    /// RAM total (bytes).
    MemoryTotal,
    /// RAM clock frequency (MHz) — Full mode via LHM (v1.0 parity).
    RamClock,
    /// RAM voltage — Full mode via LHM motherboard sensor (v1.0 parity).
    RamVoltage,

    // --- Storage ---
    /// Per-drive used capacity (bytes).
    DiskUsed,
    /// Per-drive total capacity (bytes).
    DiskTotal,
    /// Per-drive read throughput (bytes/sec).
    DiskReadBytesPerSec,
    /// Per-drive write throughput (bytes/sec).
    DiskWriteBytesPerSec,
    /// SSD SMART endurance remaining (Full mode).
    DiskSmartEndurance,
    /// SSD temperature (Full mode).
    DiskTemperature,

    // --- Network + Bandwidth (v2 amendment) ---
    /// Cumulative RX byte counter (InOctets); delta computed downstream.
    NetRxBytes,
    /// Cumulative TX byte counter (OutOctets).
    NetTxBytes,
    /// Cumulative RX packet counter.
    NetRxPackets,
    /// Cumulative TX packet counter.
    NetTxPackets,
    /// Cumulative RX error counter.
    NetRxErrors,
    /// Cumulative TX error counter.
    NetTxErrors,
    /// Derived: accumulated monthly RX bytes per-LUID (from BandwidthAccountant).
    BandwidthRxBytes,
    /// Derived: accumulated monthly TX bytes per-LUID.
    BandwidthTxBytes,

    // --- Battery ---
    /// Battery charge percent (0–100).
    BatteryPercent,
    /// Battery state — see [`BatteryState`]. The Reading's `value` is the
    /// enum ordinal as `f64` (a stable wire representation across the
    /// adapter boundary).
    BatteryState,
    /// Battery power rate (W — positive=discharging, negative=charging).
    BatteryPowerRate,

    // --- Processes ---
    /// Per-process CPU % (top-N).
    ProcessCpuPercent,
    /// Per-process memory bytes (top-N).
    ProcessMemoryBytes,
    /// Per-process GPU % (NVIDIA-only via NVML; Watch cost class per NFR-1).
    ProcessGpuPercent,

    // --- System ---
    /// System uptime (seconds).
    UptimeSeconds,
}

// ===========================================================================
// Unit — 14 variants per architecture.md §5.1.
// ===========================================================================

/// Canonical measurement unit for a `Reading`.
///
/// Used by the `format_*` module (Story 1.3) to pick the right formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Percentage (0–100).
    Percent,
    /// Temperature in Celsius.
    Celsius,
    /// Temperature in Fahrenheit.
    Fahrenheit,
    /// Temperature in Kelvin.
    Kelvin,
    /// Frequency in Hertz.
    Hertz,
    /// Raw byte count.
    Bytes,
    /// Bytes per second.
    BytesPerSec,
    /// Power in Watts.
    Watts,
    /// Voltage in Volts.
    Volts,
    /// Rotations per minute (fan speeds).
    Rpm,
    /// Time duration in seconds.
    Seconds,
    /// Generic count (packets, errors, processes).
    Count,
    // --- v2 amendment ---
    /// Bits per second (for formatted network throughput display — Mbps/Gbps).
    BitsPerSec,
    /// Packets per second.
    PacketsPerSec,
}

// ===========================================================================
// SensorId — stable identifier for a sensor instance.
// ===========================================================================

/// Stable identifier for a sensor instance.
///
/// `category` is a static string naming the sensor family
/// (`"cpu"`, `"cpu/core"`, `"gpu"`, `"ram"`, `"drive"`, `"net"`,
/// `"battery"`, `"process"`). `instance` is the per-family discriminator
/// (`"package"`, `"0"`, `"nvidia"`, `"C:"`, LUID-as-decimal-string,
/// PID-as-string, etc.).
///
/// Equality + Hash are based on the string values; renaming a NIC or
/// reordering drives changes the `instance` (which is why network adapters
/// are tracked by LUID, not name — see architecture.md AD-12).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SensorId {
    /// Sensor family (`"cpu"`, `"gpu"`, `"drive"`, etc.).
    pub category: &'static str,
    /// Per-family discriminator (`"package"`, `"0"`, `"C:"`, LUID, PID).
    pub instance: String,
}

impl SensorId {
    /// Construct a new `SensorId`.
    #[must_use]
    pub fn new(category: &'static str, instance: impl Into<String>) -> Self {
        Self {
            category,
            instance: instance.into(),
        }
    }
}

// ===========================================================================
// BatteryState — defined here because Story 1.3's format_battery references it.
// ===========================================================================

/// Battery operational state.
///
/// Carried alongside `MetricKind::BatteryPercent` and `BatteryPowerRate`
/// readings from the battery adapter (Story 3.3). The `format_battery`
/// function (Story 1.3) renders e.g. `"78% (Charging)"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BatteryState {
    /// Battery is charging from AC.
    Charging,
    /// Battery is discharging on battery power.
    Discharging,
    /// Battery is idle (AC connected, not actively charging).
    Idle,
    /// Battery state could not be determined.
    Unknown,
}

impl BatteryState {
    /// Convert to the `f64` ordinal stored in a `Reading::value` for
    /// `MetricKind::BatteryState`. Stable wire format across adapter
    /// boundaries.
    ///
    /// Ordering: Charging=0.0, Discharging=1.0, Idle=2.0, Unknown=3.0.
    #[must_use]
    pub fn to_value(self) -> f64 {
        match self {
            Self::Charging => 0.0,
            Self::Discharging => 1.0,
            Self::Idle => 2.0,
            Self::Unknown => 3.0,
        }
    }

    /// Inverse of [`to_value`](Self::to_value). Returns `Unknown` for any
    /// non-finite or unrecognized ordinal.
    ///
    /// Note: `f64::NAN as i64` is `0` on most platforms, which would
    /// incorrectly map to `Charging` — we guard against NaN/Inf explicitly.
    #[must_use]
    pub fn from_value(v: f64) -> Self {
        if !v.is_finite() {
            return Self::Unknown;
        }
        // The cast is safe: v is finite (checked above) and the match only
        // accepts ordinals 0–3 (from to_value); anything else → Unknown.
        // The clippy lint is for the general f64→i64 truncation; here the
        // values are doc-defined to be 0.0..=3.0.
        #[allow(clippy::cast_possible_truncation)]
        match v as i64 {
            0 => Self::Charging,
            1 => Self::Discharging,
            2 => Self::Idle,
            _ => Self::Unknown,
        }
    }
}

// ===========================================================================
// Reading — a single sensor reading.
// ===========================================================================

/// Numeric payload preserving exact cumulative counters while retaining the
/// existing floating-point display path for gauges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ReadingValue {
    /// A continuously valued measurement (temperature, utilization, etc.).
    Gauge(f64),
    /// An exact monotonically increasing counter (network bytes/packets).
    Counter(u64),
}

impl ReadingValue {
    /// Convert the value to the display-facing floating-point projection.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn display_value(self) -> f64 {
        match self {
            Self::Gauge(value) => value,
            Self::Counter(value) => value as f64,
        }
    }

    /// Return the exact counter, if this payload is counter-typed.
    #[must_use]
    pub fn counter_value(self) -> Option<u64> {
        match self {
            Self::Gauge(_) => None,
            Self::Counter(value) => Some(value),
        }
    }
}

/// Filter a raw `f64` to a finite value, returning `None` for NaN/±Inf.
/// Adapters call this to enforce T-20 at the source. Deduplicated here so
/// every adapter shares one definition (cert audit 2026-07-13).
#[must_use]
pub fn finite(v: f64) -> Option<f64> {
    if v.is_finite() {
        Some(v)
    } else {
        None
    }
}

/// A single sensor reading at a point in time.
///
/// `value` MUST be finite per T-20. Adapters MUST omit a reading rather
/// than emit `NaN` or `±Inf`; the `format_*` module renders `"--"` for
/// non-finite values defensively.
#[derive(Debug, Clone)]
pub struct Reading {
    /// Which sensor produced this reading.
    pub sensor: SensorId,
    /// What kind of measurement this is.
    pub kind: MetricKind,
    /// The numeric value. MUST be finite (T-20).
    pub value: f64,
    /// Exact typed payload. `value` remains the display projection for
    /// compatibility with existing gauge formatters and UI callsites.
    pub reading_value: ReadingValue,
    /// The unit of `value`.
    pub unit: Unit,
    /// When the reading was taken.
    pub timestamp: Instant,
}

impl Reading {
    /// Construct a new `Reading`. The `value` is stored as given; adapters
    /// are responsible for ensuring it's finite per T-20.
    #[must_use]
    pub fn new(sensor: SensorId, kind: MetricKind, value: f64, unit: Unit) -> Self {
        Self::gauge(sensor, kind, value, unit)
    }

    /// Construct a gauge-valued reading.
    #[must_use]
    pub fn gauge(sensor: SensorId, kind: MetricKind, value: f64, unit: Unit) -> Self {
        Self {
            sensor,
            kind,
            value,
            reading_value: ReadingValue::Gauge(value),
            unit,
            timestamp: Instant::now(),
        }
    }

    /// Construct an exact counter-valued reading.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn counter(sensor: SensorId, kind: MetricKind, value: u64, unit: Unit) -> Self {
        Self {
            sensor,
            kind,
            value: value as f64,
            reading_value: ReadingValue::Counter(value),
            unit,
            timestamp: Instant::now(),
        }
    }

    /// Borrow the exact typed payload.
    #[must_use]
    pub fn exact_value(&self) -> &ReadingValue {
        &self.reading_value
    }

    /// Return the exact counter, if this reading is counter-typed.
    #[must_use]
    pub fn counter_value(&self) -> Option<u64> {
        self.reading_value.counter_value()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    //! Story 1.1 TDD contract tests.
    //!
    //! Cited: Story 1.1 TDD contract:
    //!   Happy Path #1: SensorId::new("cpu","package") round-trips through
    //!                  Debug/PartialEq/Hash.
    //!   Happy Path #2: Reading { value: 62.0, Celsius, CpuTemperature, ... }
    //!                  constructs and clones.
    //!   Happy Path #3: Exhaustive match over MetricKind returns a
    //!                  &'static str per variant (compile-time exhaustiveness).
    //!   Boundary #1: NaN PartialEq must not equal itself.
    //!   Boundary #2: SensorId with empty instance constructs.
    //!   Boundary #3: Reading accepts f64::INFINITY at construction (no panic).

    use super::*;
    use std::collections::HashMap;

    // ----- finite() helper (T-20 contract, cert iter-1 dedup 2026-07-13) -----

    #[test]
    fn finite_passes_finite_values_and_rejects_nan_inf() {
        assert_eq!(finite(0.0), Some(0.0));
        assert_eq!(finite(-42.5), Some(-42.5));
        assert_eq!(finite(f64::NAN), None);
        assert_eq!(finite(f64::INFINITY), None);
        assert_eq!(finite(f64::NEG_INFINITY), None);
    }

    // ----- Happy Path #1: SensorId round-trips through Debug/PartialEq/Hash -----

    #[test]
    fn sensor_id_round_trips_debug_partialeq_hash() {
        let id = SensorId::new("cpu", "package");
        // Debug
        let dbg = format!("{id:?}");
        assert!(dbg.contains("cpu") && dbg.contains("package"));
        // PartialEq
        let id2 = SensorId::new("cpu", "package");
        assert_eq!(id, id2);
        // Hash — two equal keys collapse in a HashMap
        let mut m: HashMap<SensorId, i32> = HashMap::new();
        m.insert(id, 1);
        m.insert(id2, 2);
        assert_eq!(m.len(), 1, "equal SensorIds must hash-collide");
        assert_eq!(m.get(&SensorId::new("cpu", "package")), Some(&2));
    }

    // ----- Happy Path #2: Reading constructs + clones -----

    #[test]
    fn reading_constructs_and_clones() {
        let r = Reading::new(
            SensorId::new("cpu", "package"),
            MetricKind::CpuTemperature,
            62.0,
            Unit::Celsius,
        );
        let cloned = r.clone();
        assert_eq!(r.sensor, cloned.sensor);
        assert_eq!(r.kind, MetricKind::CpuTemperature);
        assert_eq!(r.unit, Unit::Celsius);
        assert!((r.value - 62.0).abs() < f64::EPSILON);
        assert!(matches!(r.exact_value(), ReadingValue::Gauge(62.0)));
    }

    #[test]
    fn counter_round_trips_exactly_above_f64_precision_limit() {
        let counter = (1_u64 << 53) + 123;
        let reading = Reading::counter(
            SensorId::new("net", "7"),
            MetricKind::NetRxBytes,
            counter,
            Unit::Bytes,
        );
        let encoded = toml::to_string(reading.exact_value()).expect("counter serializes");
        let restored: ReadingValue = toml::from_str(&encoded).expect("counter deserializes");
        assert_eq!(restored, ReadingValue::Counter(counter));
        assert_eq!(reading.counter_value(), Some(counter));
    }

    #[test]
    fn gauge_constructor_keeps_value_and_counter_behavior() {
        let reading = Reading::gauge(
            SensorId::new("cpu", "package"),
            MetricKind::CpuUtilization,
            42.5,
            Unit::Percent,
        );
        assert!((reading.value - 42.5).abs() < f64::EPSILON);
        assert!(reading.counter_value().is_none());
    }

    // ----- Happy Path #3: exhaustive match over MetricKind (compile-time) -----

    #[test]
    fn metric_kind_exhaustive_match_returns_documented_str() {
        // This function MUST handle every variant. When a new variant is
        // added, this match fails to compile, forcing the author to update
        // the canonical-name table. That's the contract — no count-based
        // assertion that can silently drift.
        fn name(k: MetricKind) -> &'static str {
            match k {
                MetricKind::CpuUtilization => "cpu.utilization",
                MetricKind::CpuFrequency => "cpu.frequency",
                MetricKind::CpuTemperature => "cpu.temperature",
                MetricKind::CpuPower => "cpu.power",
                MetricKind::CpuBusClock => "cpu.bus_clock",
                MetricKind::FanSpeed => "fan.speed",
                MetricKind::Voltage => "voltage",
                MetricKind::GpuUtilization => "gpu.utilization",
                MetricKind::GpuTemperature => "gpu.temperature",
                MetricKind::GpuMemoryUtilization => "gpu.memory_utilization",
                MetricKind::GpuPower => "gpu.power",
                MetricKind::GpuFanSpeed => "gpu.fan_speed",
                MetricKind::GpuFrequency => "gpu.frequency",
                MetricKind::MemoryUsed => "memory.used",
                MetricKind::MemoryTotal => "memory.total",
                MetricKind::RamClock => "memory.clock",
                MetricKind::RamVoltage => "memory.voltage",
                MetricKind::DiskUsed => "disk.used",
                MetricKind::DiskTotal => "disk.total",
                MetricKind::DiskReadBytesPerSec => "disk.read_bytes_per_sec",
                MetricKind::DiskWriteBytesPerSec => "disk.write_bytes_per_sec",
                MetricKind::DiskSmartEndurance => "disk.smart_endurance",
                MetricKind::DiskTemperature => "disk.temperature",
                MetricKind::NetRxBytes => "net.rx_bytes",
                MetricKind::NetTxBytes => "net.tx_bytes",
                MetricKind::NetRxPackets => "net.rx_packets",
                MetricKind::NetTxPackets => "net.tx_packets",
                MetricKind::NetRxErrors => "net.rx_errors",
                MetricKind::NetTxErrors => "net.tx_errors",
                MetricKind::BandwidthRxBytes => "bandwidth.rx_bytes",
                MetricKind::BandwidthTxBytes => "bandwidth.tx_bytes",
                MetricKind::BatteryPercent => "battery.percent",
                MetricKind::BatteryState => "battery.state",
                MetricKind::BatteryPowerRate => "battery.power_rate",
                MetricKind::ProcessCpuPercent => "process.cpu_percent",
                MetricKind::ProcessMemoryBytes => "process.memory_bytes",
                MetricKind::ProcessGpuPercent => "process.gpu_percent",
                MetricKind::UptimeSeconds => "uptime.seconds",
            }
        }
        assert_eq!(name(MetricKind::CpuTemperature), "cpu.temperature");
        assert_eq!(name(MetricKind::BatteryPercent), "battery.percent");
        assert_eq!(name(MetricKind::NetRxBytes), "net.rx_bytes");
    }

    // ----- Boundary #1: NaN PartialEq semantics -----

    #[test]
    fn reading_with_nan_value_does_not_equal_itself() {
        // T-20: NaN must not equal itself (IEEE-754 semantics). This test
        // deliberately compares f64 values with assert_ne! — clippy's
        // float_cmp lint would normally fire, but here we're testing the
        // IEEE-754 NaN property directly.
        #![allow(clippy::float_cmp)]
        let r1 = Reading::new(
            SensorId::new("cpu", "package"),
            MetricKind::CpuTemperature,
            f64::NAN,
            Unit::Celsius,
        );
        let r2 = r1.clone();
        // PartialEq on Reading derives from field-wise PartialEq. Since
        // f64::NAN != f64::NAN, two NaN-valued readings are not equal.
        assert_ne!(r1.value, r2.value, "NaN must not equal NaN");
        // Document that adapters MUST NOT emit NaN per T-20. The Reading
        // constructor accepts it (no panic) but format_* renders "--".
    }

    // ----- Boundary #2: SensorId with empty instance -----

    #[test]
    fn sensor_id_with_empty_instance_constructs() {
        // Some sensors are global (no per-instance discriminator) — empty
        // instance string is legal.
        let id = SensorId::new("uptime", "");
        assert_eq!(id.instance, "");
        assert_eq!(id.category, "uptime");
        // An empty-instance SensorId still participates in HashMap correctly.
        let mut m: HashMap<SensorId, i32> = HashMap::new();
        m.insert(id, 42);
        assert_eq!(m.get(&SensorId::new("uptime", "")), Some(&42));
    }

    // ----- Boundary #3: Reading accepts INFINITY at construction -----

    #[test]
    fn reading_accepts_infinity_at_construction() {
        // T-20: format_* must render "--" for non-finite. The Reading
        // constructor MUST NOT panic on INFINITY — it stores it.
        let r = Reading::new(
            SensorId::new("cpu", "package"),
            MetricKind::CpuTemperature,
            f64::INFINITY,
            Unit::Celsius,
        );
        assert!(r.value.is_infinite());
        assert!(r.value.is_sign_positive());
    }

    // ----- Sanity: Unit also has an exhaustive match test -----

    #[test]
    fn unit_exhaustive_match_returns_str() {
        fn unit_name(u: Unit) -> &'static str {
            match u {
                Unit::Percent => "percent",
                Unit::Celsius => "celsius",
                Unit::Fahrenheit => "fahrenheit",
                Unit::Kelvin => "kelvin",
                Unit::Hertz => "hertz",
                Unit::Bytes => "bytes",
                Unit::BytesPerSec => "bytes_per_sec",
                Unit::Watts => "watts",
                Unit::Volts => "volts",
                Unit::Rpm => "rpm",
                Unit::Seconds => "seconds",
                Unit::Count => "count",
                Unit::BitsPerSec => "bits_per_sec",
                Unit::PacketsPerSec => "packets_per_sec",
            }
        }
        assert_eq!(unit_name(Unit::Celsius), "celsius");
        assert_eq!(unit_name(Unit::BitsPerSec), "bits_per_sec");
    }

    // ----- BatteryState round-trip -----

    #[test]
    fn battery_state_round_trips_through_value() {
        for state in [
            BatteryState::Charging,
            BatteryState::Discharging,
            BatteryState::Idle,
            BatteryState::Unknown,
        ] {
            let v = state.to_value();
            assert!(v.is_finite(), "battery state value must be finite");
            let back = BatteryState::from_value(v);
            assert_eq!(state, back, "round-trip failed for {state:?}");
        }
    }

    #[test]
    fn battery_state_from_unknown_value_is_unknown() {
        assert_eq!(BatteryState::from_value(99.0), BatteryState::Unknown);
        assert_eq!(BatteryState::from_value(-1.0), BatteryState::Unknown);
        assert_eq!(BatteryState::from_value(f64::NAN), BatteryState::Unknown);
    }
}
