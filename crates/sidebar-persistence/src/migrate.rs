//! Schema migration via the `user_version` PRAGMA (AD-11, Story 4.3).
//!
//! `migrate(conn)` reads the SQLite `user_version` PRAGMA and applies the
//! sequence of registered migrations (each in its own transaction) to bring
//! the schema up to the latest known version. The registry is an explicit,
//! ordered slice of `fn(&Connection) -> Result<()>` indexed by *target*
//! version — i.e. `MIGRATIONS[0]` is the v0→v1 step.
//!
//! ## Relationship to [`crate::schema::init`]
//!
//! Story 4.1's [`crate::schema::init()`] creates the v1 tables and *also*
//! stamps `user_version = 1`. The v0→v1 migration step therefore runs the
//! *same* DDL — it does not call `schema::init()` directly, because:
//!   1. `schema::init()` sets `journal_mode = WAL` via a separate statement
//!      that cannot run inside a migration transaction (PRAGMA
//!      `journal_mode` is transactional and MUST be issued outside any
//!      `BEGIN`/`COMMIT`); and
//!   2. `schema::init()` re-asserts `foreign_keys = ON` and the WAL PRAGMA
//!      on every call (per-connection concerns), whereas the migration
//!      step is strictly concerned with the *schema* and the
//!      `user_version` stamp.
//!
//! The v0→v1 migration thus re-issues the verbatim AD-11 DDL (`CREATE TABLE
//! IF NOT EXISTS current_cycle / bandwidth_history`, the
//! `idx_history_luid_cycle` index) plus `PRAGMA user_version = 1`. A fresh
//! DB that has already been through `schema::init()` reports
//! `user_version = 1`, so [`migrate`] is a no-op on it (fixture F6).
//!
//! ## Future versions
//!
//! When v2 lands (e.g. an `ALTER TABLE` on `bandwidth_history`), push the
//! new step onto `MIGRATIONS` and bump `LATEST_USER_VERSION`. Existing v1
//! DBs will run exactly one migration; v0 DBs will run two. The
//! registry pattern keeps each migration hermetic and individually
//! testable.
//!
//! ## G21 (SQLite Operational Discipline)
//!
//! All SQLite access in the workspace funnels through `sidebar-persistence`
//! (guardrail G21). This module owns the *schema-evolution* subset; query
//! helpers for read/write live in `bandwidth_repo` (Story 4.2).
//!
//! Cited:
//!   - architecture.md AD-11 (CREATE TABLE SQL block + `user_version` PRAGMA)
//!   - architecture.md §File-layout (migrate.rs owns `user_version` migrations)
//!   - backlog/epics-and-stories.md Story 4.3 (TDD contract)
//!   - backlog/guardrails.md G21 (all SQLite via sidebar-persistence)
//!   - backlog/tdd-fixtures.md F1 (TempDir), F6 (idempotency)

use rusqlite::Connection;
use sidebar_domain::error::{Error, Result};

/// The latest schema version this build of `sidebar-persistence` understands.
///
/// Equal to the length of the registry returned by [`migrations`]. A
/// database reporting a `user_version` strictly greater than this is
/// "from the future" relative to this binary and is rejected (see
/// Boundary #1) — the operator must upgrade the binary before touching
/// such a DB.
const LATEST_USER_VERSION: u32 = 1;

/// The ordered migration registry.
///
/// `migrations()[i]` is the function that migrates a DB from
/// `user_version = i` to `user_version = i + 1`. Each entry is run inside
/// its own transaction (see [`migrate`]) so a mid-migration fault rolls
/// back cleanly.
///
/// To add v(n→n+1): push the new step here and bump
/// [`LATEST_USER_VERSION`]`. Do NOT reorder or insert — the index *is*
/// the source version.
///
/// We return a slice of function pointers (rather than a `static`) so the
/// registry is trivially `Sync`-free and stays out of the data segment.
fn migrations() -> &'static [fn(&Connection) -> Result<()>] {
    // Index 0 → v0→v1 step. Runs the AD-11 DDL verbatim and stamps
    // `user_version = 1`. `CREATE TABLE IF NOT EXISTS` keeps the step
    // idempotent against a DB that already has the tables (e.g. one
    // previously initialized via `schema::init()` on an older build that
    // pre-dated `migrate()`).
    &[migrate_v0_to_v1]
}

