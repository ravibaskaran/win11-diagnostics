//! Story 1.3 — NFR-8 human-readable formatters.
//!
//! Pure functions that map `(value, unit)` → display string. Defaults are
//! human-readable per NFR-8: GHz for clock, GB/TB for storage, Mbps/Gbps
//! for network throughput, °C for temperature, etc. Raw-value display
//! (Hz/bytes/bps) is a UI toggle (Story 8.5 settings panel) that bypasses
//! these functions entirely — this module always produces the human form.
//!
//! All functions are pure (no IO, no global state), locale-stable in v1
//! (`.` decimal separator, no thousands separator per OQ-5).
//!
//! Cited:
//!   - architecture.md AD-13 (the design decision)
//!   - architecture.md §7.1 (the 10 exact-match test cases)
//!   - nfr-thresholds.md T-28 (decimal GB default), T-29 (°C default),
//!     T-30 (precision rules)
//!   - Story 1.3 TDD contract

use crate::reading::BatteryState;

// The u64→f64 casts in format_hz/format_bytes/format_bps are intentional:
// values are formatted to 3 sig figs, well within f64's 52-bit mantissa
// precision for any realistic telemetry magnitude. clippy's
// cast_precision_loss lint is allowed per-function below.

// ===========================================================================
// Enums
// ===========================================================================

/// Byte-count formatting base.
///
/// `Decimal` uses powers of 10 (GB = 10⁹ bytes) — matches disk
/// manufacturers, ISPs, and Windows Task Manager since Win10 1903.
/// `Binary` uses powers of 2 (GiB = 2³⁰) — the "technically correct"
/// convention. Per T-28 the default is `Decimal`; binary is a Settings
/// toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// Decimal: KB=10³, MB=10⁶, GB=10⁹, TB=10¹², PB=10¹⁵, EB=10¹⁸.
    Decimal,
    /// Binary: KiB=2¹⁰, MiB=2²⁰, GiB=2³⁰, TiB=2⁴⁰, PiB=2⁵⁰, EiB=2⁶⁰.
    Binary,
}

/// Temperature display unit. Per T-29 the default is `Celsius`; Fahrenheit
/// is a Settings toggle that affects every temp reading app-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempUnit {
    /// Celsius — `"62 °C"`.
    Celsius,
    /// Fahrenheit — `"144 °F"`.
    Fahrenheit,
}

// ===========================================================================
// Precision rules per T-30 (documented inline; no shared constant needed).
// ===========================================================================

// ===========================================================================
// format_hz — clock frequencies. 3 sig figs, K/M/G/T prefixes.
// ===========================================================================

/// Format a clock frequency in Hz as a human-readable string.
///
/// 3 significant figures (T-30). Examples per architecture §7.1:
/// - `format_hz(3_840_000_000) == "3.84 GHz"`
/// - `format_hz(0) == "0 Hz"` (not `"0 GHz"`)
/// - `format_hz(u64::MAX)` scales to THz without overflow.
#[must_use]
pub fn format_hz(hz: u64) -> String {
    if hz == 0 {
        return "0 Hz".to_string();
    }
    // f64 has 15-17 sig digits; u64::MAX (~1.8e19) fits without precision loss
    // for the 3-sig-fig formatting we do.
    #[allow(clippy::cast_precision_loss)]
    let v = hz as f64;
    format_scaled(
        v,
        1_000.0,
        &["Hz", "kHz", "MHz", "GHz", "THz", "PHz", "EHz"],
    )
}

// ===========================================================================
// format_bytes — storage + memory. Decimal or binary per `Base`.
// ===========================================================================

