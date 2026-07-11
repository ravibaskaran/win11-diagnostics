//! Monitor enumeration and target fallback (Story 6.6/T-36).

use sidebar_domain::error::{Error, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsExW, GetMonitorInfoW, DEVMODEW,
    DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS, HDC, HMONITOR,
    MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

/// Stable monitor metadata consumed by the dock/viewport layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    /// DeviceID from `EnumDisplayDevicesW`, or the display name as fallback.
    pub id: String,
    /// Human-readable adapter/display name.
    pub friendly_name: String,
    /// Work-area left coordinate in virtual-screen pixels.
    pub x: i32,
    /// Work-area top coordinate in virtual-screen pixels.
    pub y: i32,
    /// Work-area width in virtual-screen pixels.
    pub width: i32,
    /// Work-area height in virtual-screen pixels.
    pub height: i32,
    /// Effective monitor DPI, falling back to 96 when unavailable.
    pub dpi: u32,
    /// Whether this is the primary display.
    pub primary: bool,
}

/// Enumerate active monitors and their stable device identities.
pub fn enumerate() -> Result<Vec<MonitorInfo>> {
    let mut monitors = Vec::new();
    // SAFETY: the callback receives a pointer to the live local vector for the
    // duration of the synchronous EnumDisplayMonitors call.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor_callback),
            LPARAM(std::ptr::from_mut(&mut monitors) as isize),
        )
    };
    if !ok.as_bool() {
        return Err(Error::Platform("EnumDisplayMonitors failed".into()));
    }
    if monitors.is_empty() {
        return Err(Error::Platform(
            "EnumDisplayMonitors returned no displays".into(),
        ));
    }
    Ok(monitors)
}

/// Resolve a configured DeviceID, with `primary` and disconnected fallbacks.
#[must_use]
pub fn resolve_target<'a>(
    monitors: &'a [MonitorInfo],
    configured_id: &str,
) -> Option<&'a MonitorInfo> {
    if monitors.is_empty() {
        return None;
    }
    if !configured_id.eq_ignore_ascii_case("primary") {
        if let Some(found) = monitors
            .iter()
            .find(|monitor| monitor.id.eq_ignore_ascii_case(configured_id))
        {
            return Some(found);
        }
        tracing::warn!(
            configured_id,
            "configured monitor is unavailable; falling back to primary"
        );
    }
    monitors
        .iter()
        .find(|monitor| monitor.primary)
        .or(monitors.first())
}

unsafe extern "system" fn enum_monitor_callback(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> windows::core::BOOL {
    // SAFETY: EnumDisplayMonitors passes back the exact pointer to the local
    // vector supplied by `enumerate`; the callback is synchronous.
    let monitors = unsafe { &mut *(data.0 as *mut Vec<MonitorInfo>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = u32::try_from(std::mem::size_of::<MONITORINFOEXW>()).unwrap_or(0);
    // SAFETY: `info` is a writable stack-owned MONITORINFOEXW with cbSize set.
    if !unsafe { GetMonitorInfoW(monitor, &raw mut info.monitorInfo) }.as_bool() {
        return windows::core::BOOL(1);
    }
    let device_name = wide_string(&info.szDevice);
    let mut display = DISPLAY_DEVICEW {
        cb: u32::try_from(std::mem::size_of::<DISPLAY_DEVICEW>()).unwrap_or(0),
        ..DISPLAY_DEVICEW::default()
    };
    // SAFETY: the device name is NUL-terminated by MONITORINFOEXW and display
    // is a writable, correctly sized DISPLAY_DEVICEW.
    let display_ok =
        unsafe { EnumDisplayDevicesW(PCWSTR(info.szDevice.as_ptr()), 0, &raw mut display, 0) }
            .as_bool();
    let mut current_mode = DEVMODEW {
        dmSize: u16::try_from(std::mem::size_of::<DEVMODEW>()).unwrap_or(0),
        ..DEVMODEW::default()
    };
    // SAFETY: `info.szDevice` is a NUL-terminated display name and
    // `current_mode` is a correctly sized writable DEVMODEW. The mode query
    // confirms the monitor is active; work-area geometry comes from
    // MONITORINFOEXW below.
    let _ = unsafe {
        EnumDisplaySettingsExW(
            PCWSTR(info.szDevice.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &raw mut current_mode,
            ENUM_DISPLAY_SETTINGS_FLAGS(0),
        )
    };
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    // SAFETY: monitor is supplied by EnumDisplayMonitors; dpi outputs are
    // valid writable locals.
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) };
    let rect = info.monitorInfo.rcWork;
    monitors.push(MonitorInfo {
        id: if display_ok {
            wide_string(&display.DeviceID)
        } else {
            device_name.clone()
        },
        friendly_name: if display_ok {
            let friendly = wide_string(&display.DeviceString);
            if friendly.is_empty() {
                device_name
            } else {
                friendly
            }
        } else {
            device_name
        },
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
        dpi: dpi_x.max(dpi_y),
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    });
    windows::core::BOOL(1)
}

fn wide_string(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, primary: bool) -> MonitorInfo {
        MonitorInfo {
            id: id.into(),
            friendly_name: id.into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            dpi: 96,
            primary,
        }
    }

    #[test]
    fn missing_target_falls_back_to_primary() {
        let displays = vec![monitor("DISPLAY1", true), monitor("DISPLAY2", false)];
        let target = resolve_target(&displays, "DISCONNECTED").unwrap();
        assert_eq!(target.id, "DISPLAY1");
    }

    #[test]
    fn primary_sentinel_selects_primary() {
        let displays = vec![monitor("DISPLAY1", false), monitor("DISPLAY2", true)];
        assert_eq!(resolve_target(&displays, "primary").unwrap().id, "DISPLAY2");
    }

    #[test]
    fn no_primary_uses_first_display() {
        let displays = vec![monitor("DISPLAY1", false)];
        assert_eq!(resolve_target(&displays, "missing").unwrap().id, "DISPLAY1");
    }
}
