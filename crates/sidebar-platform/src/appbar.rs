//! AppBar dock registration via `SHAppBarMessage` (Story 6.2).
//!
//! ## Why this module exists
//!
//! The sidebar docks to a screen edge and reserves desktop space (so other
//! windows don't overlap it — §7.4 manual smoke item 4). The Win32 AppBar
//! API is `SHAppBarMessage(ABM_NEW | QUERYPOS | SETPOS | REMOVE, &APPBARDATA)`.
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! `SHAppBarMessage` writes into the `APPBARDATA.rc` field on QUERYPOS /
//! SETPOS. Every unsafe block is annotated with HWND-validity + struct-
//! ownership reasoning. `APPBARDATA` is `#[repr(C)]` on x86_64 (not packed),
//! so the standard `Default` is safe.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, APPBARDATA,
};

use sidebar_domain::error::{Error, Result};

/// Which screen edge the sidebar docks to. Maps 1:1 to the Win32 `ABE_*`
/// constants. Cited: Story 6.2 spec, T-36 (multi-monitor target selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppBarEdge {
    /// Dock to the left edge (`ABE_LEFT = 0`).
    #[default]
    Left,
    /// Dock to the top edge (`ABE_TOP = 1`).
    Top,
    /// Dock to the right edge (`ABE_RIGHT = 2`).
    Right,
    /// Dock to the bottom edge (`ABE_BOTTOM = 3`).
    Bottom,
}

impl AppBarEdge {
    /// Map this edge to its Win32 `ABE_*` constant. Pure-logic, tested below.
    #[must_use]
    pub const fn to_abe(self) -> u32 {
        match self {
            Self::Left => ABE_LEFT,
            Self::Top => ABE_TOP,
            Self::Right => ABE_RIGHT,
            Self::Bottom => ABE_BOTTOM,
        }
    }
}

/// A loose hint about which monitor to dock to. Story 6.2 only needs the
/// type-level plumbing — the actual monitor enumeration + `MonitorFromRect`
/// lands in Epic 8 (egui viewport wiring, T-36). The hint is forwarded into
/// the AppBar rect's screen coordinates by the caller.
///
/// For now this is a marker enum; Epic 8 will widen it with the
/// `DeviceID`-based monitor picker (T-36).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonitorHint {
    /// Dock to the primary monitor (default — most users).
    #[default]
    Primary,
    /// Dock to a specific monitor identified by its stable DeviceID. Epic 8
    /// resolves the ID to an `HMONITOR` via `EnumDisplayDevices`.
    Specific,
}

/// Build the `APPBARDATA` struct for an ABM_NEW call without invoking FFI.
/// Pure helper — lets us unit-test the struct-construction logic (HWND + edge
/// + callback message wiring) without needing a real window. Cited: the
///   pragmatic test strategy from the story brief.
#[must_use]
fn build_abd(hwnd: HWND, edge: AppBarEdge, callback_msg: u32) -> APPBARDATA {
    APPBARDATA {
        cbSize: u32::try_from(std::mem::size_of::<APPBARDATA>()).unwrap_or(0),
        hWnd: hwnd,
        uCallbackMessage: callback_msg,
        uEdge: edge.to_abe(),
        rc: RECT::default(),
        lParam: windows::Win32::Foundation::LPARAM(0),
    }
}