/// Format a byte count as a human-readable string.
///
/// 3 significant figures (T-30). Examples per architecture §7.1:
/// - `format_bytes(1_840_000_000_000, Base::Decimal) == "1.84 TB"`
/// - `format_bytes(1_840_000_000_000, Base::Binary) == "1.67 TiB"`
/// - `format_bytes(0, Base::Decimal) == "0 GB"`
/// - `format_bytes(u64::MAX, Base::Decimal)` scales to EB without overflow.
#[must_use]
pub fn format_bytes(bytes: u64, base: Base) -> String {
    if bytes == 0 {
        // T-30 + §7.1 Boundary: zero is "0 GB" (not "0 bytes" or "0 KB").
        return "0 GB".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let v = bytes as f64;
    match base {
        Base::Decimal => format_scaled(v, 1_000.0, &["B", "KB", "MB", "GB", "TB", "PB", "EB"]),
        Base::Binary => format_scaled(v, 1_024.0, &["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"]),
    }
}

// ===========================================================================
// format_bps — network throughput. 3 sig figs, K/M/G/T prefixes.
// ===========================================================================

/// Format a bits-per-second value as a human-readable string.
///
/// 3 significant figures (T-30). Per architecture §7.1:
/// `format_bps(48_200_000) == "48.2 Mbps"`.
#[must_use]
pub fn format_bps(bps: u64) -> String {
    if bps == 0 {
        return "0 bps".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let v = bps as f64;
    format_scaled(v, 1_000.0, &["bps", "kbps", "Mbps", "Gbps", "Tbps", "Pbps"])
}

// ===========================================================================
// format_temp — temperatures. °C or °F.
// ===========================================================================

/// Format a temperature. Input is always Celsius (the SI unit); output
/// unit is controlled by `unit`.
///
/// Per architecture §7.1:
/// - `format_temp(62.0, TempUnit::Celsius) == "62 °C"`
/// - `format_temp(62.0, TempUnit::Fahrenheit) == "144 °F"`
///
/// NaN/Inf render as `"-- °C"` per T-20 (defensive — adapters must not
/// emit NaN, but format must not panic if one slips through).
#[must_use]
pub fn format_temp(celsius: f64, unit: TempUnit) -> String {
    if !celsius.is_finite() {
        let unit_str = match unit {
            TempUnit::Celsius => "°C",
            TempUnit::Fahrenheit => "°F",
        };
        return format!("-- {unit_str}");
    }
    let (value, unit_str) = match unit {
        TempUnit::Celsius => (celsius, "°C"),
        // T-30: °F formula (c × 9/5) + 32.
        TempUnit::Fahrenheit => (celsius * 9.0 / 5.0 + 32.0, "°F"),
    };
    // Integer temperature display: "62 °C", "144 °F" — no decimals.
    let rounded = value.round();
    format!("{rounded:.0} {unit_str}")
}

// ===========================================================================
// format_voltage — 3 decimals.
// ===========================================================================

/// Format a voltage. 3 decimals (T-30). Per architecture §7.1:
/// `format_voltage(1.248) == "1.248 V"`. NaN → `"-- V"` (T-20).
#[must_use]
pub fn format_voltage(volts: f64) -> String {
    if !volts.is_finite() {
        return "-- V".to_string();
    }
    format!("{volts:.3} V")
}

// ===========================================================================
// format_rpm — integer RPM.
// ===========================================================================

/// Format a fan speed in RPM. Integer (T-30). Per architecture §7.1:
/// `format_rpm(1840) == "1840 RPM"`.
#[must_use]
pub fn format_rpm(rpm: u32) -> String {
    format!("{rpm} RPM")
}

// ===========================================================================
// format_power — 2 decimals.
// ===========================================================================

/// Format a power draw in Watts. 2 decimals (T-30). Per architecture §7.1:
/// `format_power(45.2) == "45.20 W"`. NaN → `"-- W"` (T-20).
#[must_use]
pub fn format_power(watts: f64) -> String {
    if !watts.is_finite() {
        return "-- W".to_string();
    }
    format!("{watts:.2} W")
}

// ===========================================================================
// format_percent — integer percent.
// ===========================================================================

/// Format a percentage. Integer (T-30). Per architecture §7.1:
/// `format_percent(42.0) == "42%"`. NaN → `"--%"` (T-20).
#[must_use]
pub fn format_percent(pct: f64) -> String {
    if !pct.is_finite() {
        return "--%".to_string();
    }
    let rounded = pct.round();
    format!("{rounded:.0}%")
}

// ===========================================================================
// format_battery — percent + state.
// ===========================================================================

/// Format a battery reading (percent + state). Per architecture §7.1:
/// `format_battery(78, BatteryState::Charging) == "78% (Charging)"`.
///
/// T-20 sentinel: pct=255 (u8::MAX, used as "unknown") renders as `"--"`.
#[must_use]
pub fn format_battery(pct: u8, state: BatteryState) -> String {
    let pct_str = if pct == u8::MAX {
        "--".to_string()
    } else {
        format!("{pct}%")
    };
    let state_str = match state {
        BatteryState::Charging => "Charging",
        BatteryState::Discharging => "Discharging",
        BatteryState::Idle => "Idle",
        BatteryState::Unknown => "Unknown",
    };
    format!("{pct_str} ({state_str})")
}

// ===========================================================================
// Internal: scaled-number formatter (3 sig figs + SI prefix).
// ===========================================================================

/// Format `value` with a 1000-based (or 1024-based) prefix chain.
///
/// Picks the largest prefix such that the value is ≥ 1.0 in that unit,
/// then formats to 3 significant figures. Returns the value in the
/// smallest unit if value < 1.0 of the next tier.
fn format_scaled(value: f64, base: f64, units: &[&str]) -> String {
    debug_assert!(!units.is_empty(), "units slice must be non-empty");
    if value <= 0.0 {
        // Defensive — callers handle the explicit zero case, but be safe.
        return format!("0 {}", units[0]);
    }
    let mut v = value;
    let mut idx = 0usize;
    while v >= base && idx + 1 < units.len() {
        v /= base;
        idx += 1;
    }
    // 3 significant figures: if v >= 100, no decimals; if v >= 10, 1 decimal;
    // else 2 decimals. This yields "3.84", "48.2", "184" — matching §7.1.
    let formatted = if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    format!("{formatted} {}", units[idx])
}

#[cfg(test)]
mod tests {
    //! Story 1.3 TDD contract tests.
    //!
    //! Cited: architecture.md §7.1 (10 exact-match cases) + Story 1.3
    //! Boundary cases (cite T-20, T-28, T-29, T-30).

    use super::*;

    // ----- Happy Path: all 10 exact-match cases from architecture §7.1 -----

    #[test]
    fn format_hz_ghz_3_sig_figs() {
        assert_eq!(format_hz(3_840_000_000), "3.84 GHz");
    }

    #[test]
    fn format_bytes_decimal_tb() {
        assert_eq!(format_bytes(1_840_000_000_000, Base::Decimal), "1.84 TB");
    }

    #[test]
    fn format_bytes_binary_tib() {
        assert_eq!(format_bytes(1_840_000_000_000, Base::Binary), "1.67 TiB");
    }

    #[test]
    fn format_bps_mbps() {
        assert_eq!(format_bps(48_200_000), "48.2 Mbps");
    }

    #[test]
    fn format_temp_celsius_integer() {
        assert_eq!(format_temp(62.0, TempUnit::Celsius), "62 °C");
    }

    #[test]
    fn format_temp_fahrenheit_integer() {
        assert_eq!(format_temp(62.0, TempUnit::Fahrenheit), "144 °F");
    }

    #[test]
    fn format_voltage_3_decimals() {
        assert_eq!(format_voltage(1.248), "1.248 V");
    }

    #[test]
    fn format_rpm_integer() {
        assert_eq!(format_rpm(1840), "1840 RPM");
    }

    #[test]
    fn format_power_2_decimals() {
        assert_eq!(format_power(45.2), "45.20 W");
    }

    #[test]
    fn format_percent_integer() {
        assert_eq!(format_percent(42.0), "42%");
    }

    // ----- Happy Path bonus: format_battery + decimal/binary toggle -----

    #[test]
    fn format_battery_charging() {
        assert_eq!(format_battery(78, BatteryState::Charging), "78% (Charging)");
    }

    #[test]
    fn bytes_decimal_vs_binary_correctly_ratioed() {
        // Same input, different base → correctly-ratioed outputs.
        let d = format_bytes(1_840_000_000_000, Base::Decimal);
        let b = format_bytes(1_840_000_000_000, Base::Binary);
        assert!(d.starts_with("1.84 TB"), "decimal: {d}");
        assert!(b.starts_with("1.67 TiB"), "binary: {b}");
    }

    // ----- Boundary #1: zero -----

    #[test]
    fn format_bytes_zero_is_zero_gb() {
        // T-20: no NaN, no negative. Zero is "0 GB" per §7.1.
        assert_eq!(format_bytes(0, Base::Decimal), "0 GB");
    }

    // ----- Boundary #2: u64::MAX scales to EB without overflow -----

    #[test]
    fn format_bytes_max_u64_scales_to_eb() {
        let s = format_bytes(u64::MAX, Base::Decimal);
        assert!(
            s.ends_with(" EB") || s.ends_with(" PB"),
            "u64::MAX must scale to EB or PB without overflow, got: {s}"
        );
        // Must be a parseable number (not "inf").
        let num: &str = s.split_whitespace().next().unwrap();
        let parsed: f64 = num.parse().expect("must parse as f64");
        assert!(parsed.is_finite(), "must be finite, got {parsed}");
    }

    // ----- Boundary #3: NaN/Inf render as "--" (T-20) -----

    #[test]
    fn format_temp_nan_is_dash() {
        assert_eq!(format_temp(f64::NAN, TempUnit::Celsius), "-- °C");
        assert_eq!(format_temp(f64::NAN, TempUnit::Fahrenheit), "-- °F");
        assert_eq!(format_temp(f64::INFINITY, TempUnit::Celsius), "-- °C");
    }

    #[test]
    fn format_voltage_nan_is_dash() {
        assert_eq!(format_voltage(f64::NAN), "-- V");
    }

    #[test]
    fn format_power_nan_is_dash() {
        assert_eq!(format_power(f64::NAN), "-- W");
    }

    #[test]
    fn format_percent_nan_is_dash() {
        assert_eq!(format_percent(f64::NAN), "--%");
    }

    // ----- Boundary #4: zero Hz is "0 Hz" (not "0 GHz") -----

    #[test]
    fn format_hz_zero_is_zero_hz() {
        assert_eq!(format_hz(0), "0 Hz");
    }

    // ----- Boundary #5: format_battery sentinel (255 = unknown) -----

    #[test]
    fn format_battery_255_is_dash() {
        assert_eq!(
            format_battery(u8::MAX, BatteryState::Unknown),
            "-- (Unknown)"
        );
    }

    // ----- Precision sanity (T-30) -----

    #[test]
    fn format_hz_picks_correct_prefix_at_each_tier() {
        assert_eq!(format_hz(500), "500 Hz");
        assert_eq!(format_hz(1_500), "1.50 kHz");
        assert_eq!(format_hz(1_500_000), "1.50 MHz");
        assert_eq!(format_hz(1_500_000_000), "1.50 GHz");
    }

    #[test]
    fn format_bps_picks_correct_prefix_at_each_tier() {
        assert_eq!(format_bps(500), "500 bps");
        assert_eq!(format_bps(1_500), "1.50 kbps");
        assert_eq!(format_bps(1_500_000), "1.50 Mbps");
        assert_eq!(format_bps(1_500_000_000), "1.50 Gbps");
    }

    #[test]
    fn format_temp_rounds_correctly() {
        assert_eq!(format_temp(62.4, TempUnit::Celsius), "62 °C");
        assert_eq!(format_temp(62.5, TempUnit::Celsius), "63 °C");
    }

    #[test]
    fn format_temp_negative_celsius() {
        // Sub-zero temperatures are valid; "-5 °C".
        assert_eq!(format_temp(-5.0, TempUnit::Celsius), "-5 °C");
    }

    #[test]
    fn format_percent_rounds_correctly() {
        assert_eq!(format_percent(42.4), "42%");
        assert_eq!(format_percent(42.5), "43%");
        assert_eq!(format_percent(0.0), "0%");
    }
}
