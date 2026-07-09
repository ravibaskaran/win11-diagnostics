//! `sidebar-persistence` — SQLite-backed bandwidth state store (AD-11, Stories 4.1-4.3).
//!
//! Owns all SQLite access in the workspace (guardrail G21). Story 4.1
//! delivers schema initialization ([`schema::init`]); Stories 4.2/4.3 add
//! the read/write/rollover primitives on top.

pub mod migrate;
pub mod schema;

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
