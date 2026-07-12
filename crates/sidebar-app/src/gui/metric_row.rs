//! Story 8.3 — Metric Row (NFR-8).
//!
//! A metric row component formatting each reading via the Story 1.3 `format_*`
//! functions. The dispatch table maps `MetricKind × Unit` → the correct
//! formatter, respecting `config.display.{raw_values, temp_unit, decimal_base}`
//! (Story 1.5 config):
//!
//! - `raw_values=true` → emit the raw integer (Hz, bytes, bps) without the
//!   human-readable scaling. T-28 raw mode.
//! - `temp_unit=Fahrenheit` → convert Celsius → Fahrenheit app-wide (T-29).
//! - `decimal_base=true` → decimal GB (10⁹); `false` → binary GiB (2³⁰) (T-28).
//!
//! Boundary cases (T-20, T-28, T-29):
//!
//! - NaN / ±Inf → `"--"` (T-20 defensive — adapters must not emit NaN, but we
//!   guard).
//! - Unknown `MetricKind × Unit` combination → `"unknown"`, logged at `warn!`.
//!
//! ## Format dispatch table (MetricKind × Unit → Story 1.3 fn)
//!
//! | Kind group                                  | Unit            | Formatter (human)         | Raw mode                |
//! |---------------------------------------------|-----------------|---------------------------|-------------------------|
//! | CpuFrequency, GpuFrequency                  | Hertz           | `format_hz`               | `"{hz} Hz"`             |
//! | CpuTemperature, GpuTemperature, DiskTemp... | Celsius/Fahr/K  | `format_temp(value, cfg)` | (same — temp has no raw)|
//! | CpuUtil, GpuUtil, GpuMemUtil, BatteryPct    | Percent         | `format_percent`          | (same)                  |
//! | CpuPower, GpuPower, BatteryPowerRate        | Watts           | `format_power`            | (same)                  |
//! | Voltage                                     | Volts           | `format_voltage`          | (same)                  |
//! | FanSpeed, GpuFanSpeed                       | Rpm             | `format_rpm`              | (same)                  |
//! | Memory*, Disk{Used,Total}, ProcessMem, Net*Bytes, Bandwidth*Bytes | Bytes | `format_bytes(b, base)` | `"{b} B"`     |
//! | Disk{Read,Write}BytesPerSec                 | BytesPerSec     | `format_bytes + "/s"`     | `"{b} B/s"`             |
//! | Net*Errors, Net*Packets                     | Count/PacketsPerSec | `format_count`         | (same)                  |
//! | Process{Cpu,Gpu}Percent                     | Percent         | `format_percent`          | (same)                  |
//! | UptimeSeconds                               | Seconds         | `format_uptime`           | (same)                  |
//!
//! Any other pair → `"unknown"`, logged at `warn!` (defensive — adapters
//! should never emit a mismatched kind × unit, but we never panic).
//!
//! ## Cited
//!
//! - Story 8.3 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - architecture.md AD-13 + §4 (`metric_row.rs`) + §7.1 (format match table)
//! - nfr-thresholds.md T-20 (finite), T-28 (decimal/binary), T-29 (temp unit),
//!   T-30 (precision rules)
//! - sidebar-domain::format (Story 1.3) + sidebar-domain::config (Story 1.5)

use eframe::egui::{Color32, Ui};
use sidebar_domain::config::DisplayConfig;
use sidebar_domain::format::{self, Base, TempUnit};
use sidebar_domain::reading::{BatteryState, MetricKind, Reading, Unit};

use crate::gui::kind_label;

/// Render one metric row: a short kind label + a formatted value, using the
/// given display config to pick the formatter (Story 8.3 NFR-8 dispatch).
///
/// Splits the row into two labels (kind, value) so the F8 access tree can
/// query each independently — mirrors the Story 8.1 layout. The kind label
/// is the short uppercase form (e.g. `"CPU"`); the value is the dispatched
/// formatter output (e.g. `"3.84 GHz"`).
pub fn render(ui: &mut Ui, reading: &Reading, display: &DisplayConfig) {
    let formatted = format_reading_with_config(reading, display);
    ui.horizontal(|row| {
        row.label(kind_label(reading.kind));
        row.label(formatted);
    });
}

