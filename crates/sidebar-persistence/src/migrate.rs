//! Schema migration via the `user_version` PRAGMA (AD-11, Story 4.3).
//!
//! `migrate(conn)` reads the SQLite `user_version` PRAGMA and applies the
//! sequence of registered migrations (each in its own transaction) to bring
//! the schema up to the latest known version. The registry is an explicit,
//! ordered slice of `fn(&Connection) -> Result<()>` indexed by *target*
//! version — i.e. `MIGRATIONS[0]` is the v0→v1 step and `MIGRATIONS[1]`
//! is the v1→v2 step.
//!
//! ## Relationship to [`crate::schema::init`]
//!
//! Story 4.1's [`crate::schema::init()`] creates the v2 tables and *also*
//! stamps `user_version = 2`. The v0→v1 migration step therefore runs the
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
//! `idx_history_luid_cycle` index) plus `PRAGMA user_version = 1`. The
//! v1→v2 migration adds the current-cycle rule metadata table. A fresh DB
//! that has already been through `schema::init()` reports `user_version = 2`,
//! so [`migrate`] is a no-op on it (fixture F6).
//!
//! ## Future versions
//!
//! When v3 lands (e.g. an `ALTER TABLE` on `bandwidth_history`), push the
//! new step onto `MIGRATIONS` and bump `LATEST_USER_VERSION`. Existing v2
//! DBs will run exactly one migration; v0 DBs will run three. The
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
const LATEST_USER_VERSION: u32 = 2;

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
    &[migrate_v0_to_v1, migrate_v1_to_v2]
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
    // 1. Read the current schema version. PRAGMA user_version returns an
    //    INTEGER; SQLite stores it as i64 and it is always >= 0 for any
    //    DB this binary could open (negative values are not producible via
    //    the PRAGMA setter).
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| Error::Sqlite(e.to_string()))?;
    let current = u32::try_from(current).map_err(|_| {
        Error::Sqlite(format!(
            "user_version is negative ({current}); DB is corrupt or was tampered with"
        ))
    })?;

    // 2. Future-version guard (Boundary #1). A DB stamped with a version
    //    this binary doesn't know is "from the future" — refuse to touch
    //    it so a stale binary can't silently downgrade the schema.
    if current > LATEST_USER_VERSION {
        return Err(Error::Sqlite(format!(
            "unknown future schema: user_version = {current} but this build only knows \
             up to version {LATEST_USER_VERSION}; upgrade the binary before opening this DB"
        )));
    }

    // 3. Idempotent fast path (F6). Already at the latest version → no
    //    writes, no transactions, just return.
    if current == LATEST_USER_VERSION {
        return Ok(());
    }

    // 4. Apply each pending migration in its own transaction. We use an
    //    explicit BEGIN/COMMIT (NOT execute_batch's implicit per-statement
    //    auto-commit) so a mid-step fault rolls the whole step back and
    //    leaves user_version unchanged (Boundary #2).
    //
    //    Within a step, the migration function is free to use execute_batch
    //    for its DDL — rusqlite routes those through the open transaction
    //    without committing. The PRAGMA user_version = N at the end of each
    //    step advances the stamp atomically with the DDL.
    let registry = migrations();
    // `current` is bounded above by `LATEST_USER_VERSION` (u32, currently 2)
    // and `registry.len()` is tiny (one entry per schema version), so the
    // skip count and target version never overflow u32 in practice. The
    // `try_from` calls below defend against a pathological registry size
    // and keep clippy's cast lints quiet without `allow` attributes.
    let start = usize::try_from(current).expect("user_version fits in usize");
    for (step_idx, step_fn) in registry.iter().enumerate().skip(start) {
        let step_idx = u32::try_from(step_idx).expect("registry index fits in u32");
        let target_version = step_idx + 1;
        // Open the transaction. We use `unchecked_transaction` (rather
        // than `transaction`) because the public signature is
        // `migrate(conn: &Connection)` — matching `schema::init` — and
        // `Connection::transaction` requires `&mut`. `unchecked_transaction`
        // gives us a `Transaction<'_>` off a shared borrow; the safety
        // obligation (no other statements on `conn` while the tx is live)
        // is satisfied because we don't hand `conn` to anything else
        // inside the loop body.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        step_fn(&tx)?;
        // Commit. If this fails (e.g. busy), rusqlite returns Err and the
        // Transaction's Drop issues ROLLBACK — user_version is untouched.
        tx.commit().map_err(|e| Error::Sqlite(e.to_string()))?;
        // Defensive: assert the step actually advanced the stamp. A buggy
        // migration that forgets `PRAGMA user_version = N` would otherwise
        // leave us in a loop or silently at the wrong version.
        let stamped: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| Error::Sqlite(e.to_string()))?;
        let stamped = u32::try_from(stamped).map_err(|_| {
            Error::Sqlite(format!(
                "post-migration user_version = {stamped} is negative or > u32::MAX; DB corrupt"
            ))
        })?;
        if stamped != target_version {
            return Err(Error::Sqlite(format!(
                "migration v{step_idx}→v{target_version} committed but left user_version = \
                 {stamped}; the step must stamp PRAGMA user_version = {target_version}"
            )));
        }
    }

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

