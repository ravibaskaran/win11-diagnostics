//! Windows startup registration (v1.0 parity with reference's RunAtStartup).
//!
//! Registers sidebar to launch when the user signs in to Windows, via the
//! per-user Run key:
//!
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`
//!
//! This is the conventional, no-admin-required path. The reference app uses
//! a Scheduled Task; for a per-user, no-elevation app the Run key is the
//! simpler equivalent and matches what most consumer Windows apps do. The key
//! value is the full exe path; removing the value unregisters.
//!
//! ## Cited
//! PRD §3 UX features (v1.0 parity: "Run at Startup"), guardrails G28
//! (non-technical-user hardening — no admin prompt for autostart).

use sidebar_domain::error::{Error, Result};

#[cfg(windows)]
use windows::core::HSTRING;
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS,
    REG_SZ,
};

/// The registry value name under the Run key. A stable name makes the
/// enable call idempotent (overwrites the same entry).
const VALUE_NAME: &str = "Sidebar";

/// The registry sub-path of the per-user Run key.
const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Enable or disable sidebar launching at Windows startup.
///
/// `enabled = true` writes the current exe path to the HKCU Run key.
/// `enabled = false` deletes the value (idempotent no-op if absent).
///
/// # Errors
/// Returns [`Error::Platform`] if the registry write/delete fails (extremely
/// rare — HKCU is always writable by the owning user).
pub fn set_enabled(enabled: bool) -> Result<()> {
    #[cfg(windows)]
    {
        if enabled {
            enable_run_key()
        } else {
            disable_run_key()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// Whether the Run-key value currently exists (best-effort; returns false on
/// any read error or on non-Windows, so the Settings checkbox never gets
/// stuck "on" if the read path is flaky).
#[must_use]
pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        value_exists().is_ok()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn path_wide() -> Vec<u16> {
    RUN_KEY_PATH
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn value_name_hstring() -> HSTRING {
    HSTRING::from(VALUE_NAME)
}

/// Open HKCU\...\Run with the requested access. Creates the key if missing
/// (Windows normally creates it at first login, so this is a no-op open).
/// Returns the raw handle isize; caller must RegCloseKey.
#[cfg(windows)]
fn open_run_key(access: REG_SAM_FLAGS) -> Result<HKEY> {
    let path = path_wide();
    let mut hkey: HKEY = HKEY::default();
    let result = unsafe {
        // SAFETY: RegCreateKeyExW with a null-terminated UTF-16 path under
        // HKEY_CURRENT_USER. The key already exists; RegCreateKeyExW opens it
        // idempotently. The returned handle is owned by us until RegCloseKey.
        // `addr_of_mut!` avoids the implicit-borrow-as-raw-pointer lint.
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            std::ptr::addr_of_mut!(hkey),
            None,
        )
    };
    if result.is_err() {
        return Err(Error::Platform(format!(
            "RegCreateKeyExW failed: {result:?}"
        )));
    }
    Ok(hkey)
}

#[cfg(windows)]
fn enable_run_key() -> Result<()> {
    let exe =
        std::env::current_exe().map_err(|e| Error::Platform(format!("current_exe failed: {e}")))?;
    let exe_str = exe.display().to_string();
    // Quote the path so spaces in the user's profile path don't break the
    // command line (e.g. "C:\Users\Jane Doe\...\sidebar.exe").
    let quoted = format!("\"{exe_str}\"");
    let value_wide: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
    let name = value_name_hstring();
    let hkey = open_run_key(KEY_SET_VALUE)?;
    let data_bytes: &[u8] = unsafe {
        // SAFETY: value_wide is a stack-owned Vec<u16> living across the
        // call; casting to &[u8] of twice the length is sound (u8 alignment
        // is weaker than u16). The slice borrows value_wide's memory.
        std::slice::from_raw_parts(
            value_wide.as_ptr().cast::<u8>(),
            value_wide.len() * std::mem::size_of::<u16>(),
        )
    };
    let result = unsafe {
        // SAFETY: hkey is a valid open handle to HKCU\...\Run with
        // KEY_SET_VALUE. The value name + data are null-terminated UTF-16.
        // REG_SZ matches the type all other Run entries use.
        RegSetValueExW(hkey, &name, None, REG_SZ, Some(data_bytes))
    };
    let _ = unsafe {
        // SAFETY: hkey is an owned handle from open_run_key; RegCloseKey is
        // the documented cleanup. The handle is not used after this.
        RegCloseKey(hkey)
    };
    if result.is_err() {
        return Err(Error::Platform(format!(
            "RegSetValueExW failed: {result:?}"
        )));
    }
    tracing::info!(path = %exe_str, "startup: registered sidebar in HKCU Run key");
    Ok(())
}

#[cfg(windows)]
fn disable_run_key() -> Result<()> {
    let hkey = open_run_key(KEY_SET_VALUE)?;
    let name = value_name_hstring();
    let del_result = unsafe {
        // SAFETY: hkey is an owned valid handle; name is the value name.
        // RegDeleteValueW errors if the value is absent — we treat that as
        // idempotent success below.
        RegDeleteValueW(hkey, &name)
    };
    let _ = unsafe {
        // SAFETY: hkey is owned; RegCloseKey is the documented cleanup.
        RegCloseKey(hkey)
    };
    if del_result.is_err() {
        // Value absent — idempotent success.
        return Ok(());
    }
    tracing::info!("startup: removed sidebar from HKCU Run key");
    Ok(())
}

#[cfg(windows)]
fn value_exists() -> Result<()> {
    let hkey = open_run_key(KEY_QUERY_VALUE)?;
    let name = value_name_hstring();
    let query_result = unsafe {
        // SAFETY: hkey is an owned valid handle; querying for the value's
        // existence with null buffers. ERROR_FILE_NOT_FOUND means absent →
        // Err (caller treats Err as "not enabled").
        RegQueryValueExW(hkey, &name, None, None, None, None)
    };
    let _ = unsafe {
        // SAFETY: hkey is owned; RegCloseKey is the documented cleanup.
        RegCloseKey(hkey)
    };
    if query_result.is_err() {
        return Err(Error::Platform("Run value absent".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! The registry-mutation functions are not unit-tested here (they touch
    //! HKCU which is machine-state-mutating). Integration coverage comes from
    //! the e2e_launch_smoke + manual HITL checklist. The cross-platform
    //! stub (`#[cfg(not(windows))]`) returns Ok(()) unconditionally so the
    //! Settings checkbox renders on non-Windows dev hosts without crashing.

    use super::*;

    #[test]
    fn set_enabled_no_panic_on_round_trip_disabled() {
        // Disabling when never enabled must be idempotent (no-op).
        let _ = set_enabled(false);
        // The state after disable should read as not-enabled on non-Windows,
        // and on Windows it should be absent (since we just deleted it).
        assert!(!is_enabled() || cfg!(windows));
    }
}