/// Story 8.8 — same as [`render`] but tints both labels with the given color.
///
/// `color` is the alert color from [`crate::gui::alert_indicator::color_for`]:
/// the default text color for `Normal`, the accent for `Warning`, and
/// `CRITICAL_RED` for `Critical`. Tinting the value label (not just the kind)
/// keeps the alert visible at a glance even when the row scrolls past the
/// status pill.
pub fn render_with_color(ui: &mut Ui, reading: &Reading, display: &DisplayConfig, color: Color32) {
    let formatted = format_reading_with_config(reading, display);
    ui.horizontal(|row| {
        row.colored_label(color, kind_label(reading.kind));
        row.colored_label(color, formatted);
    });
}

/// Format a reading's value per the MetricKind × Unit dispatch table, honoring
/// the DisplayConfig toggles. Returns `"--"` for NaN (T-20) and `"unknown"`
/// for unrecognized MetricKind × Unit combinations (logged at `warn!`).
#[must_use]
pub(crate) fn format_reading_with_config(reading: &Reading, display: &DisplayConfig) -> String {
    let Reading {
        kind, value, unit, ..
    } = reading;

    // T-20: NaN/Inf → "--" for every metric. Adapters MUST NOT emit non-finite
    // values; this guard is defensive (format_* also guards, but we centralize
    // the contract here so the dispatch table below can assume a finite input).
    if !value.is_finite() {
        return "--".to_string();
    }

    // The dispatch table. We match on the (kind, unit) pair: the kind names
    // the *semantic* of the value (frequency, temperature, percent, ...), and
    // the unit names the *wire format*. A mismatched pair (e.g. CpuFrequency
    // with Unit::Bytes) is an adapter bug — we render "unknown" and log.
    //
    // Variants are written fully qualified (no `use MetricKind::*`) per the
    // workspace `clippy::enum_glob_use = "deny"` policy.
    match (*kind, *unit) {
        // --- Frequency (Hertz) ---
        (MetricKind::CpuFrequency | MetricKind::GpuFrequency, Unit::Hertz) => {
            let hz = clamp_u64(*value);
            if display.raw_values {
                format!("{hz} Hz")
            } else {
                format::format_hz(hz)
            }
        }
        // --- Temperature (Celsius / Fahrenheit / Kelvin) ---
        // Reading arrives in Celsius (canonical); config controls display unit.
        (
            MetricKind::CpuTemperature | MetricKind::GpuTemperature | MetricKind::DiskTemperature,
            Unit::Celsius | Unit::Fahrenheit | Unit::Kelvin,
        ) => {
            // The wire unit IS always Celsius at the trait boundary; if an
            // adapter emits Fahrenheit/Kelvin we treat the value as Celsius
            // anyway (defensive — same as Story 8.1's format_reading).
            format::format_temp(*value, display.temp_unit)
        }
        // --- Percent (utilization, battery, process) ---
        (
            MetricKind::CpuUtilization
            | MetricKind::GpuUtilization
            | MetricKind::GpuMemoryUtilization
            | MetricKind::BatteryPercent
            | MetricKind::ProcessCpuPercent
            | MetricKind::ProcessGpuPercent,
            Unit::Percent,
        ) => format::format_percent(*value),
        // --- Power (Watts) ---
        (
            MetricKind::CpuPower | MetricKind::GpuPower | MetricKind::BatteryPowerRate,
            Unit::Watts,
        ) => format::format_power(*value),
        // --- Voltage ---
        (MetricKind::Voltage, Unit::Volts) => format::format_voltage(*value),
        // --- Fan speed (RPM) ---
        (MetricKind::FanSpeed | MetricKind::GpuFanSpeed, Unit::Rpm) => {
            format::format_rpm(clamp_u32(*value))
        }
        // --- Byte counters / capacity (Bytes) ---
        (
            MetricKind::MemoryUsed
            | MetricKind::MemoryTotal
            | MetricKind::DiskUsed
            | MetricKind::DiskTotal
            | MetricKind::ProcessMemoryBytes
            | MetricKind::NetRxBytes
            | MetricKind::NetTxBytes
            | MetricKind::BandwidthRxBytes
            | MetricKind::BandwidthTxBytes,
            Unit::Bytes,
        ) => {
            let b = clamp_u64(*value);
            if display.raw_values {
                format!("{b} B")
            } else {
                format::format_bytes(b, base_from_config(display.decimal_base))
            }
        }
        // --- Disk throughput (Bytes/sec) ---
        (MetricKind::DiskReadBytesPerSec | MetricKind::DiskWriteBytesPerSec, Unit::BytesPerSec) => {
            let b = clamp_u64(*value);
            if display.raw_values {
                format!("{b} B/s")
            } else {
                format::format_bytes(b, base_from_config(display.decimal_base)) + "/s"
            }
        }
        // --- Network throughput (bits/sec) ---
        // Note: adapters currently emit byte counters (Bytes) for network;
        // BitsPerSec is reserved for the future formatted-throughput path.
        // We handle it defensively here for completeness.
        (_, Unit::BitsPerSec) => {
            let b = clamp_u64(*value);
            if display.raw_values {
                format!("{b} bps")
            } else {
                format::format_bps(b)
            }
        }
        // --- Counters (errors, packets) ---
        (
            MetricKind::NetRxErrors
            | MetricKind::NetTxErrors
            | MetricKind::NetRxPackets
            | MetricKind::NetTxPackets,
            Unit::Count | Unit::PacketsPerSec,
        ) => format_count(*value),
        // --- Battery state (value is the BatteryState ordinal — text form) ---
        // Display the enum name; the ordinal→enum mapping is in Story 1.1.
        (MetricKind::BatteryState, Unit::Count) => {
            BatteryState::from_value(*value).to_display_string()
        }
        // --- Uptime ---
        (MetricKind::UptimeSeconds, Unit::Seconds) => format_uptime(*value),
        // --- Anything else: unknown combination ---
        // Logged at warn so an adapter emitting mismatched kind × unit is
        // surfaced in CI/manual smoke (G15 — structured logs, no panic).
        (kind, unit) => {
            tracing::warn!(
                target = "sidebar.app.metric_row",
                kind = ?kind,
                unit = ?unit,
                "unknown MetricKind × Unit combination — rendering 'unknown'"
            );
            "unknown".to_string()
        }
    }
}