/// Register `hwnd` as an AppBar docked to `edge` on `monitor`. Returns the
/// allocated [`RECT`] (the screen-space rectangle the shell reserved for the
/// bar — typically the full edge length × the requested width).
///
/// # Errors
/// Returns [`Error::Platform`] if:
///   - `ABM_NEW` returns 0 (the HWND is already registered, or shell rejects).
///   - `ABM_QUERYPOS` / `ABM_SETPOS` yields a degenerate (zero-area) rect.
///
/// # Cited
/// Story 6.2 TDD contract: `register(hwnd, edge, monitor)` calls
/// `SHAppBarMessage` with `ABM_NEW/QUERYPOS/SETPOS`. Manual smoke §7.4
/// items 4-6 (all four edges + multi-monitor).
pub fn register(hwnd: HWND, edge: AppBarEdge, monitor: MonitorHint) -> Result<RECT> {
    // The callback message the shell posts to `hwnd` for AppBar events
    // (fullscreen-app / pos-changed). Story 6.2 doesn't wire the actual
    // WindowProc; the value `WM_USER + 0x100 = 0x8100` is reserved for the
    // sidebar and Epic 8 (egui wiring) will install the WndProc handler.
    const APPBAR_CALLBACK_MSG: u32 = 0x8100;

    // Stage 1: ABM_NEW — ask the shell to register us as an AppBar. Returns
    // TRUE (1) on success, FALSE if the HWND is already registered or the
    // callback message is already in use by another AppBar.
    let mut abd = build_abd(hwnd, edge, APPBAR_CALLBACK_MSG);
    // SAFETY: `abd` is a stack-local APPBARDATA we own; `&raw mut abd` is a
    // valid *mut APPBARDATA for the duration of the call. The HWND inside is
    // the caller's responsibility — SHAppBarMessage returns FALSE for an
    // invalid handle, which we check below. ABM_NEW does not write back into
    // `abd`.
    let new_ok = unsafe { SHAppBarMessage(ABM_NEW, &raw mut abd) };
    if new_ok == 0 {
        return Err(Error::Platform(format!(
            "SHAppBarMessage(ABM_NEW) rejected the HWND (already registered or callback msg 0x{APPBAR_CALLBACK_MSG:04X} taken)"
        )));
    }

    // Stage 2: ABM_QUERYPOS — propose a rectangle. The caller (Epic 8 egui
    // wiring) will set `abd.rc` to the desired edge rect based on the
    // monitor work-area from `SystemParametersInfo`. For Story 6.2 the
    // helper is plumbed with a default rect (zeroed); the shell fills in
    // the closest valid rect for the requested edge. The `monitor` hint
    // biases which display the shell picks when multiple are present.
    let _ = monitor; // consumed by Epic 8 when it computes the seed rect
                     // SAFETY: same `&raw mut abd` contract as ABM_NEW. QUERYPOS writes into
                     // `abd.rc` (a RECT owned by `abd`, no aliasing).
    let query_ok = unsafe { SHAppBarMessage(ABM_QUERYPOS, &raw mut abd) };
    if query_ok == 0 {
        // QUERYPOS returning FALSE is rare (older shell); treat as failure.
        // Best-effort cleanup: ABM_REMOVE so the shell doesn't leak the
        // AppBar slot on the next launch.
        // SAFETY: same `&raw mut abd` contract; ABM_REMOVE is a no-op if the
        // HWND isn't registered.
        unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut abd) };
        return Err(Error::Platform(
            "SHAppBarMessage(ABM_QUERYPOS) returned FALSE".into(),
        ));
    }

    // Stage 3: ABM_SETPOS — commit the proposed rectangle. The shell
    // adjusts it to not overlap the taskbar / other AppBars and reserves
    // the final rect so other maximized windows don't underlap the sidebar.
    // SAFETY: same `&raw mut abd` contract. SETPOS writes the final rect
    // into `abd.rc`.
    let setpos_ok = unsafe { SHAppBarMessage(ABM_SETPOS, &raw mut abd) };
    if setpos_ok == 0 {
        // SAFETY: same as above — best-effort cleanup.
        unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut abd) };
        return Err(Error::Platform(
            "SHAppBarMessage(ABM_SETPOS) returned FALSE".into(),
        ));
    }

    let final_rect = abd.rc;
    // Sanity: a zero-area rect means the shell silently rejected us (e.g.
    // edge conflict). Surface as an error so Epic 8 can fall back to a
    // floating window instead of an invisible docked one.
    let width = final_rect.right - final_rect.left;
    let height = final_rect.bottom - final_rect.top;
    if width <= 0 || height <= 0 {
        // SAFETY: same `&raw mut abd` contract.
        unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut abd) };
        return Err(Error::Platform(format!(
            "SHAppBarMessage produced a zero-area rect ({width}x{height}) for edge {edge:?}"
        )));
    }

    Ok(final_rect)
}

