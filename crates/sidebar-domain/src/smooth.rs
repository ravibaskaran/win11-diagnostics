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
/// average. Non-finite samples (NaN, ±Inf) are dropped before smoothing;
/// if the filtered history is empty, returns `None`.
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
    // T-20 defense: drop non-finite samples (NaN, ±Inf). A single NaN
    // otherwise poisons the recursion forever (`alpha*v + (1-alpha)*NaN ==
    // NaN` for every subsequent step) — a transient bad sensor read would
    // brick the smoother until process restart. After filtering, if no
    // finite value remains, return None (matches the empty-history rule).
    // `alpha` itself is NOT filtered: a non-finite alpha is a programmer
    // bug; letting the output go non-finite surfaces it at the format layer
    // (which renders "--" per T-20) instead of silently masking it here.
    let mut smoothed: Option<f64> = None;
    for &v in history {
        if !v.is_finite() {
            continue;
        }
        smoothed = Some(match smoothed {
            None => v,
            Some(prev) => alpha * v + (1.0 - alpha) * prev,
        });
    }
    smoothed
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

    // ----- Boundary #2: non-finite samples MUST NOT corrupt the smoother (T-20).

    /// Cited: Story 1.2 Boundary #2, T-20. A single NaN sample MUST NOT
    /// permanently poison the smoother. The finite samples before and after
    /// the NaN must still produce a finite, sensible result.
    #[test]
    fn ewma_ignores_non_finite_samples() {
        // Currently fails: a NaN at index 1 propagates forever (smoothed =
        // alpha*v + (1-alpha)*NaN = NaN), so the result is Some(NaN).
        // Expected: filter the NaN out, compute ewma of [10.0, 10.0] with
        // alpha=0.5 → smoothed = 0.5*10 + 0.5*10 = 10.0.
        let r = ewma(&[10.0, f64::NAN, 10.0], 0.5);
        assert!(r.is_some(), "finite input must yield Some(_), got {r:?}");
        let r = r.unwrap();
        assert!(r.is_finite(), "NaN must not leak: got {r}");
        assert!(
            (r - 10.0).abs() < 1e-9,
            "finite sequence of 10s must yield ~10.0, got {r}"
        );
    }

    /// Cited: Story 1.2 Boundary #2, T-20. An all-NaN history MUST return
    /// None (no finite value to produce), never `Some(NaN)`.
    #[test]
    fn ewma_all_nan_history_returns_none() {
        // Currently fails: returns Some(NaN). Expected: None (the
        // post-filter slice is empty, matching the empty-history rule).
        let r = ewma(&[f64::NAN, f64::NAN], 0.5);
        assert!(r.is_none(), "all-NaN input must yield None, got {r:?}");
    }

    /// Cited: Story 1.2 Boundary #2, T-20. Infinity is also non-finite
    /// and must be filtered.
    #[test]
    fn ewma_ignores_infinity_samples() {
        let r = ewma(&[10.0, f64::INFINITY, f64::NEG_INFINITY, 10.0], 0.5);
        assert!(r.unwrap().is_finite());
    }
}
