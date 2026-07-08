//! `sidebar-app` library facade.
//!
//! Story 0.2 adds the `parse_threshold` module here (rather than in a new
//! crate) so it can be unit-tested without inflating the workspace package
//! count past the G17 cap of 12. sidebar-app is now a mixed lib+bin crate
//! (Cargo supports this natively).
//!
//! Future stories add GUI/poller wiring modules here.

pub mod parse_threshold;

/// Story 0.1 smoke marker (kept for the workspace-shape test contract).
#[must_use]
pub fn crate_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::crate_present;

    #[test]
    fn crate_present_returns_true() {
        assert!(crate_present());
    }

    #[test]
    fn crate_present_is_idempotent() {
        assert_eq!(crate_present(), crate_present());
    }
}