/// Migrate the schema on `conn` up to [`LATEST_USER_VERSION`].
///
/// Reads `PRAGMA user_version`; for each missing step, executes the step's
/// function inside a transaction and commits, then advances
/// `user_version`. If the reported version is greater than
/// `LATEST_USER_VERSION`, returns [`Error::Sqlite`] with an
/// "unknown future schema" message.
///
/// **Idempotent** (fixture F6): calling [`migrate`] on a DB already at
/// `LATEST_USER_VERSION` is a cheap no-op (one PRAGMA read, zero writes).
///
/// # Errors
///
/// - [`Error::Sqlite`] carrying "unknown future schema" if
///   `user_version > LATEST_USER_VERSION`.
/// - [`Error::Sqlite`] carrying the underlying rusqlite message if any
///   PRAGMA read, transaction open/commit, or migration step DDL fails.
///   A mid-step failure rolls the transaction back, leaving `user_version`
///   unchanged (Boundary #2).
///
/// # Panics
///
/// None.
pub fn migrate(conn: &Connection) -> Result<()> {
    // RED-phase stub: intentionally does nothing. The real implementation
    // (GREEN commit) reads user_version and runs the registry. Keeping the
    // body a plain `Ok(())` rather than `todo!()` because the workspace
    // has `clippy::todo = "deny"`.
    let _ = (conn, LATEST_USER_VERSION);
    let _ = migrations();
    Ok(())
}

