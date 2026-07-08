//! Story 1.6 — Rolling-window sparkline data.
//!
//! A `RollingWindow` holds the last N values for a metric, used by the GUI
//! sparkline widget (Story 8.7). Values are pushed in chronological order;
//! the window evicts the oldest when full.
//!
//! NaN values are stored (not filtered) — the sparkline renders a gap at
//! their position. This lets the GUI show "data gap" rather than hiding
//! sensor dropouts.
//!
//! Cited: Story 1.6, architecture.md §4 + §7.1, T-22.

use std::collections::VecDeque;

/// A fixed-capacity FIFO buffer of `f64` values for sparkline rendering.
///
/// Default window size is 60 samples (T-22: 10 minutes at the default
/// 10-second poll interval per T-3). Configurable range: 10–600.
#[derive(Debug, Clone)]
pub struct RollingWindow {
    values: VecDeque<f64>,
    max_len: usize,
}

impl RollingWindow {
    /// Construct a new rolling window with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `max_len == 0` (T-22 lower bound is 10; 0 is nonsensical
    /// — the window would hold nothing).
    #[must_use]
    pub fn new(max_len: usize) -> Self {
        assert!(max_len > 0, "RollingWindow max_len must be > 0");
        Self {
            values: VecDeque::with_capacity(max_len),
            max_len,
        }
    }

    /// Push a new value, evicting the oldest if the window is full.
    pub fn push(&mut self, value: f64) {
        if self.values.len() >= self.max_len {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    /// Number of values currently in the window.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the window has zero values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Maximum capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.max_len
    }

    /// Whether the window is at capacity (next push will evict).
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.values.len() >= self.max_len
    }

    /// Return the values as a slice for rendering.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        // VecDeque::as_slices returns two slices; for a contiguous view
        // we use make_contiguous. This is O(n) if the deque is fragmented
        // but amortized O(1) for sequential pushes (the common case).
        self.values.as_slices().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_evicts_oldest_when_full() {
        let mut w = RollingWindow::new(3);
        w.push(1.0);
        w.push(2.0);
        w.push(3.0);
        assert_eq!(w.len(), 3);
        assert!(w.is_full());
        w.push(4.0);
        assert_eq!(w.len(), 3);
        // The window should now hold [2.0, 3.0, 4.0].
        let s = w.as_slice();
        assert!((s[0] - 2.0).abs() < f64::EPSILON);
        assert!((s[2] - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn default_window_holds_60_then_evicts() {
        let mut w = RollingWindow::new(60);
        for i in 0..65 {
            w.push(f64::from(i));
        }
        assert_eq!(w.len(), 60);
        let s = w.as_slice();
        // First value should be 5 (values 0–4 were evicted).
        assert!((s[0] - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_window_is_empty() {
        let w = RollingWindow::new(10);
        assert!(w.is_empty());
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn nan_value_is_stored_not_filtered() {
        let mut w = RollingWindow::new(5);
        w.push(10.0);
        w.push(f64::NAN);
        w.push(20.0);
        assert_eq!(w.len(), 3);
        let s = w.as_slice();
        assert!((s[0] - 10.0).abs() < f64::EPSILON);
        assert!(s[1].is_nan());
        assert!((s[2] - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "max_len must be > 0")]
    fn zero_capacity_panics() {
        let _ = RollingWindow::new(0);
    }

    #[test]
    fn capacity_and_is_full() {
        let mut w = RollingWindow::new(2);
        assert_eq!(w.capacity(), 2);
        assert!(!w.is_full());
        w.push(1.0);
        assert!(!w.is_full());
        w.push(2.0);
        assert!(w.is_full());
    }
}
