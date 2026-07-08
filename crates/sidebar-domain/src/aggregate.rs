//! Story 1.6 — Top-N aggregation for process lists.
//!
//! Pure function that selects the top-N readings of a given kind by value
//! (descending). Used by the GUI's "top 5 by CPU" / "top 5 by RAM" panels
//! (Story 8.x). Ties are broken by insertion order (stable sort).
//!
//! Cited: Story 1.6, architecture.md §4 + §7.1, T-21.

use crate::reading::{MetricKind, Reading};

/// Select the top-N readings of `kind` sorted by value descending.
///
/// Returns references into the input slice (no allocation beyond the result
/// Vec). Ties are broken by insertion order — if two readings have the same
/// value, the one that appeared first in the input comes first.
///
/// # Edge cases
///
/// - `readings` empty → empty result.
/// - `n == 0` → empty result.
/// - `n > count of matching kind` → all matching readings (fewer than N).
///
/// # NaN handling (T-20)
///
/// NaN-valued readings are treated as the smallest possible value (they
/// sort to the end). This ensures NaN doesn't pollute the top-N.
#[must_use]
pub fn top_n(readings: &[Reading], kind: MetricKind, n: usize) -> Vec<&Reading> {
    if n == 0 {
        return Vec::new();
    }
    // Collect (original_index, reference) pairs for matching kind.
    let mut filtered: Vec<(usize, &Reading)> = readings
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == kind)
        .collect();
    if filtered.is_empty() {
        return Vec::new();
    }
    // Stable sort by value descending; NaN sorts last (treated as -inf).
    // Use `sort_by` which is stable; ties preserve original index order.
    filtered.sort_by(|a, b| {
        let va = if a.1.value.is_nan() {
            f64::NEG_INFINITY
        } else {
            a.1.value
        };
        let vb = if b.1.value.is_nan() {
            f64::NEG_INFINITY
        } else {
            b.1.value
        };
        vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered.truncate(n);
    filtered.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reading::{SensorId, Unit};

    fn make_reading(kind: MetricKind, value: f64, instance: &str) -> Reading {
        Reading::new(
            SensorId::new("process", instance),
            kind,
            value,
            Unit::Percent,
        )
    }

    #[test]
    fn top_n_returns_descending() {
        let readings = vec![
            make_reading(MetricKind::ProcessCpuPercent, 30.0, "a"),
            make_reading(MetricKind::ProcessCpuPercent, 10.0, "b"),
            make_reading(MetricKind::ProcessCpuPercent, 50.0, "c"),
            make_reading(MetricKind::ProcessCpuPercent, 20.0, "d"),
            make_reading(MetricKind::ProcessCpuPercent, 40.0, "e"),
        ];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 3);
        assert_eq!(top.len(), 3);
        assert!((top[0].value - 50.0).abs() < f64::EPSILON);
        assert!((top[1].value - 40.0).abs() < f64::EPSILON);
        assert!((top[2].value - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn top_n_empty_input_returns_empty() {
        let top = top_n(&[], MetricKind::ProcessCpuPercent, 3);
        assert!(top.is_empty());
    }

    #[test]
    fn top_n_zero_n_returns_empty() {
        let readings = vec![make_reading(MetricKind::ProcessCpuPercent, 50.0, "a")];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 0);
        assert!(top.is_empty());
    }

    #[test]
    fn top_n_n_greater_than_count_returns_all() {
        let readings = vec![make_reading(MetricKind::ProcessCpuPercent, 50.0, "a")];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 10);
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn top_n_ties_preserve_insertion_order() {
        let readings = vec![
            make_reading(MetricKind::ProcessCpuPercent, 50.0, "first"),
            make_reading(MetricKind::ProcessCpuPercent, 50.0, "second"),
            make_reading(MetricKind::ProcessCpuPercent, 50.0, "third"),
        ];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].sensor.instance, "first");
        assert_eq!(top[1].sensor.instance, "second");
    }

    #[test]
    fn top_n_filters_by_kind() {
        let readings = vec![
            make_reading(MetricKind::ProcessCpuPercent, 90.0, "cpu"),
            make_reading(MetricKind::ProcessMemoryBytes, 999.0, "mem"),
            make_reading(MetricKind::ProcessCpuPercent, 50.0, "cpu2"),
        ];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 5);
        assert_eq!(top.len(), 2); // only the two CPU readings, not the memory one
    }

    #[test]
    fn top_n_nan_sorts_last() {
        let readings = vec![
            make_reading(MetricKind::ProcessCpuPercent, f64::NAN, "nan"),
            make_reading(MetricKind::ProcessCpuPercent, 10.0, "ten"),
        ];
        let top = top_n(&readings, MetricKind::ProcessCpuPercent, 2);
        assert_eq!(top[0].sensor.instance, "ten");
        assert_eq!(top[1].sensor.instance, "nan");
    }
}