/// Map the `decimal_base` config flag to the Story 1.3 `Base` enum.
/// `decimal_base=true` → `Base::Decimal` (10⁹); `false` → `Base::Binary` (2³⁰).
/// Per T-28 the default is `Decimal` (the config default for `decimal_base`
/// is `true` in `DisplayConfig::default`).
#[must_use]
pub(crate) fn base_from_config(decimal_base: bool) -> Base {
    if decimal_base {
        Base::Decimal
    } else {
        Base::Binary
    }
}

/// Map the configured `TempUnit` to the Story 1.3 enum. The Reading's `unit`
/// field is ALWAYS Celsius at the trait boundary (canonical per architecture
/// §5.1); the DisplayConfig controls only the display-side conversion (T-29).
/// The config and format enums are the same type — this is an identity fn
/// kept for API symmetry with `base_from_config` and used by tests.
#[must_use]
#[allow(dead_code)] // Pure helper exercised by tests; kept for API symmetry with base_from_config.
pub(crate) fn temp_unit_from_config(unit: TempUnit) -> TempUnit {
    unit
}

/// Whether a `MetricKind × Unit` pair is recognized by the dispatch table.
/// Used by the Boundary #3 test path ("unknown MetricKind → 'unknown'").
#[must_use]
#[allow(dead_code)] // Pure helper exercised by tests; mirrors the dispatch table for assertions.
pub(crate) fn is_known_combination(kind: MetricKind, unit: Unit) -> bool {
    // A tiny pure-fn mirror of the dispatch match — returns true iff the
    // pair lands in a real formatter (not the catch-all "unknown" arm). We
    // re-dispatch via format_reading_with_config against a sentinel reading;
    // the dispatch above is the single source of truth, so we don't risk
    // drift between two match tables.
    let sentinel = Reading::gauge(
        sidebar_domain::reading::SensorId::new("probe", "test"),
        kind,
        1.0,
        unit,
    );
    let display = DisplayConfig {
        temp_unit: TempUnit::Celsius,
        raw_values: false,
        decimal_base: true,
        hide_from_capture: false,
        force_opaque: false,
    };
    format_reading_with_config(&sentinel, &display) != "unknown"
}

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Upper clamp bound for `clamp_u64` — a finite f64 just below `u64::MAX` that
/// fits in f64's 52-bit mantissa without precision loss. Anything larger is a
/// sentinel overflow, not real telemetry.
const U64_CLAMP_MAX: f64 = 9_007_199_254_740_992.0; // 2^53