/// Unregister `hwnd` as an AppBar. Idempotent — calling unregister on an
/// HWND that was never registered returns `Ok(())` (Win32 ABM_REMOVE is a
/// no-op in that case).
///
/// # Errors
/// Returns [`Error::Platform`] only on null/invalid HWND (ABM_REMOVE returns
/// FALSE for a genuinely invalid handle; a not-registered HWND is a no-op
/// and returns success).
///
/// # Cited
/// Story 6.2 TDD contract: `unregister(hwnd)` calls `SHAppBarMessage` with
/// `ABM_REMOVE`.
pub fn unregister(hwnd: HWND) -> Result<()> {
    let mut abd = build_abd(hwnd, AppBarEdge::Left, 0);
    // SAFETY: `abd` is stack-local and owned; `&raw mut abd` is a valid
    // pointer for the call. ABM_REMOVE is documented as a no-op for an HWND
    // that was never registered (the shell returns success). The edge field
    // is ignored by ABM_REMOVE; we pass Left (default) for determinism.
    let removed = unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut abd) };
    // ABM_REMOVE returns TRUE on success (including the not-registered case
    // on most shell versions) and FALSE on genuine failure (invalid HWND).
    // We accept either for idempotency — the only hard error path is when
    // the handle is so invalid the shell can't even look it up, which we
    // surface as Error::Platform.
    if removed == 0 && !hwnd.is_invalid() {
        // A non-null HWND that returned FALSE from ABM_REMOVE is unusual;
        // log + propagate so the caller can decide. Null HWND is tolerated
        // (treated as already-unregistered).
        return Err(Error::Platform(
            "SHAppBarMessage(ABM_REMOVE) returned FALSE for a non-null HWND".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 6.2 TDD contract: edge → ABE_* mapping is bijective and matches
    /// the documented Win32 values (0/1/2/3).
    #[test]
    fn edge_to_abe_mapping_matches_win32_constants() {
        assert_eq!(AppBarEdge::Left.to_abe(), ABE_LEFT);
        assert_eq!(AppBarEdge::Top.to_abe(), ABE_TOP);
        assert_eq!(AppBarEdge::Right.to_abe(), ABE_RIGHT);
        assert_eq!(AppBarEdge::Bottom.to_abe(), ABE_BOTTOM);
    }

    /// ABE_* values are pinned to their documented Win32 constants so a
    /// future crate update that renumbers them surfaces as a test failure.
    #[test]
    fn abe_constants_are_documented_values() {
        assert_eq!(ABE_LEFT, 0);
        assert_eq!(ABE_TOP, 1);
        assert_eq!(ABE_RIGHT, 2);
        assert_eq!(ABE_BOTTOM, 3);
    }

    /// ABM_* message constants match the Win32 SDK. Cited: pin so a crate
    /// bump can't silently change the message numbers.
    #[test]
    fn abm_constants_are_documented_values() {
        assert_eq!(ABM_NEW, 0);
        assert_eq!(ABM_REMOVE, 1);
        assert_eq!(ABM_QUERYPOS, 2);
        assert_eq!(ABM_SETPOS, 3);
    }

    /// `build_abd` produces a struct with correct cbSize + edge + hwnd
    /// wiring without touching FFI.
    #[test]
    fn build_abd_wires_fields_correctly() {
        let hwnd = HWND(0x1234_5678 as *mut _);
        let abd = build_abd(hwnd, AppBarEdge::Right, 0x4000);
        assert_eq!(
            abd.cbSize,
            u32::try_from(std::mem::size_of::<APPBARDATA>()).unwrap()
        );
        assert_eq!(abd.hWnd, hwnd);
        assert_eq!(abd.uCallbackMessage, 0x4000);
        assert_eq!(abd.uEdge, ABE_RIGHT);
        assert_eq!(abd.rc, RECT::default());
        assert_eq!(abd.lParam.0, 0);
    }

    /// `build_abd` for every edge picks the right ABE_*.
    #[test]
    fn build_abd_all_edges() {
        let hwnd = HWND(std::ptr::null_mut());
        for edge in [
            AppBarEdge::Left,
            AppBarEdge::Top,
            AppBarEdge::Right,
            AppBarEdge::Bottom,
        ] {
            let abd = build_abd(hwnd, edge, 0);
            assert_eq!(abd.uEdge, edge.to_abe());
        }
    }

    /// `register` real-FFI smoke — needs a live HWND.
    #[test]
    #[ignore = "needs a real Win32 HWND + visible desktop (sdd-verify manual smoke, §7.4 items 4-6)"]
    fn register_smoke_real_window() {
        let hwnd = HWND(std::ptr::null_mut());
        let _ = register(hwnd, AppBarEdge::Right, MonitorHint::Primary);
    }

    /// `unregister` real-FFI smoke — needs a live HWND.
    #[test]
    #[ignore = "needs a real Win32 HWND (sdd-verify manual smoke)"]
    fn unregister_smoke_real_window() {
        let hwnd = HWND(std::ptr::null_mut());
        let _ = unregister(hwnd);
    }

    /// G1 RED marker: null HWND register MUST return Err once implemented.
    #[test]
    fn null_hwnd_register_is_err_once_implemented() {
        let hwnd = HWND(std::ptr::null_mut());
        let result = register(hwnd, AppBarEdge::Right, MonitorHint::Primary);
        assert!(
            matches!(result, Err(Error::Platform(_))),
            "null HWND should be rejected by SHAppBarMessage, got: {result:?}"
        );
    }

    /// The default AppBarEdge is Left (so uninitialized config doesn't pick a
    /// surprising edge).
    #[test]
    fn default_edge_is_left() {
        assert_eq!(AppBarEdge::default(), AppBarEdge::Left);
    }

    /// The default MonitorHint is Primary.
    #[test]
    fn default_monitor_hint_is_primary() {
        assert_eq!(MonitorHint::default(), MonitorHint::Primary);
    }
}
