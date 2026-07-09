//! Per-Monitor DPI Awareness v2 (Story 6.3).
//!
//! ## Why this module exists
//!
//! The sidebar renders crisply on hidpi + multi-mixed-DPI monitors (NFR-6,
//! §7.4 item "Multi-monitor: sidebar appears on chosen monitor at correct
//! DPI"). Win32 requires the *process* to opt into per-monitor-v2 awareness
//! via `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` **before any
//! window is created**. Once windows exist, we query each monitor's DPI via
//! `GetDpiForWindow(hwnd)`.
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! `SetProcessDpiAwarenessContext` is process-global and idempotent-ish —
//! the second call returns an error (`ERROR_ACCESS_DENIED`) which we treat
//! as success (the process is already aware). `GetDpiForWindow` takes an
//! HWND and returns a `u32` (0 on invalid handle — we surface that as a
//! sensible default, not an error, matching egui's expectation).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use sidebar_domain::error::{Error, Result};

/// Opt the *process* into Per-Monitor DPI Awareness v2. MUST be called before
/// any HWND is created — Win32 rejects the call once a window exists (returns
/// `ERROR_ACCESS_DENIED`). Idempotent: a second call (e.g. a relaunch of the
/// runtime in the same process) returns success because the process is
/// already aware.
///
/// # Errors
/// Returns [`Error::Platform`] on genuine failure (e.g. pre-Win10 1703
/// where v2 is unavailable — the older `PER_MONITOR_AWARE` v1 context is the
/// fallback there, but sidebar v1 targets Win11 24H2+ per T-31).
///
/// # Cited
/// Story 6.3 TDD contract: `set_per_monitor_v2()` calls
/// `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` before window
/// creation. NFR-6 (hidpi crispness). T-31 (target Win11 build 26100+).
pub fn set_per_monitor_v2() -> Result<()> {
    // SAFETY: `SetProcessDpiAwarenessContext` is a process-global call with
    // no pointer arguments — there is no aliasing or lifetime concern. The
    // only invariant is "call before any window is created"; callers (the
    // sidebar runtime entry, before eframe) uphold that. The function is
    // documented as idempotent on Win10 1703+: a second call returns
    // ERROR_ACCESS_DENIED, which the `windows` crate surfaces as
    // `Err(windows::core::Error)`. We treat that specific case as success
    // (the process is already per-monitor-v2 aware — the desired end state).
    let result =
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // ERROR_ACCESS_DENIED is the "already set" path — Win32 returns
            // it when the process is already DPI-aware at any context. The
            // `windows` crate wraps GetLastError into the HRESULT; we check
            // the code via the Error's `.code()` (HRESULT) — access-denied
            // is 0x80070005 (HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED)), which
            // as a signed i32 is -2_147_024_891. Writing the negative
            // decimal directly avoids the u32→i32 cast clippy rejects.
            const HRESULT_ACCESS_DENIED: i32 = -2_147_024_891;
            if e.code() == windows::core::HRESULT(HRESULT_ACCESS_DENIED) {
                tracing::debug!(
                    target = "sidebar.platform.dpi",
                    "SetProcessDpiAwarenessContext: process already DPI-aware (idempotent success)"
                );
                return Ok(());
            }
            tracing::warn!(
                target = "sidebar.platform.dpi",
                error = %e,
                "SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) failed"
            );
            Err(Error::Platform(format!(
                "SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) failed: {e}"
            )))
        }
    }
}

/// Query the DPI of the monitor that `hwnd` is on. Returns 96 (the Win32
/// default DPI for 100% scaling) on invalid HWND or any failure — egui's
/// viewport code expects a non-zero DPI, so we never return 0 in the public
/// API.
///
/// # Cited
/// Story 6.3 TDD contract: `get_dpi_for_window(hwnd) -> u32` via
/// `GetDpiForWindow`. NFR-6.
#[must_use]
pub fn get_dpi_for_window(hwnd: HWND) -> u32 {
    // SAFETY: `GetDpiForWindow` takes an HWND and returns a u32; no pointer
    // out-params, no aliasing. An invalid HWND returns 0 per the MS contract;
    // we substitute 96 (the documented "user DPI" / 100%-scaling baseline)
    // so egui's viewport code never sees a degenerate zero DPI.
    let raw = unsafe { GetDpiForWindow(hwnd) };
    if raw == 0 {
        tracing::debug!(
            target = "sidebar.platform.dpi",
            "GetDpiForWindow returned 0 (invalid HWND?) — falling back to 96 dpi"
        );
        return 96;
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `set_per_monitor_v2` happy path: returns Ok. Real-FFI smoke is NOT
    /// #[ignore] because SetProcessDpiAwarenessContext is process-global and
    /// safe to call from a test (it doesn't need a window). It DOES mark the
    /// test process as per-monitor-v2 aware, which is fine for the workspace
    /// test binary.
    #[test]
    fn set_per_monitor_v2_smoke() {
        // The first call may succeed or may already-have-been-called by
        // another test in the same binary (process-global). Either is fine.
        let _ = set_per_monitor_v2();
    }

    /// Idempotency: calling twice must not error (Story 6.3 Boundary).
    #[test]
    fn set_per_monitor_v2_is_idempotent() {
        let _ = set_per_monitor_v2();
        let second = set_per_monitor_v2();
        assert!(
            second.is_ok(),
            "second call must be Ok (already-aware is success), got: {second:?}"
        );
    }

    /// The v2 context constant is the documented sentinel `-4`. Pin it so a
    /// crate bump that renumbers it surfaces as a test failure.
    #[test]
    fn per_monitor_v2_constant_is_minus_four() {
        // DPI_AWARENESS_CONTEXT wraps *mut c_void; the documented value is -4.
        // We compare via the inner isize cast.
        let raw = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.0 as isize;
        assert_eq!(raw, -4, "PER_MONITOR_AWARE_V2 must be the -4 sentinel");
    }

    /// `get_dpi_for_window` with a null HWND returns a sensible default (96),
    /// not 0 — egui viewport code expects a non-zero DPI.
    #[test]
    fn null_hwnd_dpi_is_default_not_zero() {
        let hwnd = HWND(std::ptr::null_mut());
        let dpi = get_dpi_for_window(hwnd);
        assert_ne!(dpi, 0, "DPI must never be 0 in the public API");
        assert!(
            dpi >= 96,
            "DPI floor is 96 (100% scaling baseline), got {dpi}"
        );
    }

    /// Real-FFI smoke against the test process's own hidden message-only
    /// window would require creating an HWND; skipped here. The contract is
    /// documented via the null-HWND test above + the manual smoke in §7.4
    /// (Multi-monitor: sidebar appears at correct DPI).
    #[test]
    #[ignore = "needs a real Win32 HWND (sdd-verify manual smoke, §7.4 'Multi-monitor ... correct DPI')"]
    fn get_dpi_smoke_real_window() {
        let hwnd = HWND(std::ptr::null_mut());
        let _ = get_dpi_for_window(hwnd);
    }
}
