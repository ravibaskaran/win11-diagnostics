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

    /// Return the values as a contiguous slice for rendering.
    ///
    /// `&mut self` is required because `VecDeque::make_contiguous` rotates
    /// the ring buffer so the data lives in a single contiguous segment.
    /// After any `push` that evicts (`pop_front` + `push_back`), the deque
    /// is fragmented into two segments and `as_slices().0` alone returns
    /// only the front portion — see the regression in Story 1.6's own
    /// `push_evicts_oldest_when_full` test, fixed here.
    ///
    /// Amortized O(1) for the sequential-push pattern the sparkline uses;
    /// O(n) in the worst case when the deque is heavily fragmented.
    pub fn as_slice(&mut self) -> &[f64] {
        self.values.make_contiguous();
        self.values.as_slices().0
    }

    /// Immutable view of the current values as a Vec. Allocates; use for
    /// rendering where a contiguous borrow isn't possible (e.g. egui render).
    #[must_use]
    pub fn to_vec(&self) -> Vec<f64> {
        self.values.iter().copied().collect()
    }
}

// ===========================================================================
// Story 12.2 — per-metric history map.
// ===========================================================================

/// A key identifying a single metric stream for the per-metric history.
///
/// Stories 8.x render each `Reading` by `(category, instance, kind)`; this
/// key mirrors that triple so the history map aligns with the render path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricKey {
    /// Sensor category (e.g. `"cpu"`, `"gpu"`).
    pub category: String,
    /// Sensor instance (e.g. `"package"`, `"0"`).
    pub instance: String,
    /// Metric kind name (the `Debug` form of `MetricKind`, e.g.
    /// `"CpuUtilization"`).
    pub kind: String,
}

/// Story 12.2 — per-metric rolling-history map. Holds one `RollingWindow`
/// per `MetricKey`, pushing new values as readings arrive. The GUI reads
/// each metric's window to render a short history graph alongside the
/// sparkline (T-22: default 60 samples, configurable 10–600).
///
/// Pure logic — no IO, no GUI. The poller pushes; the GUI reads.
///
/// Cited: Story 12.2 DoD, architecture.md §7.1, nfr-thresholds.md T-22.
#[derive(Debug, Clone)]
pub struct MetricHistory {
    window_size: usize,
    windows: std::collections::HashMap<MetricKey, RollingWindow>,
}

impl MetricHistory {
    /// Construct with the T-22 window size (default 60; range 10–600).
    #[must_use]
    pub fn new(window_size: usize) -> Self {
        let clamped = window_size.clamp(10, 600);
        Self {
            window_size: clamped,
            windows: std::collections::HashMap::new(),
        }
    }

    /// Push a value for the given metric key. Creates the window lazily on
    /// first sighting.
    pub fn push(&mut self, key: MetricKey, value: f64) {
        self.windows
            .entry(key)
            .or_insert_with(|| RollingWindow::new(self.window_size))
            .push(value);
    }

    /// v1.0 audit 2 (P2) — drop any `MetricKey` NOT in `keep`. The caller
    /// (`replace_readings`) passes the set of keys derived from the current
    /// readings batch so transient sensors (hot-plug NIC, mounted ISO,
    /// reconnected Bluetooth) don't leave permanent stale `RollingWindow`s
    /// behind. Each window is ~500 B; over a long session this would be a
    /// slow unbounded memory leak on a tool meant to stay running for days.
    pub fn retain_recent(&mut self, keep: &std::collections::HashSet<MetricKey>) {
        self.windows.retain(|key, _| keep.contains(key));
    }

    /// Borrow the rolling window for `key`, if it exists.
    #[must_use]
    pub fn get(&self, key: &MetricKey) -> Option<&RollingWindow> {
        self.windows.get(key)
    }

    /// Borrow the rolling window for `key` mutably (for `as_slice`).
    pub fn get_mut(&mut self, key: &MetricKey) -> Option<&mut RollingWindow> {
        self.windows.get_mut(key)
    }

    /// v1.0 parity — flatten the first window whose MetricKey.kind matches
    /// the given kind name into a chronological Vec<f64>, for the line-graph
    /// popup. Empty if no window matches yet. The reference SidebarDiagnostics
    /// popup plots a single metric's session history; we do the same.
    #[must_use]
    pub fn snapshot_for_kind(&self, kind_name: &str) -> Vec<f64> {
        for (key, w) in &self.windows {
            if key.kind == kind_name {
                return w.to_vec();
            }
        }
        Vec::new()
    }

    /// The configured window size (post-clamp).
    #[must_use]
    pub fn window_size(&self) -> usize {
        self.window_size
    }

    /// Number of distinct metrics tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// True when no metrics have been pushed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
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

    // ----- Story 12.2: MetricHistory -----

    fn key(category: &str, instance: &str, kind: &str) -> MetricKey {
        MetricKey {
            category: category.to_string(),
            instance: instance.to_string(),
            kind: kind.to_string(),
        }
    }

    #[test]
    fn metric_history_push_creates_and_evicts_per_metric() {
        // T-22 minimum window is 10; push 11 values to prove eviction.
        let mut h = MetricHistory::new(10);
        let cpu = key("cpu", "package", "CpuUtilization");
        for i in 0..11_i32 {
            h.push(cpu.clone(), f64::from(i));
        }
        let w = h.get(&cpu).expect("CPU window exists");
        assert_eq!(w.len(), 10, "window evicts to capacity (T-22 min)");
    }

