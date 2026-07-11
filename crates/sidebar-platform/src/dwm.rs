//! DWM peek exclusion + capture-affinity wrappers (Story 6.1).
//!
//! ## Why these wrappers
//!
//! The sidebar viewport must stay visible during Aero Peek (Win+Tab,
//! hover-show-desktop). Peek exclusion uses `DwmSetWindowAttribute`; capture
//! exclusion uses `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on the
//! viewport HWND. `set_capture_cloak` retains its caller-facing name for
//! compatibility even though its implementation is capture affinity, not DWM
//! cloaking.
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! Each `unsafe` block carries a `// SAFETY:` comment explaining HWND
//! validity + attribute-pointer lifetime + size. The workspace lint
//! `clippy::undocumented_unsafe_blocks = "deny"` enforces this.

use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_EXCLUDED_FROM_PEEK};
use windows::Win32::UI::WindowsAndMessaging::WDA_EXCLUDEFROMCAPTURE;

use sidebar_domain::error::{Error, Result};

/// Apply DWMWA_EXCLUDED_FROM_PEEK = TRUE so the sidebar does not fade out
/// during Aero Peek (Win+Tab, hover-show-desktop). Idempotent.
///
/// # Errors
/// Returns [`Error::Platform`] if the Win32 call reports an HRESULT failure
/// (typically: DWM disabled / compositor off, or the HWND is not a top-level
/// window). Callers should treat failure as non-fatal — the window still
/// works, just behaves like a normal window during Peek.
///
/// # Cited
/// Story 6.1 TDD contract (3): `dwm::exclude_from_peek(hwnd)` calls
/// `DwmSetWindowAttribute` with `DWMWA_EXCLUDED_FROM_PEEK`. NFR-7.
pub fn exclude_from_peek(hwnd: HWND) -> Result<()> {
    set_bool_attribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, true)
}

/// Toggle capture exclusion on the sidebar viewport. When `enabled = true`,
/// `WDA_EXCLUDEFROMCAPTURE` hides the window contents from supported capture
/// APIs while leaving the window visible; `false` restores `WDA_NONE`.
///
/// # Default
/// OFF — most users want the sidebar captured in OBS / Snipping Tool.
///
/// # Errors
/// Returns [`Error::Platform`] on HRESULT failure. Callers should treat
/// older Windows (pre-Vista) as "attribute unsupported → no-op + debug!"
/// (Story 6.1 Boundary #5) — the HRESULT check below surfaces that as
/// `Err`, and the Epic-6 wiring layer decides whether to log-and-continue.
///
/// # Cited
/// Story 6.1 TDD contract (4): `dwm::set_capture_cloak(hwnd, true)` applies
/// `WDA_EXCLUDEFROMCAPTURE`. NFR-7.
pub fn set_capture_cloak(hwnd: HWND, enabled: bool) -> Result<()> {
    let affinity = if enabled {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        windows::Win32::UI::WindowsAndMessaging::WDA_NONE
    };
    // SAFETY: `hwnd` is the caller's live top-level window handle and
    // `affinity` is a documented WINDOW_DISPLAY_AFFINITY value. The call is
    // synchronous and does not retain any borrowed memory.
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(hwnd, affinity) }
        .map_err(|e| Error::Platform(format!("SetWindowDisplayAffinity failed: {e}")))
}

