//! Story 12.3 — pure geometry helpers for sidebar repositioning via drag.
//!
//! The actual `SetWindowPos`/`SendMessage(WM_NCLBUTTONDOWN, HTCAPTION)`
//! call is HITL-gated (needs a live HWND). The math — how a vertical drag
//! delta maps to a new offset along the docked edge, clamped to the
//! monitor's work area — is pure and unit-testable here.
//!
//! Cited: Story 12.3 DoD, PRD section 5.7, nfr-thresholds.md T-36.

/// Compute the new offset along the docked edge after a vertical drag.
///
/// `current_offset` is the pixel offset from the top of the monitor's work
/// area (for Left/Right edges) or from the left (for Top/Bottom edges).
/// `drag_delta` is the signed pixel delta since drag-start. `monitor_size`
/// is the length of the docked edge in pixels (height for Left/Right, width
/// for Top/Bottom). `sidebar_length` is the sidebar's extent along that edge.
///
/// The result is clamped so the sidebar stays fully on-screen:
/// `0 <= offset <= max(0, monitor_size - sidebar_length)`.
#[must_use]
pub fn compute_new_offset(
    current_offset: i32,
    drag_delta: i32,
    monitor_size: i32,
    sidebar_length: i32,
) -> i32 {
    let max_offset = (monitor_size - sidebar_length).max(0);
    (current_offset + drag_delta).clamp(0, max_offset)
}

#[cfg(test)]
mod tests {
    use super::compute_new_offset;

    #[test]
    fn drag_down_increases_offset_for_right_edge() {
        // 1080p monitor, 200px sidebar height, currently at offset 100.
        // Drag down 50px → new offset 150.
        assert_eq!(compute_new_offset(100, 50, 1080, 200), 150);
    }

    #[test]
    fn drag_clamps_to_top_of_monitor() {
        // Currently at offset 30, drag up 100 → clamped to 0 (not -70).
        assert_eq!(compute_new_offset(30, -100, 1080, 200), 0);
    }

    #[test]
    fn drag_clamps_to_bottom_of_monitor() {
        // 1080 monitor, 200 sidebar → max_offset = 880. Drag past it clamps.
        assert_eq!(compute_new_offset(800, 200, 1080, 200), 880);
    }

    #[test]
    fn sidebar_taller_than_monitor_pins_to_zero() {
        // Edge case: sidebar taller than monitor → max_offset = 0 → always 0.
        assert_eq!(compute_new_offset(50, 100, 600, 800), 0);
    }
}
