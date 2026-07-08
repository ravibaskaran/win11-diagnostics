//! `sidebar-adapter-net` — Per-NIC throughput via GetIfEntry2 (Story 3.5).
//!
//! Story 0.1 stub. Real functionality lands in subsequent stories per the
//! backlog critical path.

/// Story 0.1 smoke marker — proves the crate is reachable via `cargo test`.
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