/// Clamp a finite f64 to u64 for the format_hz/format_bytes/format_bps path.
/// Negatives clamp to 0; magnitudes past `U64_CLAMP_MAX` clamp to it. The cast
/// is safe because we've already verified finiteness in the caller and the
/// clamp bound is a 52-bit-clean value.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn clamp_u64(v: f64) -> u64 {
    if v < 0.0 {
        0
    } else {
        v.clamp(0.0, U64_CLAMP_MAX) as u64
    }
}

/// Clamp a finite f64 to u32 for the format_rpm path.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn clamp_u32(v: f64) -> u32 {
    if v < 0.0 {
        0
    } else {
        v.clamp(0.0, f64::from(u32::MAX)) as u32
    }
}

/// Upper clamp bound for `format_count` — `i64::MAX` rounded down into f64's
/// 52-bit mantissa. Counts above this are sentinel overflows.
const I64_CLAMP_MAX: f64 = 9_007_199_254_740_992.0; // 2^53

/// Format an integer count (packets, errors). No suffix. Negative finite
/// values clamp to 0 (defensive — adapters shouldn't emit negative counts).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_count(v: f64) -> String {
    if v < 0.0 {
        "0".to_string()
    } else {
        format!("{}", v.clamp(0.0, I64_CLAMP_MAX) as i64)
    }
}

/// Format an uptime in seconds as `Xh Ym` / `Ym Zs` / `Zs` (compact, no
/// trailing unit when zero). Keeps the metric row width-bounded for the
/// sidebar. Mirrors the Story 8.1 helper.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_uptime(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Local trait impl to render `BatteryState` as a display string without
/// pulling `format_battery` (which requires the percent u8 — we only have
/// the state ordinal here). Kept private to this module.
trait BatteryStateDisplay {
    fn to_display_string(self) -> String;
}

impl BatteryStateDisplay for BatteryState {
    fn to_display_string(self) -> String {
        match self {
            BatteryState::Charging => "Charging",
            BatteryState::Discharging => "Discharging",
            BatteryState::Idle => "Idle",
            BatteryState::Unknown => "Unknown",
        }
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.3 TDD contract tests (F8 egui_kittest + pure-fn unit tests).

    use super::*;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;
    use sidebar_domain::config::DisplayConfig;
    use sidebar_domain::format::TempUnit;
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};

    fn reading(kind: MetricKind, value: f64, unit: Unit) -> Reading {
        Reading::gauge(SensorId::new("cpu", "package"), kind, value, unit)
    }

    /// Default display config: human-readable, Celsius, decimal GB.
    fn default_display() -> DisplayConfig {
        DisplayConfig {
            temp_unit: TempUnit::Celsius,
            raw_values: false,
            decimal_base: true,
            hide_from_capture: false,
            force_opaque: false,
        }
    }

    /// Walk the kittest access tree and collect every node's text (same shape
    /// as the Story 8.1 + 8.2 helpers).
    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: CpuFrequency 3.84e9 → "3.84 GHz" =====

    #[test]
    fn cpu_frequency_renders_as_ghz() {
        let r = reading(MetricKind::CpuFrequency, 3_840_000_000.0, Unit::Hertz);
        assert_eq!(
            format_reading_with_config(&r, &default_display()),
            "3.84 GHz",
            "CpuFrequency 3.84e9 Hz must format as '3.84 GHz' (NFR-8 default)"
        );
    }

