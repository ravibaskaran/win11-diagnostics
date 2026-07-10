//! Story 8.7 — Sparkline Widget (T-22).
//!
//! A custom egui painter that renders a [`RollingWindow`] (Story 1.6) as a
//! mini line chart. NaN values render as gaps (the line breaks, no segment
//! drawn across a NaN position) per the Story 1.6 "store NaN, render gap"
//! contract.
//!
//! ## Empty window
//!
//! An empty window renders the [`EMPTY_TEXT`] placeholder (`"—"`) — the F8
//! access tree surfaces it as a queryable label.
//!
//! ## Overflow (window wider than widget width)
//!
//! If the window holds more samples than the widget has horizontal pixels,
//! we render one vertex per pixel via simple stride downsampling (LTTB-style
//! bucketing is overkill for a 60-sample sparkline; we pick every Nth value
//! where N = samples / pixels). This is documented in
//! [`render_segments`] and the `overflow_downsamples_to_pixel_width` test.
//!
//! ## Cited
//!
//! - Story 8.7 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - nfr-thresholds.md T-22 (rolling window), T-20 (NaN handling)
//! - sidebar-domain::graph::RollingWindow (Story 1.6)

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2};
use sidebar_domain::graph::RollingWindow;

/// Empty-state placeholder rendered when the window holds no samples.
pub const EMPTY_TEXT: &str = "—";

/// Default sparkline width in pixels (the sidebar viewport is 280 wide; a
/// 100px sparkline fits a metric row without crowding the value).
pub const DEFAULT_WIDTH: f32 = 100.0;

/// Default sparkline height in pixels.
pub const DEFAULT_HEIGHT: f32 = 24.0;

/// Stroke color — a muted accent that reads as a graph trace.
const STROKE: Color32 = Color32::from_rgb(0x80, 0xC0, 0xFF);

/// Stroke width in pixels.
const STROKE_WIDTH: f32 = 1.5;

/// Render the sparkline for the given rolling window at the requested width.
///
/// - `ui` — the parent UI to paint into.
/// - `window` — the Story 1.6 rolling window (mutable because `as_slice()`
///   requires `&mut self` to make the VecDeque contiguous).
/// - `width` — pixel width of the sparkline; height defaults to
///   [`DEFAULT_HEIGHT`].
///
/// Empty window → renders the [`EMPTY_TEXT`] placeholder label and returns.
/// Otherwise: allocates the requested size, then paints line segments across
/// the finite (non-NaN) runs of the window.
pub fn render(ui: &mut Ui, window: &mut RollingWindow, width: f32) {
    let size = Vec2::new(width, DEFAULT_HEIGHT);
    if window.is_empty() {
        ui.label(EMPTY_TEXT);
        return;
    }
    let (rect, _response) = ui.allocate_at_least(size, egui::Sense::hover());
    let segments = render_segments(window, rect);
    paint_segments(ui, rect, &segments);
}

/// Compute the per-segment point lists for a window inside `rect`. Each
/// returned `Vec<Pos2>` is a contiguous run of finite values; NaN values
/// split the runs (gap rendering per Story 1.6).
///
/// Vertical mapping: `value` → `y` so the min/max of the finite values
/// span the rect's height. A single finite value (or all-identical values)
/// renders as a flat line at the vertical center.
///
/// Overflow: if `samples > pixel_width`, we stride across the values at
/// `samples / pixels` (integer) — every Nth sample wins. This keeps the
/// vertex count at or below the pixel count (T-22 cap is 600 samples vs a
/// 100px sparkline → 6:1 downsampling worst case).
#[must_use]
pub fn render_segments(window: &RollingWindow, rect: Rect) -> Vec<Vec<Pos2>> {
    // Story 8.7 RED stub: no segments computed, so the ascending-segments
    // assertions FAIL. The empty-window branch is handled in `render` so the
    // empty test passes.
    let _ = (window, rect);
    Vec::new()
}

