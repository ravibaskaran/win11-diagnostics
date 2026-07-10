//! Story 8.8 — Threshold Alert UI (T-35, T-20).
//!
//! Drives metric-row color from the Story 1.2 [`check_threshold`] result:
//! Normal → default row color, Warning → accent, Critical → red (`#F44336`).
//! Blinking is OFF by default (calm UX per the Story 8.8 spec — it can be
//! added later behind a config flag without changing the [`color_for`] API).
//!
//! ## Hysteresis (Story 1.2 contract)
//!
//! The previous [`AlertState`] is fed back into [`classify`] so the
//! hysteresis band prevents flapping. The render path keeps one previous
//! state per sensor; for the test contract we expose a stateless
//! [`classify`] that accepts the prev state explicitly.
//!
//! ## Cited
//!
//! - Story 8.8 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - sidebar-domain::alert::{check_threshold, AlertState} (Story 1.2)
//! - sidebar-domain::config::ThresholdConfig (Story 1.5)
//! - nfr-thresholds.md T-20 (NaN handling), T-35 (theme + accent)

use eframe::egui::Color32;
use sidebar_domain::alert::check_threshold;
use sidebar_domain::alert::AlertState;
use sidebar_domain::config::ThresholdConfig;
use sidebar_domain::reading::MetricKind;
use sidebar_domain::reading::Reading;

/// Pick the row color for a reading given an optional threshold config + the
/// previous alert state (hysteresis).
///
/// - `reading` — the current [`Reading`]. NaN value → no alert, default
///   color (T-20). Non-temperature kinds → no alert (we only alert on
///   `CpuTemperature` and `GpuTemperature`).
/// - `threshold` — the `[thresholds]` config (None → no alerting).
/// - `prev_state` — the previous [`AlertState`] for this sensor (hysteresis).
/// - `accent` — the configured accent color (Warning fill).
/// - `default` — the default row text/fill color (Normal).
///
/// Returns `(color, new_state)` so the render path can stash the state for
/// the next frame's hysteresis input.
#[must_use]
pub fn color_for(
    reading: &Reading,
    threshold: Option<&ThresholdConfig>,
    prev_state: AlertState,
    accent: Color32,
    default: Color32,
) -> (Color32, AlertState) {
    let state = classify(reading, threshold, prev_state);
    let color = color_for_state(state, accent, default);
    (color, state)
}

/// Classify a reading's alert state via the Story 1.2 [`check_threshold`].
/// Maps the reading's `MetricKind` to the matching warn/crit thresholds in
/// the config (CPU temps → `cpu_temp_*`, GPU temps → `gpu_temp_*`); other
/// kinds return [`AlertState::Normal`] (we don't alert on non-temperature
/// metrics in v1).
#[must_use]
pub fn classify(
    reading: &Reading,
    threshold: Option<&ThresholdConfig>,
    prev_state: AlertState,
) -> AlertState {
    let Some(t) = threshold else {
        return AlertState::Normal;
    };
    // NaN reading → Normal (T-20). check_threshold handles NaN internally,
    // but we short-circuit here so non-temperature NaN readings also resolve
    // to Normal without needing a MetricKind mapping.
    if reading.value.is_nan() {
        return AlertState::Normal;
    }
    let (warn, crit) = match reading.kind {
        MetricKind::CpuTemperature => (t.cpu_temp_warn, t.cpu_temp_critical),
        MetricKind::GpuTemperature => (t.gpu_temp_warn, t.gpu_temp_critical),
        // v1 alerts only on CPU/GPU temperatures (Story 8.8 spec §2).
        _ => return AlertState::Normal,
    };
    check_threshold(reading.value, warn, HYSTERESIS_C, crit, prev_state)
}

/// Critical-alert red (PRD §3 — `#F44336`, Material red). Mirrors
/// [`crate::gui::theme::CRITICAL_RED`] so this module is self-contained for
/// the row-color mapping.
pub const CRITICAL_RED: Color32 = Color32::from_rgb(0xF4, 0x43, 0x36);

/// Documented hysteresis band (°C). The Story 1.2 contract makes this
/// configurable per-sensor in a future story; v1 uses a single 5°C band.
pub const HYSTERESIS_C: f64 = 5.0;