    #[test]
    fn cpu_frequency_renders_pill_in_harness() {
        let r = reading(MetricKind::CpuFrequency, 3_840_000_000.0, Unit::Hertz);
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &r, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("3.84 GHz"),
            "metric row must surface '3.84 GHz' as a queryable node (got: {labels})"
        );
    }

    // ===== Happy Path #2: raw_values=true → "3840000000 Hz" =====

    #[test]
    fn raw_values_emits_plain_hz() {
        let r = reading(MetricKind::CpuFrequency, 3_840_000_000.0, Unit::Hertz);
        let mut display = default_display();
        display.raw_values = true;
        assert_eq!(
            format_reading_with_config(&r, &display),
            "3840000000 Hz",
            "raw_values=true must emit the unscaled Hz count (T-28 raw mode)"
        );
    }

    // ===== Boundary #1: CpuTemperature + Fahrenheit → "144 °F" (T-29) =====

    #[test]
    fn cpu_temperature_fahrenheit_renders_144f() {
        // Reading arrives in Celsius (canonical); config requests Fahrenheit.
        let r = reading(MetricKind::CpuTemperature, 62.0, Unit::Celsius);
        let mut display = default_display();
        display.temp_unit = TempUnit::Fahrenheit;
        assert_eq!(
            format_reading_with_config(&r, &display),
            "144 °F",
            "CpuTemperature 62 °C with temp_unit=Fahrenheit must render '144 °F' (T-29)"
        );
    }

    // ===== Boundary #2: NaN → "--" (T-20) =====

    #[test]
    fn nan_reading_renders_dash_dash() {
        let r = reading(MetricKind::CpuTemperature, f64::NAN, Unit::Celsius);
        let formatted = format_reading_with_config(&r, &default_display());
        assert!(
            formatted.starts_with("--"),
            "NaN reading must render as '--' (T-20); got '{formatted}'"
        );
    }

    // ===== Boundary #3: unknown MetricKind → "unknown", logged =====

    #[test]
    fn unknown_kind_renders_unknown() {
        // Every MetricKind variant is known to the dispatch table at compile
        // time (exhaustive match), so this test exercises the Unit mismatch
        // path: a frequency kind paired with a Bytes unit is an unknown
        // combination → "unknown".
        let r = reading(MetricKind::CpuFrequency, 1.0, Unit::Bytes);
        assert_eq!(
            format_reading_with_config(&r, &default_display()),
            "unknown",
            "an unrecognized MetricKind × Unit combination must render 'unknown'"
        );
    }

    /// Sanity: base_from_config maps correctly.
    #[test]
    fn base_from_config_maps_correctly() {
        assert_eq!(base_from_config(true), Base::Decimal);
        assert_eq!(base_from_config(false), Base::Binary);
    }

    /// Sanity: temp_unit_from_config is the identity (config → format enum
    /// are the same type — T-29).
    #[test]
    fn temp_unit_from_config_is_identity() {
        assert_eq!(temp_unit_from_config(TempUnit::Celsius), TempUnit::Celsius);
        assert_eq!(
            temp_unit_from_config(TempUnit::Fahrenheit),
            TempUnit::Fahrenheit
        );
    }

    /// Sanity: is_known_combination agrees with the dispatch.
    #[test]
    fn is_known_combination_agrees_with_dispatch() {
        assert!(is_known_combination(MetricKind::CpuFrequency, Unit::Hertz));
        assert!(is_known_combination(
            MetricKind::CpuTemperature,
            Unit::Celsius
        ));
        assert!(!is_known_combination(MetricKind::CpuFrequency, Unit::Bytes));
    }

    /// Cross-check: format_percent for utilization.
    #[test]
    fn cpu_utilization_renders_percent() {
        let r = reading(MetricKind::CpuUtilization, 42.0, Unit::Percent);
        assert_eq!(format_reading_with_config(&r, &default_display()), "42%");
    }

    /// Cross-check: MemoryUsed decimal.
    #[test]
    fn memory_used_renders_tb() {
        let r = reading(MetricKind::MemoryUsed, 1_840_000_000_000.0, Unit::Bytes);
        assert_eq!(
            format_reading_with_config(&r, &default_display()),
            "1.84 TB"
        );
    }

    /// Cross-check: MemoryUsed binary.
    #[test]
    fn memory_used_binary_renders_tib() {
        let r = reading(MetricKind::MemoryUsed, 1_840_000_000_000.0, Unit::Bytes);
        let mut display = default_display();
        display.decimal_base = false;
        assert_eq!(
            format_reading_with_config(&r, &display),
            "1.67 TiB",
            "decimal_base=false must switch to Base::Binary (T-28)"
        );
    }

    /// Cross-check: raw byte counters.
    #[test]
    fn memory_used_raw_renders_plain_bytes() {
        let r = reading(MetricKind::MemoryUsed, 1_840_000_000_000.0, Unit::Bytes);
        let mut display = default_display();
        display.raw_values = true;
        assert_eq!(format_reading_with_config(&r, &display), "1840000000000 B");
    }
}
