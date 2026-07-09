//! SQLite schema initialization for the bandwidth state store.
//!
//! Story 4.1 — `init()` creates the two tables (`current_cycle`,
//! `bandwidth_history`) defined in architecture.md AD-11, sets the
//! `user_version = 1` / `journal_mode = WAL` / `foreign_keys = ON` PRAGMAs,
//! and is idempotent (safe to call repeatedly).
//!
//! Cited:
//!   - architecture.md AD-11 (CREATE TABLE SQL block — authoritative)
//!   - nfr-thresholds.md T-6 (bundled sqlite ≤ 3 MiB RSS contribution)
//!   - nfr-thresholds.md T-17 (WAL autocheckpoint = SQLite default; do NOT override)
//!   - nfr-thresholds.md T-26 (LUID is 64-bit → stored as INTEGER / i64)
//!   - guardrails.md G21 (all SQLite access funnels through sidebar-persistence)

use rusqlite::Connection;
use sidebar_domain::error::Result;

/// Initialize the bandwidth-state schema on `conn`.
///
/// Creates `current_cycle` + `bandwidth_history` (per AD-11) and sets the
/// `user_version = 1`, `journal_mode = WAL`, `foreign_keys = ON` PRAGMAs.
/// Idempotent — uses `CREATE TABLE IF NOT EXISTS` and re-asserts PRAGMAs.
///
/// # Errors
///
/// Returns `sidebar_domain::Error::Sqlite` if any SQLite statement fails
/// (e.g. corrupt file at the path, read-only filesystem).
///
/// # Panics
///
/// None — this function never panics.
pub fn init(_conn: &Connection) -> Result<()> {
    // RED stub — empty body returns Ok(()). Tests below assert PRAGMAs and
    // tables are set; they fail because the stub does nothing. GREEN lands
    // in the next commit.
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Story 4.1 TDD contract tests.
    //!
    //! Three happy-path tests + three boundary tests. Cited:
    //!   - architecture.md AD-11 (table + PRAGMA spec)
    //!   - nfr-thresholds.md T-17 (default wal_autocheckpoint)
    //!   - nfr-thresholds.md T-26 (adapter_luid stored as INTEGER / i64)
    //!   - fixture F1 (TempDir)
    //!   - fixture F6 (idempotency)

    use super::init;
    use rusqlite::Connection;
    use sidebar_domain::error::Error;
    use tempfile::TempDir;

    /// Helper: open a fresh SQLite file inside a TempDir (fixture F1) and
    /// hand back `(Connection, TempDir)` — the TempDir must outlive the
    /// connection, so the caller binds both.
    fn open_temp() -> (Connection, TempDir) {
        let dir = TempDir::new().expect("TempDir::new must succeed");
        let path = dir.path().join("bandwidth.db");
        let conn = Connection::open(&path).unwrap_or_else(|e| panic!("open must succeed: {e}"));
        (conn, dir)
    }

    // -----------------------------------------------------------------
    // Happy Path #1 — user_version is set to 1 (RED: stub leaves it 0).
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Happy Path #1, AD-11, fixture F1.
    #[test]
    fn init_sets_user_version_to_1() {
        let (conn, _dir) = open_temp();
        init(&conn).expect("init must succeed on a fresh DB");
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("PRAGMA user_version must query");
        assert_eq!(
            user_version, 1,
            "user_version MUST be 1 after init (AD-11); got {user_version}"
        );
    }

    // -----------------------------------------------------------------
    // Happy Path #2 — journal_mode is "wal" (RED: stub leaves it "delete").
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Happy Path #2, AD-11, T-6.
    #[test]
    fn init_sets_journal_mode_to_wal() {
        let (conn, _dir) = open_temp();
        init(&conn).expect("init must succeed on a fresh DB");
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("PRAGMA journal_mode must query");
        assert_eq!(
            journal_mode.to_lowercase(),
            "wal",
            "journal_mode MUST be 'wal' after init (AD-11); got '{journal_mode}'"
        );
    }

    // -----------------------------------------------------------------
    // Happy Path #3 — init is idempotent (F6). RED: passes trivially
    // because stub returns Ok(()) twice; but the assertions on user_version
    // / journal_mode after the second call are what we're really proving.
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Happy Path #3, fixture F6.
    #[test]
    fn init_is_idempotent() {
        let (conn, _dir) = open_temp();
        init(&conn).expect("first init must succeed");
        init(&conn).expect("second init MUST succeed (F6 idempotency)");
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("PRAGMA user_version must query after second init");
        assert_eq!(user_version, 1, "user_version still 1 after second init");
    }

    // -----------------------------------------------------------------
    // Boundary #1 — a corrupt / non-SQLite file at the path makes open()
    // itself fail; init surfaces the error rather than overwriting it.
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Boundary #1.
    #[test]
    fn init_surfaces_error_on_corrupt_file() {
        let dir = TempDir::new().expect("TempDir::new");
        let path = dir.path().join("not-a-db.db");
        // Write garbage — NOT a valid SQLite header.
        std::fs::write(&path, b"this is definitely not a sqlite database file")
            .expect("write garbage");
        // Opening a non-SQLite file succeeds (SQLite opens anything), but
        // subsequent PRAGMA/query will fail. Call init on it and assert Err.
        let conn = Connection::open(&path).expect("open succeeds even on garbage");
        // init() on a corrupt file: PRAGMA journal_mode=WAL on a non-DB
        // file returns an error → surfaced as Error::Sqlite.
        let result = init(&conn);
        assert!(
            result.is_err(),
            "init on a corrupt file MUST return Err, got Ok"
        );
        // Confirm the file was NOT overwritten with a fresh DB: garbage
        // bytes should still be present (we did not truncate/recreate).
        let bytes = std::fs::read(&path).expect("read back");
        assert!(
            bytes.starts_with(b"this is definitely not"),
            "init MUST NOT overwrite a corrupt file; first bytes preserved"
        );
    }

    // -----------------------------------------------------------------
    // Boundary #2 — read-only filesystem: open() fails, error surfaces.
    // On Windows `Connection::open` on a read-only parent dir fails with
    // a disk I/O error; we assert init never succeeds against an
    // unwritable path. We simulate by pointing at a path inside a
    // read-only directory.
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Boundary #2.
    #[test]
    fn init_surfaces_error_on_readonly_path() {
        let dir = TempDir::new().expect("TempDir::new");
        // Make the parent directory read-only. On Unix the readonly bit on
        // the directory denies file creation inside it; on Windows the
        // readonly bit on a directory is effectively a no-op for creation
        // (ACLs govern), so this test is most meaningful on Unix but runs
        // everywhere — the assertion is that the pipeline surfaces SOME
        // error when the path can't be written.
        let mut perms = std::fs::metadata(dir.path())
            .expect("metadata")
            .permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(dir.path(), perms).expect("set readonly");
        let path = dir.path().join("subdir").join("bandwidth.db");
        // open() should fail → we assert that this surfaces as an error
        // somewhere in the pipeline (open OR init).
        let open_result = Connection::open(&path);
        let result = match open_result {
            Ok(conn) => init(&conn),
            Err(e) => Err(Error::Sqlite(e.to_string())),
        };
        assert!(
            result.is_err(),
            "open+init on a read-only path MUST return Err"
        );
        // Restore perms so TempDir::drop can clean up. Unix-only: Windows
        // readonly bit is harmless for cleanup (TempDir::drop uses
        // RemoveDirectoryW which ignores the readonly attribute).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let restored = std::fs::Permissions::from_mode(0o755);
            let _ = std::fs::set_permissions(dir.path(), restored);
        }
    }

    // -----------------------------------------------------------------
    // Boundary #3 — adapter_luid is INTEGER / i64; u64::MAX round-trips
    // via reinterpret cast (T-26). RED: the stub doesn't create the table,
    // so the INSERT fails.
    // -----------------------------------------------------------------
    /// Cited: Story 4.1 TDD contract Boundary #3, T-26, AD-11.
    #[test]
    fn adapter_luid_stores_u64_max_as_i64() {
        let (conn, _dir) = open_temp();
        init(&conn).expect("init must succeed before insert");
        // LUID is 64-bit (MIB_IF_ROW2.InterfaceLuid). u64::MAX reinterpreted
        // as i64 is -1; SQLite stores INTEGER as i64 so this round-trips.
        let luid_max: u64 = u64::MAX;
        // Intentional reinterpret cast (T-26: "store as i64 reinterpret-cast").
        let luid_signed: i64 = luid_max.cast_signed();
        conn.execute(
            "INSERT INTO current_cycle \
             (adapter_luid, adapter_name, cycle_start, rx_bytes, tx_bytes, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                luid_signed,
                "Ethernet",
                "2026-07-01",
                1_234_567_i64,
                7_654_321_i64,
                "2026-07-01T12:00:00",
            ],
        )
        .expect("insert u64::MAX-reinterpreted LUID must succeed");
        let read_back: i64 = conn
            .query_row(
                "SELECT adapter_luid FROM current_cycle WHERE adapter_luid = ?1",
                rusqlite::params![luid_signed],
                |row| row.get(0),
            )
            .expect("read back must succeed");
        assert_eq!(
            read_back, luid_signed,
            "LUID (u64::MAX as i64) MUST round-trip as i64 (T-26)"
        );
        // And the u64 reinterpret confirms the value.
        assert_eq!(read_back.cast_unsigned(), u64::MAX);
    }
}
