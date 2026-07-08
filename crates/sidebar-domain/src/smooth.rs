//! Story 1.2 — EWMA (Exponentially Weighted Moving Average) smoother.
//!
//! Pure function that calms jittery telemetry before display. The GUI
//! (Story 8.x) maintains a small history per metric and calls `ewma` each
//! tick to get a smoothed value.
//!
//! Cited: Story 1.2, architecture.md §7.1.

/// Compute the EWMA of a history slice with smoothing factor `alpha`.
///
/// `alpha` controls the weight of recent vs. older values: 0.0 = fully
/// old-dominated (output = first value), 1.0 = fully recent (output =
/// last value). The conventional range is `0.0 < alpha < 1.0`.
///
/// Returns `None` for an empty history (per T-20 — no NaN). Returns
/// `Some(v)` for non-empty input where `v` is the recursively-weighted
/// average.
///
/// # Examples
///
/// ```
/// use sidebar_domain::smooth::ewma;
/// // Constant input converges to that constant.
/// let r = ewma(&[10.0, 10.0, 10.0], 0.5).unwrap();
/// assert!((r - 10.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn ewma(history: &[f64], alpha: f64) -> Option<f64> {
    if history.is_empty() {
        return None;
    }
    // Standard recursive EWMA: start with the first value, then
    //   smoothed[i] = alpha * history[i] + (1 - alpha) * smoothed[i-1]
    let mut smoothed = history[0];
    for &v in &history[1..] {
        smoothed = alpha * v + (1.0 - alpha) * smoothed;
    }
    Some(smoothed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 1.2 Happy Path #1: constant input converges.
    #[test]
    fn ewma_constant_converges() {
        let r = ewma(&[10.0, 10.0, 10.0], 0.5);
        assert!(r.is_some());
        assert!((r.unwrap() - 10.0).abs() < 1e-9);
    }

    /// Story 1.2 Boundary #1: empty history → None (T-20).
    #[test]
    fn ewma_empty_is_none() {
        assert!(ewma(&[], 0.5).is_none());
    }

    #[test]
    fn ewma_single_element_returns_it() {
        let r = ewma(&[42.0], 0.5).unwrap();
        assert!((r - 42.0).abs() < 1e-9);
    }

    #[test]
    fn ewma_alpha_one_is_last_value() {
        let r = ewma(&[10.0, 20.0, 30.0], 1.0).unwrap();
        assert!((r - 30.0).abs() < 1e-9);
    }

    #[test]
    fn ewma_alpha_zero_is_first_value() {
        let r = ewma(&[10.0, 20.0, 30.0], 0.0).unwrap();
        assert!((r - 10.0).abs() < 1e-9);
    }

    #[test]
    fn ewma_weights_recent_more() {
        // With alpha=0.5, recent values matter more. A rising sequence
        // should produce a value between the first and last, biased
        // toward the last.
        let r = ewma(&[0.0, 0.0, 0.0, 100.0], 0.5).unwrap();
        assert!(r > 0.0 && r < 100.0);
        // The last value (100) has weight alpha=0.5, so the result is at
        // least 50.0.
        assert!(r >= 50.0, "ewma should be >= 50.0, got {r}");
    }
}