/// v1 → v2: add current-cycle rule metadata and stamp `user_version = 2`.
///
/// This migration is intentionally additive so it works for both databases
/// produced by the original v1 build (which lack the metadata table) and
/// databases initialized by an intermediate build that already created it.
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS current_cycle_metadata (
            id               INTEGER PRIMARY KEY CHECK (id = 1),
            cycle_start      TEXT NOT NULL,
            cycle_start_rule TEXT NOT NULL
        );
        PRAGMA user_version = 2;",
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
    // user_version = 2, and all current schema tables exist.
    // user_version at 0.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Happy Path #1, AD-11, fixture F1.
    #[test]
    fn migrate_v0_db_to_v2() {
        let (conn, _dir) = open_temp();
        migrate(&conn).expect("migrate on a fresh v0 DB must succeed");
        assert_eq!(
            user_version(&conn),
            2,
            "user_version MUST be 2 after migrate"
        );
        // All schema tables must exist and be queryable.
        let n_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name IN \
                 ('current_cycle', 'bandwidth_history', 'current_cycle_metadata')",
                [],
                |row| row.get(0),
            )
            .expect("sqlite_master query must succeed");
        assert_eq!(n_tables, 3, "all schema tables MUST exist after migrate");
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
    // Happy Path #2 — existing v1 DB → migrate → v2, including the
    // metadata table added by the new migration.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Happy Path #2, fixture F6.
    #[test]
    fn migrate_existing_v1_db_to_v2() {
        let (conn, _dir) = open_temp();
        // Simulate a legacy v1 database created before cycle-rule metadata
        // was introduced: tables exist, metadata does not, and the version
        // stamp is 1.
        conn.execute_batch(
            "CREATE TABLE current_cycle (
                adapter_luid INTEGER PRIMARY KEY,
                adapter_name TEXT NOT NULL,
                cycle_start TEXT NOT NULL,
                rx_bytes INTEGER NOT NULL DEFAULT 0,
                tx_bytes INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE bandwidth_history (
                rowid INTEGER PRIMARY KEY AUTOINCREMENT,
                adapter_luid INTEGER NOT NULL,
                adapter_name TEXT NOT NULL,
                cycle_start TEXT NOT NULL,
                cycle_end TEXT NOT NULL,
                rx_bytes INTEGER NOT NULL,
                tx_bytes INTEGER NOT NULL,
                archived_at TEXT NOT NULL
            );
            CREATE INDEX idx_history_luid_cycle
                ON bandwidth_history(adapter_luid, cycle_start);
            PRAGMA user_version = 1;",
        )
        .expect("legacy v1 schema setup must succeed");
        assert_eq!(user_version(&conn), 1, "test setup must land at v1");

        migrate(&conn).expect("migrate from legacy v1 to v2 MUST succeed");
        let after_second = user_version(&conn);
        assert_eq!(
            after_second, 2,
            "user_version MUST advance to 2 after migrating a legacy v1 DB"
        );
        let metadata_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name = 'current_cycle_metadata'",
                [],
                |row| row.get(0),
            )
            .expect("metadata table lookup must succeed");
        assert_eq!(metadata_exists, 1, "v1→v2 must create metadata table");

        // The now-v2 database is idempotent on subsequent calls.
        migrate(&conn).expect("migrate on v2 MUST be a no-op");
        assert_eq!(user_version(&conn), 2, "user_version must remain 2");
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
    // user_version unchanged. We inject a fault by flipping the
    // connection into read-only mode (`PRAGMA query_only = ON`) before
    // calling migrate. The v0→v1 step's `CREATE TABLE` then fails
    // inside the transaction; rusqlite's `Transaction` drops with an
    // error and issues ROLLBACK, so neither the tables nor the
    // `user_version = 1` stamp land.
    //
    // Why query_only (rather than poisoning sqlite_master with a
    // conflicting VIEW): SQLite's `CREATE TABLE IF NOT EXISTS x` is
    // *too* forgiving — if the name `x` is already taken by any object
    // it silently skips the statement rather than erroring. `query_only
    // = ON` instead rejects every write (DDL included) with
    // SQLITE_READONLY, giving a hermetic, deterministic mid-step fault.
    // -----------------------------------------------------------------
    /// Cited: Story 4.3 TDD contract Boundary #2.
    #[test]
    fn migration_fault_rolls_back_transaction() {
        let (conn, _dir) = open_temp();
        // Poison the connection: read-only mode rejects all DDL/DML.
        conn.pragma_update(None, "query_only", "ON")
            .expect("PRAGMA query_only = ON must succeed for test setup");
        assert_eq!(
            user_version(&conn),
            0,
            "test setup: user_version still 0 before migrate"
        );

        let err = migrate(&conn).expect_err("migrate MUST fail under query_only = ON");
        // Any SQLite error is acceptable — the contract is "migrate
        // surfaces the fault rather than silently advancing".
        let _ = err.to_string();

        // Lift the read-only lock so the post-conditions can read.
        conn.pragma_update(None, "query_only", "OFF")
            .expect("PRAGMA query_only = OFF must succeed for post-condition");

        // The load-bearing assertion: user_version MUST be unchanged.
        assert_eq!(
            user_version(&conn),
            0,
            "user_version MUST be unchanged after a rolled-back migration (Boundary #2)"
        );
        // The tables must NOT exist — the transaction rolled back, so
        // the DDL never landed.
        let n_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'table' AND name IN ('current_cycle', 'bandwidth_history')",
                [],
                |row| row.get(0),
            )
            .expect("post-fault sqlite_master query must succeed");
        assert_eq!(n_tables, 0, "neither AD-11 table MUST exist after rollback");
    }
}