/// Shared helper: set a DWM attribute whose value is a single `BOOL`.
/// `DWMWA_EXCLUDED_FROM_PEEK` takes `BOOL*` per the MS
/// `DwmSetWindowAttribute` contract — passing the wrong size silently
/// no-ops the call, so we pin `cbAttribute = size_of::<BOOL>()` (= 4).
///
/// # Errors
/// `Error::Platform` if the HRESULT returned by `DwmSetWindowAttribute` is a
/// failure (the windows crate wraps the call in `Result<()>`; `.ok()` turns
/// failure into `Err(windows::core::Error)` which we stringify into
/// `Error::Platform`).
fn set_bool_attribute(
    hwnd: HWND,
    attr: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: bool,
) -> Result<()> {
    let bool_value = BOOL(i32::from(value));
    // SAFETY: `hwnd` is the caller's window handle — DWM checks validity and
    // returns an HRESULT failure for an invalid/null HWND, which we surface
    // as Error::Platform below. `&raw const bool_value` is a pointer into a
    // stack local that outlives the call; `cbattribute = size_of::<BOOL>()`
    // (= 4) matches the documented BOOL* contract for both peek + cloak
    // attrs. No threading concern — DWM attribute set is synchronous.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            (&raw const bool_value).cast::<std::ffi::c_void>(),
            u32::try_from(size_of::<BOOL>()).unwrap_or(0),
        )
    };
    result.map_err(|e| {
        tracing::debug!(
            target = "sidebar.platform.dwm",
            attr = attr.0,
            error = %e,
            "DwmSetWindowAttribute failed"
        );
        Error::Platform(format!(
            "DwmSetWindowAttribute(attr={}) failed: {e}",
            attr.0
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 6.1 TDD contract #3: `exclude_from_peek` accepts an HWND and
    /// returns `Ok` on the happy path. The real-FFI call needs a live window
    /// (the test crate doesn't own one) → marked `#[ignore]` and run in
    /// sdd-verify against a real egui viewport (architecture.md §7.4).
    ///
    /// Cited: F11 (unsafe FFI test with SAFETY contract), §7.4 manual smoke.
    #[test]
    #[ignore = "needs a real Win32 HWND (sdd-verify manual smoke, §7.4)"]
    fn exclude_from_peek_smoke_real_window() {
        // Placeholder: the sdd-verify harness passes a live HWND here. The
        // assertion is "returns Ok" — the visual check (sidebar doesn't fade
        // during Win+Tab) is the manual smoke item.
        // We can't synthesize a valid HWND in a unit test, so this is a
        // documentation-shaped placeholder.
        let hwnd = HWND(std::ptr::null_mut());
        let result = exclude_from_peek(hwnd);
        // We expect Err for a null HWND (DWM rejects it); the point of this
        // test is to document the call shape, not assert a particular result
        // without a real window.
        let _ = result;
    }

    /// Story 6.1 TDD contract #4: `set_capture_cloak(hwnd, true)` shape.
    /// Same #[ignore] rationale as above.
    #[test]
    #[ignore = "needs a real Win32 HWND (sdd-verify manual smoke, §7.4)"]
    fn set_capture_cloak_smoke_real_window() {
        let hwnd = HWND(std::ptr::null_mut());
        let _ = set_capture_cloak(hwnd, true);
        let _ = set_capture_cloak(hwnd, false);
    }

    /// Pure-logic check: the size we pass to `DwmSetWindowAttribute` for a
    /// BOOL attribute is `size_of::<BOOL>()`. This catches the classic
    /// off-by-attribute-size bug without touching FFI.
    #[test]
    fn bool_attribute_size_is_four_bytes() {
        // In `windows = 0.62`, `BOOL` lives at `windows::core::BOOL` (it
        // moved out of `Win32::Foundation` between 0.5x and 0.6x).
        // DWM documents the attribute pointer for peek/cloak as `BOOL*`, so
        // the cbAttribute passed in MUST be 4. A wrong size is silently
        // ignored by DWM → attribute does nothing → bug surfaces only in
        // manual smoke. Pin the size here.
        assert_eq!(
            size_of::<BOOL>(),
            4,
            "BOOL attribute size must be 4 bytes (DWMWA_EXCLUDED_FROM_PEEK contract)"
        );
    }

    /// The peek-attribute constant must equal the documented DWM value (12).
    /// A wrong constant would silently no-op the attribute.
    #[test]
    fn dwmwa_excluded_from_peek_value_is_twelve() {
        assert_eq!(DWMWA_EXCLUDED_FROM_PEEK.0, 12);
    }

    /// The capture-affinity constant must equal the documented Win32 value
    /// (`WDA_EXCLUDEFROMCAPTURE = 17`).
    #[test]
    fn capture_affinity_value_is_exclude_from_capture() {
        assert_eq!(WDA_EXCLUDEFROMCAPTURE.0, 17);
    }

    /// Null HWND should produce `Err(Platform)` once implemented — DWM rejects
    /// invalid handles. This test will START FAILING in RED (Ok returned) and
    /// turn GREEN in the impl commit (the call returns Err). Marker for G1.
    #[test]
    fn null_hwnd_exclude_from_peek_is_err_once_implemented() {
        let hwnd = HWND(std::ptr::null_mut());
        let result = exclude_from_peek(hwnd);
        // In RED this is Ok (stub). In GREEN it MUST be Err.
        // We assert Err to drive the impl; flip back to Ok-tolerant if the
        // impl legitimately succeeds on null handles on some Windows builds.
        assert!(
            matches!(result, Err(Error::Platform(_))),
            "null HWND should be rejected by DWM, got: {result:?}"
        );
    }

    /// Same null-HWND contract for `set_capture_cloak`.
    #[test]
    fn null_hwnd_set_capture_cloak_is_err_once_implemented() {
        let hwnd = HWND(std::ptr::null_mut());
        let result = set_capture_cloak(hwnd, true);
        assert!(
            matches!(result, Err(Error::Platform(_))),
            "null HWND should be rejected by DWM, got: {result:?}"
        );
    }
}
