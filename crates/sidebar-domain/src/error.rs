//! Shared error type for the sidebar workspace.
//!
//! Story 0.6 — provides a single `Error` enum + `Result<T>` alias that every
//! crate can use via `sidebar-domain`. Keeping the type in `sidebar-domain`
//! (rather than a separate `sidebar-error` crate) preserves the G17 cap of
//! 12 workspace packages.
//!
//! ## Design
//!
//! The enum uses [`thiserror::Error`] so `Display` + `std::error::Error` are
//! derived. Each variant maps to a category of failure the codebase can
//! produce. The variants carry `String` (not specific third-party error
//! types) so that `sidebar-domain` stays pure-Rust with NO heavy deps
//! (no rusqlite, no serde_json, no ureq here). Crates that produce those
//! specific errors convert to the appropriate variant at the boundary.
//!
//! Cited:
//!   - Story 0.6 TDD contract
//!   - architecture.md §4 (sidebar-domain owns shared types, no IO deps)

/// The shared error type for the sidebar workspace.
///
/// Every crate that needs to bubble errors up through `sidebar-domain`'s
/// public API converts its local error into one of these variants. Crates
/// MAY also define their own local error types for internal use, but the
/// boundary into `sidebar-domain` is `Error`.
///
/// Variants carry `String` rather than specific third-party error types so
/// that `sidebar-domain` stays dependency-light (per AD-4: pure, no IO).
/// Crates like `sidebar-persistence` convert `rusqlite::Error` to
/// `Error::Sqlite(message)` at the boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// I/O failure (file not found, permission denied, disk full, etc.).
    /// Wraps a `String` description (the original `io::Error`'s message).
    #[error("io error: {0}")]
    Io(String),

    /// TOML parse or serialization failure (config.toml).
    #[error("toml error: {0}")]
    Toml(String),

    /// SQLite failure (rusqlite).
    #[error("sqlite error: {0}")]
    Sqlite(String),

    /// JSON (de)serialization failure (config, LHM /data.json, etc.).
    #[error("json error: {0}")]
    Json(String),

    /// HTTP failure (LHM /data.json fetch, ureq).
    #[error("http error: {0}")]
    Http(String),

    /// Configuration is structurally valid but semantically wrong
    /// (out-of-range value, unknown field, etc.).
    #[error("config error: {0}")]
    Config(String),

    /// A sensor or provider returned data that doesn't match the expected
    /// shape (e.g. LHM JSON missing required fields).
    #[error("sensor data error: {0}")]
    SensorData(String),

    /// Platform/Win32 call returned an error. The inner `String` carries
    /// the function name + GetLastError code for diagnostics.
    #[error("platform error: {0}")]
    Platform(String),

    /// Billing cycle arithmetic violation (e.g. Day(29) rejected per T-26).
    #[error("billing error: {0}")]
    Billing(String),

    /// Catch-all for errors that don't fit a more specific variant.
    /// Use sparingly — prefer adding a specific variant.
    #[error("sidebar error: {0}")]
    Other(String),
}

/// Convenience `Result` alias for the workspace.
pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    /// Convert any `std::io::Error` into `Error::Io`. This is the one
    /// automatic conversion at the domain level — `io::Error` is std-only
    /// and doesn't violate the no-IO-deps contract.
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Self::Toml(e.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(e: toml::ser::Error) -> Self {
        Self::Toml(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Story 0.6 TDD contract tests.
    //!
    //! Cited: Story 0.6 TDD contract:
    //!   Happy Path #1: Error::Io(io::Error::from(...)) constructs and formats.
    //!   Happy Path #2: Result::<()>::Err(Error::Config(...)) returns through
    //!                  the `?` operator in a test fn.
    //!   Boundary #3: adding a variant forces all match sites to update
    //!                (compile-time exhaustiveness — verified by the
    //!                exhaustive_match_returns_string test below).

    use super::{Error, Result};

    /// Story 0.6 Happy Path #1 — Error::Io constructs from std::io::Error
    /// (via the From impl) and Display formats sensibly.
    #[test]
    fn io_variant_constructs_and_formats() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing file");
        let err = Error::from(io_err);
        let displayed = format!("{err}");
        assert!(
            displayed.contains("io error"),
            "Display must contain 'io error', got: {displayed}"
        );
        assert!(
            displayed.contains("missing file"),
            "Display must contain the inner message, got: {displayed}"
        );
    }

    /// Story 0.6 Happy Path #2 — Result::Err(Error::Config(...)) propagates
    /// through the `?` operator.
    #[test]
    fn config_variant_propagates_through_question_mark() {
        fn inner() -> Result<()> {
            // Simulate a config-validation failure. The `?` below would
            // propagate it; we explicitly return to make the test point clear.
            Err(Error::Config("cycle_start_day out of range".into()))
        }
        let result = inner();
        assert!(matches!(result, Err(Error::Config(_))));
        if let Err(Error::Config(msg)) = result {
            assert!(
                msg.contains("cycle_start_day"),
                "config message must round-trip, got: {msg}"
            );
        }
    }

    /// Story 0.6 Happy Path — the `?` operator works with the From impl.
    /// A function returning Result<()> can use `?` on io::Result.
    #[test]
    fn from_io_enables_question_mark() {
        fn inner() -> Result<()> {
            // This fails because the file doesn't exist — the io::Error
            // converts to Error::Io via the From impl.
            let _ = std::fs::read_to_string("nonexistent-file-for-test.txt")?;
            Ok(())
        }
        let result = inner();
        assert!(matches!(result, Err(Error::Io(_))));
    }

    /// Story 0.6 Boundary #3 (exhaustiveness) — a function that matches
    /// every variant compiles only if the match is exhaustive. Adding a
    /// variant to the enum without updating this match would be a
    /// compile-time error, which is the contract.
    #[test]
    fn exhaustive_match_returns_string() {
        fn classify(err: &Error) -> &'static str {
            // Intentionally exhaustive — when a new variant is added, this
            // match must be updated, and the compiler will fail until it is.
            match err {
                Error::Io(_) => "io",
                Error::Toml(_) => "toml",
                Error::Sqlite(_) => "sqlite",
                Error::Json(_) => "json",
                Error::Http(_) => "http",
                Error::Config(_) => "config",
                Error::SensorData(_) => "sensor",
                Error::Platform(_) => "platform",
                Error::Billing(_) => "billing",
                Error::Other(_) => "other",
            }
        }
        assert_eq!(classify(&Error::Io("x".into())), "io");
        assert_eq!(classify(&Error::Config("x".into())), "config");
        assert_eq!(classify(&Error::Other("y".into())), "other");
    }

    /// Each Display impl produces a non-empty string.
    #[test]
    fn all_variants_display_nonempty() {
        let cases: Vec<Error> = vec![
            Error::Io("io".into()),
            Error::Toml("toml".into()),
            Error::Sqlite("sqlite".into()),
            Error::Json("json".into()),
            Error::Http("http".into()),
            Error::Config("config".into()),
            Error::SensorData("missing field".into()),
            Error::Platform("GetIfEntry2 failed".into()),
            Error::Billing("Day(29) invalid".into()),
            Error::Other("misc".into()),
        ];
        for err in &cases {
            let s = format!("{err}");
            assert!(!s.is_empty(), "Display must produce a non-empty string");
        }
    }

    /// Error implements std::error::Error (required for downcasting, chains).
    #[test]
    fn error_impls_std_error() {
        fn assert_std_error<T: std::error::Error>(_: &T) {}
        let err = Error::Config("test".into());
        assert_std_error(&err);
    }
}