    #[test]
    fn metric_history_tracks_distinct_metrics_independently() {
        let mut h = MetricHistory::new(60);
        let cpu = key("cpu", "package", "CpuUtilization");
        let gpu = key("gpu", "0", "GpuUtilization");
        h.push(cpu.clone(), 42.0);
        h.push(gpu.clone(), 65.0);
        assert_eq!(h.len(), 2, "two distinct metrics tracked");
        // Pushing to one doesn't affect the other.
        h.push(cpu.clone(), 43.0);
        assert_eq!(
            h.get(&gpu).map(RollingWindow::len),
            Some(1),
            "GPU window unaffected by CPU push"
        );
        assert_eq!(
            h.get(&cpu).map(RollingWindow::len),
            Some(2),
            "CPU window grew"
        );
    }

    #[test]
    fn metric_history_clamps_window_size_to_t22_range() {
        let h_small = MetricHistory::new(0);
        assert_eq!(h_small.window_size(), 10, "min window is 10 (T-22)");
        let h_big = MetricHistory::new(9999);
        assert_eq!(h_big.window_size(), 600, "max window is 600 (T-22)");
    }

    /// v1.0 parity — snapshot_for_kind returns the first matching window's
    /// values for the graph popup. Empty when no window matches yet.
    #[test]
    fn snapshot_for_kind_returns_matching_window_values() {
        let mut h = MetricHistory::new(60);
        let cpu = MetricKey {
            category: "cpu".into(),
            instance: "cpu/0".into(),
            kind: "CpuTemperature".into(),
        };
        let gpu = MetricKey {
            category: "gpu".into(),
            instance: "gpu/0".into(),
            kind: "GpuTemperature".into(),
        };
        h.push(cpu.clone(), 50.0);
        h.push(cpu.clone(), 51.0);
        h.push(cpu, 52.0);
        h.push(gpu, 60.0);
        let snap = h.snapshot_for_kind("CpuTemperature");
        assert_eq!(snap, vec![50.0, 51.0, 52.0]);
        // Non-matching kind → empty.
        assert!(h.snapshot_for_kind("RamClock").is_empty());
        // GPU window is found when asked.
        assert_eq!(h.snapshot_for_kind("GpuTemperature"), vec![60.0]);
    }

    /// v1.0 parity — snapshot_for_kind on an empty history is empty (the
    /// popup shows "Waiting for samples…" rather than crashing).
    #[test]
    fn snapshot_for_kind_on_empty_history_is_empty() {
        let h = MetricHistory::new(60);
        assert!(h.snapshot_for_kind("CpuTemperature").is_empty());
    }

    // ===== v1.0 audit 2 (P2) — retain_recent evicts transient sensors =====

    /// Cited: v1.0 audit Iteration 2. A sensor that disappears from the
    /// readings batch (hot-plug NIC unplugged, USB drive unmounted) MUST
    /// have its history window evicted so the map stays bounded.
    #[test]
    fn retain_recent_evicts_absent_keys() {
        let mut h = MetricHistory::new(60);
        let cpu = MetricKey {
            category: "cpu".into(),
            instance: "package".into(),
            kind: "CpuTemperature".into(),
        };
        let stale_nic = MetricKey {
            category: "net".into(),
            instance: "999".into(),
            kind: "NetRxBytes".into(),
        };
        h.push(cpu.clone(), 50.0);
        h.push(stale_nic.clone(), 1000.0);
        assert_eq!(h.len(), 2, "both windows seeded");

        // New batch keeps CPU, drops the unplugged NIC.
        let keep = std::collections::HashSet::from([cpu.clone()]);
        h.retain_recent(&keep);
        assert_eq!(h.len(), 1, "stale NIC window evicted");
        assert!(h.get(&cpu).is_some(), "kept CPU window survives");
        assert!(h.get(&stale_nic).is_none(), "stale NIC window gone");
    }

    /// Cited: v1.0 audit Iteration 2. An empty keep-set evicts everything.
    #[test]
    fn retain_recent_with_empty_keep_clears_all() {
        let mut h = MetricHistory::new(60);
        h.push(
            MetricKey {
                category: "cpu".into(),
                instance: "package".into(),
                kind: "CpuTemperature".into(),
            },
            50.0,
        );
        let keep = std::collections::HashSet::new();
        h.retain_recent(&keep);
        assert!(h.is_empty(), "empty keep-set must evict all windows");
    }

    /// Cited: v1.0 audit Iteration 2. A keep-set that covers everything
    /// preserves all windows (no spurious eviction).
    #[test]
    fn retain_recent_with_full_keep_preserves_all() {
        let mut h = MetricHistory::new(60);
        let cpu = MetricKey {
            category: "cpu".into(),
            instance: "package".into(),
            kind: "CpuTemperature".into(),
        };
        let gpu = MetricKey {
            category: "gpu".into(),
            instance: "package".into(),
            kind: "GpuTemperature".into(),
        };
        h.push(cpu.clone(), 50.0);
        h.push(gpu.clone(), 60.0);
        let keep = std::collections::HashSet::from([cpu, gpu]);
        h.retain_recent(&keep);
        assert_eq!(h.len(), 2, "full keep-set preserves both windows");
    }
}