/// Paint the computed segments into `ui`'s painter.
fn paint_segments(ui: &mut Ui, _rect: Rect, segments: &[Vec<Pos2>]) {
    let painter = ui.painter();
    let stroke = Stroke::new(STROKE_WIDTH, STROKE);
    for run in segments {
        if run.len() >= 2 {
            painter.line(run.clone(), stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.7 TDD contract tests (pure-fn segment computation + F8).
    //!
    //! RED phase: `render_segments` always returns empty, so the
    //! ascending-segments / flat-line / NaN-gap assertions FAIL. The
    //! empty-window path renders the placeholder in `render` so that test
    //! passes.

    use super::*;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;
    use sidebar_domain::graph::RollingWindow;

    /// Walk the kittest access tree collecting every node's text.
    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    /// A 100×24 rect at the origin for segment-position assertions.
    fn test_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, DEFAULT_HEIGHT))
    }

    // ===== Happy Path #1: [10, 20, 30] → 3 ascending segments =====
    //
    // "3 segments" here means a single run of 3 finite values, producing
    // 2 line segments (10→20, 20→30) — the F8 contract phrases it as
    // "renders 3 segments ascending" which we interpret as 3 vertices
    // (3 points = 2 line segments = strictly ascending line).

    #[test]
    fn three_ascending_values_produce_run_of_three_points() {
        let mut w = RollingWindow::new(60);
        w.push(10.0);
        w.push(20.0);
        w.push(30.0);
        let segments = render_segments(&w, test_rect());
        assert_eq!(
            segments.len(),
            1,
            "three finite values must produce a single contiguous run (got {segments:?})"
        );
        let run = &segments[0];
        assert_eq!(
            run.len(),
            3,
            "the run must have 3 vertices (10, 20, 30) — got {}",
            run.len()
        );
        // Strictly ascending in y (egui y grows downward, so higher value =
        // LOWER y; assert the y-coordinates are strictly descending).
        assert!(
            run[0].y > run[1].y && run[1].y > run[2].y,
            "ascending values must map to strictly descending y (egui y-down); got {run:?}"
        );
    }

    // ===== Happy Path #2: empty window → "—" placeholder =====

    #[test]
    fn empty_window_renders_placeholder() {
        let mut w = RollingWindow::new(60);
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut w, DEFAULT_WIDTH);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(EMPTY_TEXT),
            "empty window must render the placeholder '{EMPTY_TEXT}' (got: {labels})"
        );
    }

    // ===== Boundary #1: NaN → gap (split into two runs) =====

    #[test]
    fn nan_value_splits_into_two_runs() {
        let mut w = RollingWindow::new(60);
        w.push(10.0);
        w.push(f64::NAN);
        w.push(30.0);
        let segments = render_segments(&w, test_rect());
        assert_eq!(
            segments.len(),
            2,
            "NaN in the middle must split the line into two runs (got {} runs)",
            segments.len()
        );
        // First run = [10], second run = [30]. Neither has enough points to
        // draw a line on its own, but the gap is the contract.
        assert_eq!(segments[0].len(), 1);
        assert_eq!(segments[1].len(), 1);
    }

    #[test]
    fn nan_at_boundary_leaves_other_runs_intact() {
        // [10, 20, NaN, 40, 50] → two runs: [10, 20] and [40, 50].
        let mut w = RollingWindow::new(60);
        w.push(10.0);
        w.push(20.0);
        w.push(f64::NAN);
        w.push(40.0);
        w.push(50.0);
        let segments = render_segments(&w, test_rect());
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].len(), 2);
        assert_eq!(segments[1].len(), 2);
    }

    // ===== Boundary #2: overflow → downsample to pixel width =====
    //
    // We can't observe downsampled vertex count from render_segments directly
    // without a real rect width; here we push more samples than a 1px width
    // could hold and assert the run is bounded.

    #[test]
    fn overflow_does_not_exceed_pixel_count_plus_one() {
        let mut w = RollingWindow::new(600);
        for i in 0..300 {
            w.push(f64::from(i));
        }
        // 100px-wide rect → at most 101 vertices (one per pixel + endpoint).
        let rect = test_rect();
        let segments = render_segments(&w, rect);
        let total_vertices: usize = segments.iter().map(std::vec::Vec::len).sum();
        // The cast is safe: rect.width() is a small positive pixel count.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pixel_cap = (rect.width() as usize) + 1;
        assert!(
            total_vertices <= pixel_cap,
            "overflow must downsample to ≤ width+1 vertices (got {total_vertices}, cap {pixel_cap})"
        );
    }

    // ===== Boundary #3: all-identical values → flat line at vertical center =====

    #[test]
    fn all_identical_values_render_flat_at_center() {
        let mut w = RollingWindow::new(60);
        w.push(42.0);
        w.push(42.0);
        w.push(42.0);
        let rect = test_rect();
        let segments = render_segments(&w, rect);
        assert_eq!(segments.len(), 1);
        let run = &segments[0];
        assert_eq!(run.len(), 3);
        let center_y = rect.center().y;
        for p in run {
            assert!(
                (p.y - center_y).abs() < 0.01,
                "all-identical values must render at the vertical center y={center_y}; got {run:?}"
            );
        }
    }

    // ===== Sanity: constants =====

    #[test]
    fn empty_text_is_em_dash() {
        assert_eq!(EMPTY_TEXT, "—");
    }
}
