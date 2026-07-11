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

// ===========================================================================
// Story 12.6 — per-metric alert acknowledgement + snooze.
// ===========================================================================

/// Per-metric alert acknowledgement state. Story 12.6.
///
/// - `None` — no ack; the displayed state is the raw `check_threshold` result.
/// - `Acknowledged` — user dismissed the alert; the color is suppressed until
///   the value recovers below `warn - hysteresis` (re-arm).
/// - `Snoozed(until)` — user snoozed; the color is suppressed until `until`
///   (epoch seconds), then re-evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertAck {
    /// User acknowledged the alert; suppress until recovery (re-arm).
    Acknowledged,
    /// User snoozed until `until` (epoch seconds).
    Snoozed(i64),
}

/// Compute the displayed alert state after applying ack/snooze suppression.
///
/// `raw_state` is the `check_threshold` result. `ack` is the per-metric ack
/// (None if never acked). `now_epoch` is the wall-clock in seconds (so tests
/// can inject a fixed time). The returned `AlertState` is what the GUI
/// renders:
/// - If snoozed and `now < until` → `Normal` (suppressed).
/// - If acknowledged → `Normal` (suppressed; the ack clears via
///   `ack_should_clear` when the metric recovers).
/// - No ack → `raw_state` passthrough.
///
/// Cited: Story 12.6 DoD, alert.rs hysteresis contract.
#[must_use]
pub fn displayed_state(raw_state: AlertState, ack: Option<AlertAck>, now_epoch: i64) -> AlertState {
    let Some(ack) = ack else {
        return raw_state;
    };
    let suppressed = match ack {
        AlertAck::Snoozed(until) => now_epoch < until,
        AlertAck::Acknowledged => true, // suppress until ack_should_clear flips.
    };
    if suppressed {
        AlertState::Normal
    } else {
        raw_state
    }
}

/// Decide whether an ack should clear (re-arm) given the current raw state.
///
/// Returns `true` when the ack is stale (snooze expired, or acknowledged +
/// metric recovered) and should be dropped so the next breach re-alerts.
#[must_use]
pub fn ack_should_clear(raw_state: AlertState, ack: AlertAck, now_epoch: i64) -> bool {
    match ack {
        AlertAck::Snoozed(until) => now_epoch >= until,
        AlertAck::Acknowledged => raw_state == AlertState::Normal,
    }
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

    // ----- Story 12.6: alert ack/snooze -----

    #[test]
    fn snooze_suppresses_critical_until_expiry() {
        // Snoozed until epoch 1000; now is 500 → suppressed to Normal.
        let displayed = displayed_state(AlertState::Critical, Some(AlertAck::Snoozed(1000)), 500);
        assert_eq!(displayed, AlertState::Normal, "snooze suppresses");
        // After expiry (now=1500 > 1000) → raw state returns.
        let displayed = displayed_state(AlertState::Critical, Some(AlertAck::Snoozed(1000)), 1500);
        assert_eq!(displayed, AlertState::Critical, "snooze expiry re-alerts");
    }

    #[test]
    fn ack_suppresses_until_recovery() {
        // Acknowledged while Warning → suppressed.
        let displayed = displayed_state(AlertState::Warning, Some(AlertAck::Acknowledged), 0);
        assert_eq!(displayed, AlertState::Normal, "ack suppresses Warning");
        // Acknowledged while Critical → suppressed.
        let displayed = displayed_state(AlertState::Critical, Some(AlertAck::Acknowledged), 0);
        assert_eq!(displayed, AlertState::Normal, "ack suppresses Critical");
    }

    #[test]
    fn ack_clears_when_metric_recovers() {
        // Acked + still Warning → should NOT clear (stays suppressed).
        assert!(
            !ack_should_clear(AlertState::Warning, AlertAck::Acknowledged, 0),
            "ack stays while metric is still in alert"
        );
        // Acked + recovered to Normal → clears (re-arm).
        assert!(
            ack_should_clear(AlertState::Normal, AlertAck::Acknowledged, 0),
            "ack clears on recovery (re-arm)"
        );
    }

    #[test]
    fn snooze_clears_on_expiry() {
        // Before expiry → does not clear.
        assert!(
            !ack_should_clear(AlertState::Warning, AlertAck::Snoozed(1000), 500),
            "snooze does not clear before expiry"
        );
        // After expiry → clears.
        assert!(
            ack_should_clear(AlertState::Warning, AlertAck::Snoozed(1000), 1500),
            "snooze clears on expiry"
        );
    }

    #[test]
    fn no_ack_passes_raw_state_through() {
        assert_eq!(
            displayed_state(AlertState::Critical, None, 0),
            AlertState::Critical
        );
        assert_eq!(
            displayed_state(AlertState::Normal, None, 0),
            AlertState::Normal
        );
    }
}
