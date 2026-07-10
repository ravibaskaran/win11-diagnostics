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
//! - Unknown `MetricKind` (defensive — should not happen at runtime since
//!   `MetricKind` is exhaustive) → `"unknown"`, logged at `warn!`.
//!
//! ## Cited
//!
//! - Story 8.3 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - architecture.md AD-13 + §4 (`metric_row.rs`) + §7.1 (format match table)
//! - nfr-thresholds.md T-20 (finite), T-28 (decimal/binary), T-29 (temp unit),
//!   T-30 (precision rules)
//! - sidebar-domain::format (Story 1.3) + sidebar-domain::config (Story 1.5)
//!
//! ## RED phase
//!
//! `render` and `format_reading_with_config` are STUBS — tests below encode
//! the Story 8.3 contract and are expected to FAIL at this commit.

use eframe::egui::Ui;
use sidebar_domain::config::DisplayConfig;
use sidebar_domain::format::{Base, TempUnit};
use sidebar_domain::reading::{MetricKind, Reading, Unit};

/// Render one metric row: a short kind label + a formatted value, using the
/// given display config to pick the formatter (Story 8.3 NFR-8 dispatch).
///
/// STUB — renders nothing in RED phase.
pub fn render(_ui: &mut Ui, _reading: &Reading, _display: &DisplayConfig) {
    // Intentionally empty: RED phase stub.
}

/// Format a reading's value per the MetricKind × Unit dispatch table, honoring
/// the DisplayConfig toggles. Returns `"--"` for NaN (T-20) and `"unknown"`
/// for unrecognized MetricKind × Unit combinations (logged at `warn!`).
///
/// STUB — returns `"--"` always in RED phase.
///
/// RED phase: unused in the lib build until GREEN wires it into `render` +
/// `render_snapshot`.
#[allow(dead_code)]
#[must_use]
pub(crate) fn format_reading_with_config(_reading: &Reading, _display: &DisplayConfig) -> String {
    // RED stub: always returns the sentinel so the Happy Path tests fail.
    "--".to_string()
}

/// Map the `decimal_base` config flag to the Story 1.3 `Base` enum.
/// `decimal_base=true` → `Base::Decimal` (10⁹); `false` → `Base::Binary` (2³⁰).
/// Per T-28 the default is `Decimal` (the config default for `decimal_base`
/// is `true` in `DisplayConfig::default`).
///
/// RED phase: unused in the lib build until GREEN.
#[allow(dead_code)]
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
///
/// RED phase: unused in the lib build until GREEN.
#[allow(dead_code)]
#[must_use]
pub(crate) fn temp_unit_from_config(unit: sidebar_domain::format::TempUnit) -> TempUnit {
    unit
}

/// Whether a `MetricKind × Unit` pair is recognized by the dispatch table.
/// Used by the Boundary #3 test ("unknown MetricKind → 'unknown'").
///
/// STUB — returns `false` always in RED so the "known kind" path is
/// unreachable (forcing the "3.84 GHz" test to fail).
///
/// RED phase: unused in the lib build until GREEN.
#[allow(dead_code)]
#[must_use]
pub(crate) fn is_known_combination(_kind: MetricKind, _unit: Unit) -> bool {
    false
}

#[cfg(test)]
mod tests {
    //! Story 8.3 TDD contract tests (F8 egui_kittest + pure-fn unit tests).
    //!
    //! RED phase: the formatting tests are expected to FAIL —
    //! `format_reading_with_config` always returns `"--"`.

    use super::*;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;
    use sidebar_domain::config::DisplayConfig;
    use sidebar_domain::format::TempUnit;
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};
    use std::time::Instant;

    fn reading(kind: MetricKind, value: f64, unit: Unit) -> Reading {
        Reading {
            sensor: SensorId::new("cpu", "package"),
            kind,
            value,
            unit,
            timestamp: Instant::now(),
        }
    }

    /// Default display config: human-readable, Celsius, decimal GB.
    fn default_display() -> DisplayConfig {
        DisplayConfig {
            temp_unit: TempUnit::Celsius,
            raw_values: false,
            decimal_base: true,
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

    /// Sanity: base_from_config maps correctly. (Passes in RED — locks the
    /// helper.)
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
}
