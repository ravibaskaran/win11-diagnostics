//! Windows system-theme bridge (Story 6.6/T-35).

use sidebar_domain::error::{Error, Result};
use sidebar_domain::event::Event;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

const PERSONALIZE_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";

/// Resolved system preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    /// Windows reports AppsUseLightTheme = 0.
    Dark,
    /// Windows reports AppsUseLightTheme = 1.
    Light,
}

/// Read the current user's `AppsUseLightTheme` registry value.
pub fn system_theme() -> Result<ThemeMode> {
    let key = wide(PERSONALIZE_KEY);
    let value_name = wide("AppsUseLightTheme");
    let mut value = 0_u32;
    let mut size = u32::try_from(std::mem::size_of::<u32>()).unwrap_or(0);
    // SAFETY: all pointers reference stack-owned buffers that remain alive for
    // the synchronous registry call; the key/value strings are NUL-terminated.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(Error::Platform(format!(
            "RegGetValueW(AppsUseLightTheme) failed: {}",
            status.0
        )));
    }
    Ok(theme_mode_from_apps_use_light_theme(value))
}

/// Return the system preference, defaulting to dark when the registry value is
/// missing or malformed (T-35 boundary behavior).
#[must_use]
pub fn is_system_dark() -> bool {
    matches!(system_theme().unwrap_or(ThemeMode::Dark), ThemeMode::Dark)
}

/// Convert a `WM_SETTINGCHANGE` message into a theme event when applicable.
#[must_use]
pub fn theme_event_for_setting_change(message: u32, setting: &str) -> Option<Event> {
    if !is_theme_setting_change(message, setting) {
        return None;
    }
    Some(Event::ThemeChanged(if is_system_dark() {
        "dark".into()
    } else {
        "light".into()
    }))
}

/// Pure message filter used by the platform message seam and tests.
#[must_use]
pub fn is_theme_setting_change(message: u32, setting: &str) -> bool {
    message == WM_SETTINGCHANGE && setting.eq_ignore_ascii_case("ImmersiveColorSet")
}

/// Map the Win32 registry DWORD to the app's theme mode.
#[must_use]
pub const fn theme_mode_from_apps_use_light_theme(value: u32) -> ThemeMode {
    if value == 1 {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

/// Decode a `WM_SETTINGCHANGE` payload and emit a theme event when it names
/// `ImmersiveColorSet`. Windows owns the message string for the duration of
/// message dispatch; this function copies it before returning.
#[must_use]
pub fn theme_event_from_message(message: u32, lparam: LPARAM) -> Option<Event> {
    if message != WM_SETTINGCHANGE || lparam.0 == 0 {
        return None;
    }
    // SAFETY: WM_SETTINGCHANGE documents lParam as a valid NUL-terminated
    // UTF-16 string for the duration of dispatch. Bound the scan to avoid
    // walking untrusted memory if a malformed message reaches the seam.
    let name = unsafe {
        let ptr = lparam.0 as *const u16;
        let mut len = 0usize;
        while len < 256 && *ptr.add(len) != 0 {
            len += 1;
        }
        if len == 256 {
            return None;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    };
    theme_event_for_setting_change(message, &name)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_change_filter_accepts_immersive_color_set() {
        use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;
        assert!(is_theme_setting_change(
            WM_SETTINGCHANGE,
            "ImmersiveColorSet"
        ));
        assert!(!is_theme_setting_change(WM_SETTINGCHANGE, "Environment"));
        assert!(!is_theme_setting_change(
            WM_DISPLAYCHANGE,
            "ImmersiveColorSet"
        ));
    }

    #[test]
    fn registry_dword_maps_to_theme_mode() {
        assert_eq!(theme_mode_from_apps_use_light_theme(0), ThemeMode::Dark);
        assert_eq!(theme_mode_from_apps_use_light_theme(1), ThemeMode::Light);
        assert_eq!(theme_mode_from_apps_use_light_theme(9), ThemeMode::Dark);
    }
}