/// Map an [`AlertState`] to its row color.
#[must_use]
pub fn color_for_state(state: AlertState, accent: Color32, default: Color32) -> Color32 {
    match state {
        AlertState::Normal => default,
        AlertState::Warning => accent,
        AlertState::Critical => CRITICAL_RED,
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.8 TDD contract tests (pure-fn state classification).
    //!
    //! RED phase: `classify` always returns `Normal`, so the
    //! critical→red + warning→accent + hysteresis tests FAIL. The
    //! `None → default` and `NaN → default` tests pass trivially (Normal →
    //! default is the stub's behavior).
    //!
    //! We test the classification via `color_for` AND `classify` — the
    //! former drives the actual row color (the Story 8.8 user-visible
    //! contract), the latter pins the Story 1.2 logic hookup.

    use super::*;
    use sidebar_domain::config::ThresholdConfig;
    use sidebar_domain::reading::{MetricKind, Reading, SensorId, Unit};
    use std::time::Instant;

    const ACCENT: Color32 = Color32::from_rgb(0x4C, 0xAF, 0x50);
    const DEFAULT: Color32 = Color32::from_rgb(0xC0, 0xC0, 0xC0);

    fn cpu_temp(value: f64) -> Reading {
        Reading {
            sensor: SensorId::new("cpu", "package"),
            kind: MetricKind::CpuTemperature,
            value,
            unit: Unit::Celsius,
            timestamp: Instant::now(),
        }
    }

    fn thresholds(warn: f64, crit: f64) -> ThresholdConfig {
        ThresholdConfig {
            cpu_temp_warn: warn,
            cpu_temp_critical: crit,
            gpu_temp_warn: warn,
            gpu_temp_critical: crit,
        }
    }

    // ===== Happy Path #1: 95°C > crit 90°C → red =====

    #[test]
    fn reading_above_critical_is_red() {
        let r = cpu_temp(95.0);
        let t = thresholds(80.0, 90.0);
        let (color, state) = color_for(&r, Some(&t), AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(
            state,
            AlertState::Critical,
            "95°C > critical 90°C must classify as Critical"
        );
        assert_eq!(
            color, CRITICAL_RED,
            "Critical state must map to CRITICAL_RED (#F44336)"
        );
    }

    // ===== Happy Path #2: 60°C < warn 80°C → default =====

    #[test]
    fn reading_below_warning_is_default() {
        let r = cpu_temp(60.0);
        let t = thresholds(80.0, 90.0);
        let (color, state) = color_for(&r, Some(&t), AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(
            state,
            AlertState::Normal,
            "60°C < warn 80°C must classify as Normal"
        );
        assert_eq!(
            color, DEFAULT,
            "Normal state must map to the default row color"
        );
    }

    // ===== Happy Path #3: warn ≤ value < crit → accent =====

    #[test]
    fn reading_in_warning_band_is_accent() {
        let r = cpu_temp(85.0);
        let t = thresholds(80.0, 90.0);
        let (color, state) = color_for(&r, Some(&t), AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(state, AlertState::Warning);
        assert_eq!(color, ACCENT, "Warning state must map to the accent color");
    }

    // ===== Boundary #1: hysteresis prevents flapping (1.2 contract) =====
    //
    // Oscillation 88→92→88 with threshold 80/95, hysteresis 5 → color must
    // NOT flap back to default until value < 75 (warn - hysteresis).

    #[test]
    fn hysteresis_prevents_flapping_back_to_normal() {
        let t = thresholds(80.0, 95.0);
        // 1. value=85, prev=Normal → Warning.
        let (_, s1) = color_for(
            &cpu_temp(85.0),
            Some(&t),
            AlertState::Normal,
            ACCENT,
            DEFAULT,
        );
        assert_eq!(s1, AlertState::Warning, "85 > warn 80 → Warning");

        // 2. value=78 (below warn but within hysteresis band 75–80), prev=Warning → Warning.
        let (c2, s2) = color_for(&cpu_temp(78.0), Some(&t), s1, ACCENT, DEFAULT);
        assert_eq!(
            s2,
            AlertState::Warning,
            "78 is within hysteresis band → stays Warning"
        );
        assert_eq!(c2, ACCENT);

        // 3. value=74 (below warn - hysteresis = 75), prev=Warning → Normal.
        let (c3, s3) = color_for(&cpu_temp(74.0), Some(&t), s2, ACCENT, DEFAULT);
        assert_eq!(
            s3,
            AlertState::Normal,
            "74 < 75 (warn - hysteresis) → Normal"
        );
        assert_eq!(c3, DEFAULT);
    }

    // ===== Boundary #2: threshold None → no alerting, default color =====

    #[test]
    fn no_threshold_returns_default() {
        let r = cpu_temp(150.0); // would be critical if threshold were set.
        let (color, state) = color_for(&r, None, AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(
            state,
            AlertState::Normal,
            "threshold=None must not alert, regardless of value"
        );
        assert_eq!(color, DEFAULT);
    }

    // ===== Boundary #3: NaN reading → no alert, default color (T-20) =====

    #[test]
    fn nan_reading_returns_default() {
        let r = cpu_temp(f64::NAN);
        let t = thresholds(80.0, 90.0);
        let (color, state) = color_for(&r, Some(&t), AlertState::Critical, ACCENT, DEFAULT);
        assert_eq!(
            state,
            AlertState::Normal,
            "NaN reading must not alert (T-20) even with prev=Critical"
        );
        assert_eq!(color, DEFAULT);
    }

    // ===== Boundary #4: non-temperature kind → Normal =====

    #[test]
    fn non_temperature_kind_is_normal() {
        // CpuUtilization has no threshold mapping in v1.
        let r = Reading {
            sensor: SensorId::new("cpu", "package"),
            kind: MetricKind::CpuUtilization,
            value: 99.0,
            unit: Unit::Percent,
            timestamp: Instant::now(),
        };
        let t = thresholds(80.0, 90.0);
        let (color, state) = color_for(&r, Some(&t), AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(state, AlertState::Normal);
        assert_eq!(color, DEFAULT);
    }

    // ===== Boundary #5: GPU temp uses gpu_temp_* thresholds =====

    #[test]
    fn gpu_temperature_uses_gpu_thresholds() {
        let mut t = thresholds(80.0, 90.0);
        // Set GPU thresholds higher than CPU so we can distinguish.
        t.gpu_temp_warn = 95.0;
        t.gpu_temp_critical = 100.0;
        let r = Reading {
            sensor: SensorId::new("gpu", "nvidia"),
            kind: MetricKind::GpuTemperature,
            value: 92.0,
            unit: Unit::Celsius,
            timestamp: Instant::now(),
        };
        let (_, state) = color_for(&r, Some(&t), AlertState::Normal, ACCENT, DEFAULT);
        assert_eq!(
            state,
            AlertState::Normal,
            "GPU 92°C with gpu_warn=95 must be Normal (uses gpu thresholds, not cpu)"
        );
    }

    // ===== Pure-fn sanity: color_for_state =====

    #[test]
    fn color_for_state_maps_each_variant() {
        assert_eq!(
            color_for_state(AlertState::Normal, ACCENT, DEFAULT),
            DEFAULT
        );
        assert_eq!(
            color_for_state(AlertState::Warning, ACCENT, DEFAULT),
            ACCENT
        );
        assert_eq!(
            color_for_state(AlertState::Critical, ACCENT, DEFAULT),
            CRITICAL_RED
        );
    }

    /// Sanity: the hysteresis band is 5°C (documented v1 value).
    #[test]
    #[allow(clippy::float_cmp)]
    fn hysteresis_is_five_celsius() {
        assert_eq!(HYSTERESIS_C, 5.0);
    }

    /// Sanity: check_threshold is wired (Story 1.2 still passes through).
    #[test]
    fn check_threshold_wired_correctly() {
        // Direct call to the Story 1.2 fn to document the wiring.
        let s = check_threshold(95.0, 80.0, HYSTERESIS_C, 90.0, AlertState::Normal);
        assert_eq!(s, AlertState::Critical);
    }
}
