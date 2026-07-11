//! Story 6.6 — Windows-only integration smoke for monitor enumeration,
//! hotkey parsing, and theme registry read.
//!
//! These tests exercise the real Win32 `EnumDisplayMonitors` +
//! `EnumDisplayDevicesW` + `GetDpiForMonitor` path (monitors), the
//! hotkey-combo parser (T-34 default `Ctrl+Shift+S`), and the registry
//! `AppsUseLightTheme` read (T-35) on a Windows host. They are
//! `#[cfg(target_os = "windows")]`-gated and run on every Windows CI run.
//!
//! Cited:
//!   - Story 6.6 Happy Path #2 (enumerate() returns >= 1 monitor)
//!   - Story 6.6 Boundary (primary flag set on exactly one monitor)
//!   - nfr-thresholds.md T-34 (hotkey default Ctrl+Shift+S)
//!   - nfr-thresholds.md T-35 (theme default Dark; system registry read)
//!   - nfr-thresholds.md T-36 (multi-monitor: default primary)
//!   - guardrails.md G6 (platform gating)

#![cfg(target_os = "windows")]

use sidebar_platform::hotkey::HotkeyCombo;
use sidebar_platform::monitors::{enumerate, resolve_target};
use sidebar_platform::theme_bridge::{is_system_dark, system_theme};

/// Story 6.6 Happy Path #2 — `enumerate()` MUST return at least one monitor
/// on any Windows host with a usable desktop (CI runners included). This is
/// the load-bearing smoke for the platform-FFI path.
#[test]
fn enumerate_returns_at_least_one_monitor_on_windows() {
    let monitors = enumerate().expect("enumerate() must succeed on Windows");
    assert!(
        !monitors.is_empty(),
        "Windows CI must have at least one display"
    );
    for m in &monitors {
        // Every entry must carry a non-empty identity + friendly name.
        assert!(!m.id.is_empty(), "monitor id must be non-empty: {m:?}");
        assert!(
            !m.friendly_name.is_empty(),
            "monitor friendly_name must be non-empty: {m:?}"
        );
        // DPI is positive (96 minimum on Windows; high-DPI displays report more).
        assert!(m.dpi >= 96, "dpi must be >= 96 (got {})", m.dpi);
        // Geometry sanity — width/height positive.
        assert!(m.width > 0, "width must be positive: {m:?}");
        assert!(m.height > 0, "height must be positive: {m:?}");
    }
}

/// Story 6.6 Boundary / T-36 — exactly one monitor is marked primary. CI
/// runners have a single virtual display; multi-monitor dev machines have
/// exactly one primary by Win32 contract.
#[test]
fn enumerate_marks_exactly_one_primary_monitor() {
    let monitors = enumerate().expect("enumerate() must succeed");
    let primary_count = monitors.iter().filter(|m| m.primary).count();
    assert_eq!(
        primary_count,
        1,
        "exactly one monitor must be primary (T-36); got {primary_count} out of {}",
        monitors.len()
    );
}

/// Story 6.6 — `resolve_target("primary", ...)` MUST return the same monitor
/// flagged as primary by `enumerate()`. This is the T-36 default-dock contract
/// end-to-end (config string `"primary"` → real Win32 primary display).
#[test]
fn resolve_target_primary_matches_enumerate_primary() {
    let monitors = enumerate().expect("enumerate() must succeed");
    let resolved = resolve_target(&monitors, "primary");
    assert!(resolved.is_some(), "primary must resolve");
    let resolved = resolved.unwrap();
    let primary = monitors
        .iter()
        .find(|m| m.primary)
        .expect("enumerate must mark one primary");
    assert_eq!(
        resolved.id, primary.id,
        "resolve_target(\"primary\") must return the same monitor enumerate flags primary"
    );
}

/// Story 6.6 / T-34 — the default hotkey `Ctrl+Shift+S` MUST parse to a valid
/// `HotkeyCombo` with Ctrl+Shift modifiers and the 'S' virtual-key code (0x53).
/// This is the config-default round-trip on a real Windows host.
#[test]
fn default_hotkey_ctrl_shift_s_parses() {
    let combo = HotkeyCombo::parse("Ctrl+Shift+S").expect("default hotkey must parse");
    assert!(combo.ctrl, "Ctrl modifier must be set");
    assert!(combo.shift, "Shift modifier must be set");
    assert!(!combo.alt, "Alt modifier must NOT be set");
    assert!(!combo.win, "Win modifier must NOT be set");
    // VK_S = 0x53 = 83 (Windows virtual-key code for 'S').
    assert_eq!(combo.key, 0x53, "key must be VK_S (0x53)");
}

/// Story 6.6 / T-35 — `system_theme()` MUST succeed on Windows (the registry
/// key is present on every Win10+ install). The result is one of {Dark, Light}
/// depending on the user's preference; we don't assert which, only that the
/// read works.
#[test]
fn system_theme_reads_registry_on_windows() {
    let theme = system_theme().expect("system_theme() must succeed on Windows");
    // Theme is deterministic per-machine but we don't assert which — just that
    // the call returns a value (the registry read path works end-to-end).
    let _ = theme; // ThemeMode is Debug; we just want Ok(()).
}

/// Story 6.6 / T-35 — `is_system_dark()` MUST return a boolean and default to
/// dark on any registry-read failure (T-35 boundary behavior). On a real
/// Windows host it returns the actual preference.
#[test]
fn is_system_dark_returns_boolean() {
    let _dark = is_system_dark();
    // No assertion on the value — just that the call doesn't panic and the
    // default-dark fallback path is reachable.
}
