//! Story 1.2 — Threshold alert with hysteresis.
//!
//! Pure function that classifies a value against warning/critical
//! thresholds with hysteresis to prevent flapping when the value
//! oscillates near a threshold boundary.
//!
//! Cited: Story 1.2, architecture.md §7.1.

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertState {
    /// Value is below the warning threshold (accounting for hysteresis).
    Normal,
    /// Value is at or above the warning threshold but below critical.
    Warning,
    /// Value is at or above the critical threshold.
    Critical,
}

/// Classify a value against warning and critical thresholds with hysteresis.
///
/// `value` is the current reading. `warn_threshold` and `crit_threshold`
/// are the levels at which the alert transitions UP (Normal→Warning,
/// Warning→Critical). `hysteresis` is the band below each threshold that
/// the value must drop below before transitioning DOWN (Warning→Normal,
/// Critical→Warning) — this prevents flapping when the value oscillates
/// near a boundary.
///
/// # Hysteresis contract
///
/// - UP transition (Normal→Warning): value ≥ warn_threshold.
/// - DOWN transition (Warning→Normal): value < warn_threshold − hysteresis.
/// - UP transition (Warning→Critical): value ≥ crit_threshold.
/// - DOWN transition (Critical→Warning): value < crit_threshold − hysteresis.
///
/// # NaN handling (T-20, G15)
///
/// `value = NaN` returns `Normal` (graceful — no panic). Adapters must not
/// emit NaN, but this function is defensive.
///
/// # Examples
///
/// ```
/// use sidebar_domain::alert::{check_threshold, AlertState};
/// let s = check_threshold(95.0, 90.0, 5.0, 100.0, AlertState::Normal);
/// assert_eq!(s, AlertState::Warning);
/// ```
#[must_use]
pub fn check_threshold(
    value: f64,
    warn_threshold: f64,
    hysteresis: f64,
    crit_threshold: f64,
    prev_state: AlertState,
) -> AlertState {
    // T-20 / G15: NaN value → Normal (no panic). INFINITY is a valid
    // extreme reading and flows through the normal threshold logic.
    if value.is_nan() {
        return AlertState::Normal;
    }

    // Check critical first (highest priority).
    let crit_down = crit_threshold - hysteresis;
    let already_critical = matches!(prev_state, AlertState::Critical);
    if value >= crit_threshold || (already_critical && value >= crit_down) {
        return AlertState::Critical;
    }

    // Check warning.
    let warn_down = warn_threshold - hysteresis;
    let already_warning = matches!(prev_state, AlertState::Warning | AlertState::Critical);
    if value >= warn_threshold {
        return AlertState::Warning;
    }
    if already_warning && value >= warn_down {
        // Still in the hysteresis band — don't flap back to Normal.
        return AlertState::Warning;
    }

    AlertState::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 1.2 Happy Path #2: value above warn threshold → Warning.
    #[test]
    fn check_threshold_above_warn_is_warning() {
        let s = check_threshold(95.0, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s, AlertState::Warning);
    }

    /// Story 1.2 Happy Path: value above critical → Critical.
    #[test]
    fn check_threshold_above_crit_is_critical() {
        let s = check_threshold(105.0, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s, AlertState::Critical);
    }

    /// Story 1.2 Boundary #1: NaN value → Normal (T-20, G15).
    #[test]
    fn check_threshold_nan_is_normal() {
        let s = check_threshold(f64::NAN, 90.0, 5.0, 100.0, AlertState::Warning);
        assert_eq!(s, AlertState::Normal);
    }

    /// Story 1.2 Boundary #4: INFINITY → Critical (mathematically sensible).
    #[test]
    fn check_threshold_infinity_is_critical() {
        let s = check_threshold(f64::INFINITY, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s, AlertState::Critical);
    }

    /// Story 1.2 Boundary #2: hysteresis prevents flapping.
    /// Oscillation 88→92→88 with threshold 90, hysteresis 5 MUST NOT
    /// return to Normal until value < 85.
    #[test]
    fn hysteresis_prevents_flapping() {
        // Start at Normal, value 92 → Warning.
        let s1 = check_threshold(92.0, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s1, AlertState::Warning);

        // Now Warning, value drops to 88 (below threshold but within
        // hysteresis band 85–90) → MUST stay Warning.
        let s2 = check_threshold(88.0, 90.0, 5.0, 100.0, AlertState::Warning);
        assert_eq!(s2, AlertState::Warning);

        // Value drops to 84 (below warn_threshold - hysteresis = 85) → Normal.
        let s3 = check_threshold(84.0, 90.0, 5.0, 100.0, AlertState::Warning);
        assert_eq!(s3, AlertState::Normal);
    }

    #[test]
    fn value_below_warn_is_normal() {
        let s = check_threshold(80.0, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s, AlertState::Normal);
    }

    #[test]
    fn negative_hysteresis_band_still_works() {
        // Edge: hysteresis = 0 means no band; transitions are immediate.
        let s = check_threshold(90.0, 90.0, 0.0, 100.0, AlertState::Normal);
        assert_eq!(s, AlertState::Warning);
    }

    #[test]
    fn critical_hysteresis_prevents_flapping() {
        // Value 100 → Critical. Drop to 98 (within crit hysteresis 95-100)
        // → stays Critical. Drop to 94 → Warning.
        let s1 = check_threshold(100.0, 90.0, 5.0, 100.0, AlertState::Normal);
        assert_eq!(s1, AlertState::Critical);

        let s2 = check_threshold(98.0, 90.0, 5.0, 100.0, AlertState::Critical);
        assert_eq!(s2, AlertState::Critical);

        let s3 = check_threshold(94.0, 90.0, 5.0, 100.0, AlertState::Critical);
        assert_eq!(s3, AlertState::Warning);
    }
}
