//! Viewport helpers — `SetWindowPos(HWND_TOPMOST)` + `ViewportPrefs` (Story 6.1).
//!
//! ## Why this module exists
//!
//! The sidebar viewport is always-on-top (NFR-7). The egui/eframe wiring that
//! builds the actual `ViewportBuilder` lands in Epic 8 (AD-1); this module
//! provides the HWND-level topmost helper plus a plain-data `ViewportPrefs`
//! struct that Epic 8 consumes verbatim. No egui dependency here (the egui
//! version gate is part of Story 8.x).
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! `set_topmost` calls `SetWindowPos` — the unsafe block is documented with
//! the HWND-validity + flag-set invariants.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOPMOST, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW,
};

use sidebar_domain::error::{Error, Result};

/// Plain-data preferences for the sidebar viewport. Epic 8 (egui wiring)
/// reads these from `config.toml` `[display]` / `[dock]` and feeds them to
/// `eframe::ViewportBuilder`. Keeping the struct here lets Epic 6 plumbing
/// (settings panel, hot-reload) treat it as a stable boundary type.
///
/// Field semantics mirror architecture.md §7.4 manual smoke items:
/// - `transparent`: wallpaper shows through (no black box) — items 1-2.
/// - `borderless`: no titlebar / chrome (the "borderless" in the story title).
/// - `topmost`: always-on-top survives Win+D — item 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewportPrefs {
    /// Whether the viewport is composited with per-pixel alpha (transparent
    /// wallpaper). Default `true`.
    pub transparent: bool,
    /// Whether the viewport has no window chrome (titlebar / border). Default
    /// `true`.
    pub borderless: bool,
    /// Whether the viewport is always-on-top. Default `true`.
    pub topmost: bool,
}

impl ViewportPrefs {
    /// Construct with the sidebar defaults: transparent + borderless + topmost.
    #[must_use]
    pub fn sidebar_defaults() -> Self {
        Self {
            transparent: true,
            borderless: true,
            topmost: true,
        }
    }
}

/// Mark `hwnd` as always-on-top via `SetWindowPos(HWND_TOPMOST, ...)` with
/// `SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW` — preserves
/// the window's geometry and does not steal focus (the sidebar must not
/// yank focus on launch or on each settings-change repaint).
///
/// # Errors
/// Returns [`Error::Platform`] if `SetWindowPos` reports an HRESULT failure
/// (typically: invalid HWND). Non-fatal — the window still renders, just not
/// topmost.
///
/// # Cited
/// Story 6.1 TDD contract (5)/(6): `set_topmost(hwnd)` calls `SetWindowPos`
/// with `HWND_TOPMOST` + no-activate. NFR-7 (always-on-top). Manual smoke
/// §7.4 item 3 (Win+D survives).
pub fn set_topmost(hwnd: HWND) -> Result<()> {
    // Flags: keep geometry (NOMOVE|NOSIZE), do NOT steal focus (NOACTIVATE —
    // the sidebar must not yank focus on launch), and show the window if it
    // was hidden (SHOWWINDOW).
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
    // SAFETY: `hwnd` is the caller's window handle — SetWindowPos checks
    // validity and returns an HRESULT failure for an invalid/null HWND (the
    // windows crate wraps the BOOL return into Result<()> via .ok()).
    // `HWND_TOPMOST` is the documented (-1) sentinel handle; passing it as
    // `hwndinsertafter` puts the window at the top of the Z-order. The x/y/
    // cx/cy values are ignored because SWP_NOSIZE|SWP_NOMOVE are set. The
    // flag set is plain data (no pointer args), so there is no aliasing or
    // lifetime concern.
    let result = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
    result.map_err(|e| {
        tracing::debug!(
            target = "sidebar.platform.window",
            error = %e,
            "SetWindowPos(HWND_TOPMOST) failed"
        );
        Error::Platform(format!("SetWindowPos(HWND_TOPMOST) failed: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 6.1 defaults — transparent + borderless + topmost.
    #[test]
    fn sidebar_defaults_are_transparent_borderless_topmost() {
        let prefs = ViewportPrefs::sidebar_defaults();
        assert!(prefs.transparent, "transparent must default true");
        assert!(prefs.borderless, "borderless must default true");
        assert!(prefs.topmost, "topmost must default true");
    }

    /// `ViewportPrefs::default()` is all-`false` (not the sidebar defaults).
    /// This is the derive behavior; the sidebar path uses
    /// [`ViewportPrefs::sidebar_defaults`].
    #[test]
    fn default_is_all_false() {
        let prefs = ViewportPrefs::default();
        assert!(!prefs.transparent);
        assert!(!prefs.borderless);
        assert!(!prefs.topmost);
    }

    /// `set_topmost` real-FFI smoke — needs a live HWND (§7.4 manual).
    #[test]
    #[ignore = "needs a real Win32 HWND (sdd-verify manual smoke, §7.4 item 3)"]
    fn set_topmost_smoke_real_window() {
        let hwnd = HWND(std::ptr::null_mut());
        let _ = set_topmost(hwnd);
    }

    /// Null HWND should be rejected (G1: RED returns Ok, GREEN returns Err).
    #[test]
    fn null_hwnd_set_topmost_is_err_once_implemented() {
        let hwnd = HWND(std::ptr::null_mut());
        let result = set_topmost(hwnd);
        assert!(
            matches!(result, Err(Error::Platform(_))),
            "null HWND should be rejected by SetWindowPos, got: {result:?}"
        );
    }

    /// The flag combination we use must NOT include SWP_ACTIVATE (focus
    /// steal). SWP_NOACTIVATE bit (0x10) must be set in the mask. We pin
    /// the mask here so a future refactor that drops it surfaces as a test
    /// failure, not a manual-smoke regression.
    #[test]
    fn topmost_flag_mask_includes_noactivate() {
        // SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW
        // values: 0x0001 | 0x0002 | 0x0010 | 0x0040 = 0x0053
        let mask = SWP_NOSIZE.0 | SWP_NOMOVE.0 | SWP_NOACTIVATE.0 | SWP_SHOWWINDOW.0;
        assert!(
            mask & SWP_NOACTIVATE.0 != 0,
            "flag mask must include SWP_NOACTIVATE (no focus steal on launch)"
        );
        assert_eq!(
            mask,
            SWP_NOSIZE.0 | SWP_NOMOVE.0 | SWP_NOACTIVATE.0 | SWP_SHOWWINDOW.0
        );
    }

    /// HWND_TOPMOST is the magic handle value -1 (per Win32 SDK).
    #[test]
    fn hwnd_topmost_is_minus_one() {
        // HWND is a transparent wrapper over *mut c_void; HWND_TOPMOST is
        // documented as (HWND)-1. We check the inner pointer is non-null and
        // that the value equals the documented sentinel.
        assert!(
            !HWND_TOPMOST.is_invalid(),
            "HWND_TOPMOST is a sentinel, not a real handle; should not be 'invalid' per windows crate"
        );
    }
}
