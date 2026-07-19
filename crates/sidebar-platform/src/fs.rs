//! Cross-file-atomic-write helper (v1.0 audit Iteration 1 — correctness sweep).
//!
//! `std::fs::write` on Windows is `CreateFile(GENERIC_WRITE, TRUNCATE_EXISTING)`
//! followed by `WriteFile`s — it truncates the destination first, then streams
//! the new contents. A crash (Windows update reboot, OOM kill, antivirus
//! quarantine mid-write, power loss) leaves the destination truncated/corrupt.
//!
//! `atomic_write` writes to a sibling temp file then renames it over the
//! destination. On NTFS the rename is atomic via `MoveFileEx(
//! MOVEFILE_REPLACE_EXISTING)` (which `std::fs::rename` invokes under the
//! hood), so a crash at any point leaves either the old or the new file —
//! never a half-written hybrid.
//!
//! Used by:
//! - the app's own `config.toml` (sidebar-app/src/gui/mod.rs::atomic_write_config)
//! - the LHM user config patcher (sidebar-platform/src/ohm_supervisor.rs),
//!   which writes to a third-party app's 100KB+ settings file and MUST NOT
//!   corrupt it on crash
//!
//! Cited: repo invariant "Config writes are atomic (temp + rename)".

use std::path::Path;

/// Write `contents` to `path` atomically: temp file in the same directory,
/// then rename over the destination. On success, no temp file remains.
/// On failure, the destination is untouched and the temp file is removed
/// best-effort.
///
/// The temp file is named `<filename>.<ext>.tmp` so it sits in the same
/// directory as the destination (NTFS rename within a volume is atomic;
/// cross-volume rename would silently become copy+delete and lose the
/// atomicity guarantee).
///
/// # Errors
///
/// Returns the underlying `std::io::Error` from either the temp write or
/// the rename. The caller decides whether to log + continue (G15) or
/// surface to the user.
pub fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = tmp_sibling(path);
    if let Err(e) = std::fs::write(&tmp, contents) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Build the temp-file path: append `.tmp` to the full filename. This
/// keeps the temp file in the same directory + same volume as the target
/// (NTFS intra-volume rename is atomic; cross-volume is not).
fn tmp_sibling(path: &Path) -> std::path::PathBuf {
    // Append ".tmp" to the filename. `with_extension` would REPLACE the
    // extension, which is wrong for files like `LibreHardwareMonitor.config`
    // (becomes `LibreHardwareMonitor.tmp` — different file, not a sibling).
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from("atomic.tmp"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    //! v1.0 audit Iteration 1 — atomic_write contract.
    //!
    //! The bug (ohm_supervisor::patch_lhm_user_config): std::fs::write
    //! truncates first, so a crash mid-write corrupts the destination.
    //! The fix uses temp+rename; these tests pin the contract.

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("out.config");
        atomic_write(&target, "<x/>").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "<x/>");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("out.config");
        std::fs::write(&target, "OLD").unwrap();
        atomic_write(&target, "NEW").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "NEW");
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_on_success() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("out.config");
        atomic_write(&target, "<x/>").unwrap();
        let tmp = tmp_sibling(&target);
        assert!(!tmp.exists(), "temp file {tmp:?} must not linger");
    }

    #[test]
    fn tmp_sibling_preserves_extension() {
        // `with_extension` would turn `LibreHardwareMonitor.config` into
        // `LibreHardwareMonitor.tmp` — wrong file. Appending `.tmp` keeps
        // the original filename intact.
        let p = Path::new("C:/resources/LibreHardwareMonitor.config");
        let tmp = tmp_sibling(p);
        assert_eq!(
            tmp.file_name().unwrap(),
            std::ffi::OsStr::new("LibreHardwareMonitor.config.tmp")
        );
    }

    #[test]
    fn tmp_sibling_lives_in_same_directory() {
        // Same-directory = same-volume on NTFS = atomic rename. Cross-volume
        // rename silently degrades to copy+delete.
        let p = Path::new("C:/resources/LibreHardwareMonitor.config");
        let tmp = tmp_sibling(p);
        assert_eq!(tmp.parent(), p.parent());
    }
}