/// v0 → v1: create the AD-11 tables + index and stamp `user_version = 1`.
///
/// This is the DDL body of [`crate::schema::init`] (verbatim from AD-11)
/// re-asserted here so the migration is self-contained — the registry
/// owns *all* schema evolution, including the initial bootstrap. The
/// `CREATE TABLE IF NOT EXISTS` form makes this safe to run against a DB
/// that already has the tables (e.g. one initialized by `schema::init`
/// on a prior launch).
///
/// `foreign_keys = ON` is a per-connection PRAGMA and is intentionally NOT
/// set here — `migrate()` callers are expected to set per-connection
/// PRAGMAs via `schema::init()` or their own setup. `journal_mode = WAL`
/// likewise cannot be set inside a transaction and is owned by
/// `schema::init()`.
fn migrate_v0_to_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS current_cycle (
            adapter_luid   INTEGER PRIMARY KEY,
            adapter_name   TEXT NOT NULL,
            cycle_start    TEXT NOT NULL,
            rx_bytes       INTEGER NOT NULL DEFAULT 0,
            tx_bytes       INTEGER NOT NULL DEFAULT 0,
            updated_at     TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS bandwidth_history (
            rowid          INTEGER PRIMARY KEY AUTOINCREMENT,
            adapter_luid   INTEGER NOT NULL,
            adapter_name   TEXT NOT NULL,
            cycle_start    TEXT NOT NULL,
            cycle_end      TEXT NOT NULL,
            rx_bytes       INTEGER NOT NULL,
            tx_bytes       INTEGER NOT NULL,
            archived_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_history_luid_cycle
            ON bandwidth_history(adapter_luid, cycle_start);
        PRAGMA user_version = 1;",
    )
    .map_err(|e| Error::Sqlite(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Story 4.3 TDD contract tests.
    //!
    //! Two happy-path tests + two boundary tests. Cited:
    //!   - backlog/epics-and-stories.md Story 4.3 (TDD contract)
    //!   - architecture.md AD-11 (`user_version` PRAGMA + DDL)
    //!   - backlog/guardrails.md G21 (all SQLite via sidebar-persistence)
    //!   - backlog/tdd-fixtures.md F1 (TempDir), F6 (idempotency)

    use super::migrate;
    use rusqlite::Connection;
    use tempfile::TempDir;

    /// Helper: open a fresh SQLite file inside a TempDir (fixture F1) and
    /// hand back `(Connection, TempDir)`. The TempDir must outlive the
    /// connection, so the caller binds both. The returned DB has
    /// `user_version = 0` (the SQLite default for a freshly-created file).
    fn open_temp() -> (Connection, TempDir) {
        let dir = TempDir::new().expect("TempDir::new must succeed");
        let path = dir.path().join("bandwidth.db");
        let conn = Connection::open(&path).unwrap_or_else(|e| panic!("open must succeed: {e}"));
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("fresh DB user_version must read 0");
        assert_eq!(v, 0, "freshly opened DB MUST start at user_version = 0");
        (conn, dir)
    }

    /// Read `PRAGMA user_version` as `i64` (SQLite returns it as INTEGER).
    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("PRAGMA user_version must query")
    }

    // -----------------------------------------------------------------
    // Happy Path #1 — empty DB (user_version = 0) → migrate →
    // user_version = 1, and the AD-11 tables exist. RED: stub leaves
    // user_version at 0.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Happy Path #1, AD-11, fixture F1.
    #[test]
    fn migrate_v0_db_to_v1() {
        let (conn, _dir) = open_temp();
        migrate(&conn).expect("migrate on a fresh v0 DB must succeed");
        assert_eq!(
            user_version(&conn),
            1,
            "user_version MUST be 1 after migrate (AD-11)"
        );
        // AD-11 tables must exist and be queryable.
        let n_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name IN ('current_cycle', 'bandwidth_history')",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master query must succeed");
        assert_eq!(n_tables, 2, "both AD-11 tables MUST exist after migrate");
        // The index from AD-11 must exist too.
        let has_index: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'index' AND name = 'idx_history_luid_cycle'",
                [],
                |row| row.get(0),
            )
            .expect("index lookup must succeed");
        assert_eq!(has_index, 1, "idx_history_luid_cycle MUST exist");
    }

    // -----------------------------------------------------------------
    // Happy Path #2 — already-v1 DB → migrate → no-op (F6 idempotency).
    // RED: stub is trivially idempotent, so this passes in RED too; but
    // it is load-bearing once the registry grows past v1 (the LATEST
    // check would then re-enter the loop).
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Happy Path #2, fixture F6.
    #[test]
    fn migrate_on_v1_db_is_no_op() {
        let (conn, _dir) = open_temp();
        // First migrate: v0 → v1.
        migrate(&conn).expect("first migrate must succeed");
        let after_first = user_version(&conn);
        assert_eq!(after_first, 1, "first migrate must land at v1");
        // Second migrate: must be a no-op (F6).
        migrate(&conn).expect("second migrate MUST succeed (F6 idempotency)");
        let after_second = user_version(&conn);
        assert_eq!(
            after_second, 1,
            "user_version MUST remain 1 after a redundant migrate"
        );
    }

    // -----------------------------------------------------------------
    // Boundary #1 — user_version = 99 → Err "unknown future schema".
    // RED: stub returns Ok(()) → assertion on the Err fires.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Boundary #1.
    #[test]
    fn reject_future_schema_version() {
        let (conn, _dir) = open_temp();
        // Forcibly stamp user_version = 99 to simulate a DB written by a
        // future build. (Directly setting the PRAGMA is exactly what the
        // migration registry does at the end of each step.)
        conn.pragma_update(None, "user_version", 99_i64)
            .expect("PRAGMA user_version = 99 must succeed for the test setup");
        assert_eq!(user_version(&conn), 99, "test setup: must be at v99");

        let err = migrate(&conn).expect_err("migrate on v99 MUST return Err");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown future schema"),
            "error MUST mention 'unknown future schema'; got: {msg}"
        );
        // user_version MUST be unchanged — migrate didn't touch the schema.
        assert_eq!(
            user_version(&conn),
            99,
            "user_version MUST be unchanged after a rejected migrate"
        );
    }

    // -----------------------------------------------------------------
    // Boundary #2 — migration fails mid-way → transaction rolls back,
    // user_version unchanged. We inject the fault by poisoning the DB:
    // after the v0→v1 step creates `current_cycle`, we can't easily make
    // the *next* statement fail mid-batch. Instead, we simulate a
    // future v2 step that faults, by first migrating v0→v1, stamping
    // user_version = 1, then running a `migrate_with_fault` that we
    // drive through the public surface.
    //
    // The cleanest hermetic check the *current* registry admits is:
    // pre-create a table that conflicts with the migration DDL (so the
    // v0→v1 step's `CREATE TABLE` fails), call migrate, and assert the
    // error surfaces AND user_version stays at 0. We force the fault by
    // creating a `current_cycle` VIEW (not a table) — `CREATE TABLE IF
    // NOT EXISTS current_cycle` then fails because the name is taken by
    // a non-table object, mid-batch, leaving the schema unchanged.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Boundary #2.
    #[test]
    fn migration_fault_rolls_back_transaction() {
        let (conn, _dir) = open_temp();
        // Poison the schema: declare `current_cycle` as a VIEW. The v0→v1
        // step's `CREATE TABLE IF NOT EXISTS current_cycle` then errors
        // ("object name is already used") mid-batch. Because the step
        // runs inside a transaction (GREEN contract), nothing it did
        // before the fault is committed — and critically `user_version`
        // is NOT advanced.
        conn.execute_batch("CREATE VIEW current_cycle AS SELECT 1 AS x;")
            .expect("creating a conflicting VIEW must succeed for test setup");
        assert_eq!(
            user_version(&conn),
            0,
            "test setup: user_version still 0 before migrate"
        );

        let err = migrate(&conn).expect_err("migrate MUST fail on a poisoned schema");
        // Any SQLite error is acceptable — the contract is "migrate
        // surfaces the fault rather than silently advancing".
        let _ = err.to_string();

        // The load-bearing assertion: user_version MUST be unchanged.
        assert_eq!(
            user_version(&conn),
            0,
            "user_version MUST be unchanged after a rolled-back migration (Boundary #2)"
        );
        // The other table must NOT exist — the transaction rolled back,
        // so the DDL after the faulting statement never landed.
        let n_history: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name = 'bandwidth_history'",
                [],
                |row| row.get(0),
            )
            .expect("post-fault sqlite_master query must succeed");
        assert_eq!(
            n_history, 0,
            "bandwidth_history MUST NOT exist after rollback"
        );
    }
}
