//! `sidebar-persistence` — SQLite-backed bandwidth state store (AD-11, Stories 4.1-4.3).
//!
//! Owns all SQLite access in the workspace (guardrail G21). Story 4.1
//! delivers schema initialization ([`schema::init`]); Story 4.2 adds the
//! read/write/rollover primitives ([`bandwidth_repo`]); Story 4.3 will add
//! the `user_version` migration module.

pub mod bandwidth_repo;
pub mod migrate;
pub mod schema;

use sidebar_domain::error::{Error, Result};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Quarantine a corrupt `bandwidth.db` + reopen a fresh, schema-initialized
/// connection at the same path. Used by the accountant thread
/// (`sidebar_app::main::run_accountant_on_thread`) when `schema::init`
/// fails on an existing corrupt file — instead of permanently disabling
/// bandwidth tracking, the corrupt file is renamed aside for forensics
/// and a fresh DB is created in its place.
///
/// Steps:
/// 1. Rename `<path>` → `<path>.corrupt-<unix_timestamp>` (best-effort;
///    if the rename fails, the error surfaces and the caller disables the
///    accountant per G15 — no crash).
/// 2. Open a fresh `Connection` at `<path>` (SQLite creates a new file).
/// 3. Run `schema::init` on the fresh connection.
/// 4. Return the connection.
///
/// `schema::init`'s "must not overwrite a corrupt file" contract
/// (Boundary #1 in `schema.rs`) is preserved — `init` is never called on
/// the corrupt file; it is called only on the fresh post-rename connection.
///
/// Cited: Story 13.2, guardrails.md G15/G21/G28, tdd-fixtures.md F15.
///
/// # Errors
///
/// Returns [`Error::Io`] if the quarantine rename fails, or
/// [`Error::Sqlite`] if the fresh `Connection::open` or `schema::init`
/// fails (extremely unlikely on a clean path).
pub fn quarantine_and_reopen(db_path: &Path) -> Result<rusqlite::Connection> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backup = db_path.with_extension(format!("db.corrupt-{timestamp}"));
    std::fs::rename(db_path, &backup).map_err(|e| {
        tracing::warn!(
            original = %db_path.display(),
            backup = %backup.display(),
            error = %e,
            "quarantine rename failed (G15 — non-fatal, accountant will disable)"
        );
        Error::Io(format!("quarantine rename failed: {e}"))
    })?;
    tracing::warn!(
        original = %db_path.display(),
        backup = %backup.display(),
        "corrupt bandwidth.db quarantined (Story 13.2, G28) — reopening fresh"
    );
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| Error::Sqlite(format!("fresh open after quarantine failed: {e}")))?;
    schema::init(&conn)?;
    Ok(conn)
}

/// Story 0.1 smoke marker — proves the crate is reachable via `cargo test`.
///
/// Retained from the Story 0.1 stub so other crates that may probe for
/// presence continue to compile. Real functionality lives in [`schema`].
#[must_use]
pub fn crate_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::crate_present;

    /// Story 0.1 Happy Path #1. Cited: G17 (no empty stubs).
    #[test]
    fn crate_present_returns_true() {
        assert!(crate_present(), "crate_present() must return true");
    }

    /// Story 0.1 idempotency. Cited: fixture F6.
    #[test]
    fn crate_present_is_idempotent() {
        assert_eq!(crate_present(), crate_present());
    }
}
