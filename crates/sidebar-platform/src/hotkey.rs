//! Global hotkey registration and click-through toggling (Story 6.6/T-34).

use sidebar_domain::error::{Error, Result};
use windows::Win32::Foundation::{GetLastError, SetLastError, HWND, WIN32_ERROR};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOP, SET_WINDOW_POS_FLAGS, SWP_FRAMECHANGED,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_HOTKEY, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
};

/// Parsed global shortcut. The key is a Windows virtual-key code.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyCombo {
    /// Control modifier.
    pub ctrl: bool,
    /// Shift modifier.
    pub shift: bool,
    /// Alt modifier.
    pub alt: bool,
    /// Windows-key modifier.
    pub win: bool,
    /// Virtual-key code for the non-modifier key.
    pub key: u32,
}

impl HotkeyCombo {
    /// Parse a compact config string such as `Ctrl+Shift+S`.
    pub fn parse(input: &str) -> Result<Self> {
        let mut combo = Self {
            ctrl: false,
            shift: false,
            alt: false,
            win: false,
            key: 0,
        };
        for token in input.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" if !combo.ctrl => combo.ctrl = true,
                "shift" if !combo.shift => combo.shift = true,
                "alt" if !combo.alt => combo.alt = true,
                "win" | "windows" | "meta" if !combo.win => combo.win = true,
                key if combo.key == 0 => {
                    combo.key = parse_key(key).ok_or_else(|| {
                        Error::Config(format!("unsupported global hotkey key: {token}"))
                    })?;
                }
                _ => return Err(Error::Config(format!("invalid global hotkey: {input}"))),
            }
        }
        if combo.key == 0 || !(combo.ctrl || combo.shift || combo.alt || combo.win) {
            return Err(Error::Config(format!("invalid global hotkey: {input}")));
        }
        Ok(combo)
    }

    /// Return the Win32 modifier bit mask.
    #[must_use]
    pub const fn modifier_mask(self) -> u32 {
        (if self.ctrl { MOD_CONTROL.0 } else { 0 })
            | (if self.shift { MOD_SHIFT.0 } else { 0 })
            | (if self.alt { MOD_ALT.0 } else { 0 })
            | (if self.win { MOD_WIN.0 } else { 0 })
            | MOD_NOREPEAT.0
    }
}

fn parse_key(token: &str) -> Option<u32> {
    if token.len() == 1 {
        let byte = token.as_bytes()[0].to_ascii_uppercase();
        if byte.is_ascii_alphanumeric() {
            return Some(u32::from(byte));
        }
    }
    token
        .strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=24).contains(n))
        .map(|n| 0x70 + n - 1)
}

/// Register a shortcut against the caller's window message queue.
pub fn register(hwnd: HWND, id: i32, combo: HotkeyCombo) -> Result<()> {
    // SAFETY: the caller owns the HWND and keeps it alive for the registration.
    unsafe {
        RegisterHotKey(
            Some(hwnd),
            id,
            HOT_KEY_MODIFIERS(combo.modifier_mask()),
            combo.key,
        )
    }
    .map_err(|error| {
        tracing::warn!(id, ?combo, %error, "global hotkey registration failed");
        Error::Platform(format!("RegisterHotKey failed: {error}"))
    })
}

/// Unregister a previously registered shortcut. Missing registrations are harmless.
pub fn unregister(hwnd: HWND, id: i32) -> Result<()> {
    // SAFETY: the HWND and id are the same values supplied to `register`.
    unsafe { UnregisterHotKey(Some(hwnd), id) }
        .map_err(|error| Error::Platform(format!("UnregisterHotKey failed: {error}")))
}

/// Toggle the extended window style used for click-through mode.
pub fn set_click_through(hwnd: HWND, enabled: bool) -> Result<()> {
    if hwnd.is_invalid() {
        return Err(Error::Platform("click-through requires a live HWND".into()));
    }
    // SAFETY: `hwnd` is validated above and GWL_EXSTYLE is an integer style slot.
    let current =
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let mut style = u32::try_from(current).unwrap_or_default();
    let click_bits = WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
    if enabled {
        style |= click_bits;
    } else {
        style &= !click_bits;
    }
    // SAFETY: `hwnd` is validated above; GWL_EXSTYLE is a documented integer
    // style slot and no borrowed memory crosses the call. Win32 reports
    // SetWindowLongPtrW failure as a zero previous value, so clear and check
    // last-error to distinguish that from a legitimate zero style.
    let (previous, last_error) = unsafe {
        SetLastError(WIN32_ERROR(0));
        let previous = SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            isize::try_from(style).unwrap_or(isize::MAX),
        );
        (previous, GetLastError().0)
    };
    if window_long_update_failed(previous, last_error) {
        return Err(Error::Platform(format!(
            "SetWindowLongPtrW failed: {last_error}"
        )));
    }
    // SAFETY: `hwnd` remains valid for the duration of this synchronous style
    // refresh and the flags only request a non-moving frame update.
    unsafe {
        let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED;
        SetWindowPos(hwnd, Some(HWND_TOP), 0, 0, 0, 0, flags)
            .map_err(|error| Error::Platform(format!("SetWindowPos style refresh failed: {error}")))
    }
}

#[must_use]
const fn window_long_update_failed(previous: isize, last_error: u32) -> bool {
    previous == 0 && last_error != 0
}

/// Decode a Windows message into a registered hotkey id.
#[must_use]
pub fn hotkey_id_from_message(message: u32, wparam: usize) -> Option<i32> {
    if message == WM_HOTKEY {
        i32::try_from(wparam).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_click_through_hotkey() {
        let combo = HotkeyCombo::parse("Ctrl+Shift+S").expect("default hotkey");
        assert!(combo.ctrl);
        assert!(combo.shift);
        assert!(!combo.alt);
        assert_eq!(combo.key, 'S' as u32);
    }

    #[test]
    fn rejects_unknown_tokens_and_missing_key() {
        assert!(HotkeyCombo::parse("Foo+Bar").is_err());
        assert!(HotkeyCombo::parse("Ctrl+Shift").is_err());
    }

    #[test]
    fn modifier_mask_matches_ctrl_shift() {
        let combo = HotkeyCombo::parse("Ctrl+Shift+S").unwrap();
        assert_eq!(combo.modifier_mask(), 2 | 4 | MOD_NOREPEAT.0);
    }

    #[test]
    fn hotkey_message_id_is_decoded() {
        use windows::Win32::UI::WindowsAndMessaging::{WM_HOTKEY, WM_SETTINGCHANGE};
        assert_eq!(hotkey_id_from_message(WM_HOTKEY, 17), Some(17));
        assert_eq!(hotkey_id_from_message(WM_SETTINGCHANGE, 17), None);
    }

    #[test]
    fn zero_previous_value_without_last_error_is_success() {
        assert!(!window_long_update_failed(0, 0));
        assert!(window_long_update_failed(0, 5));
        assert!(!window_long_update_failed(1, 5));
    }
}
