//! `sidebar-sensor` — SensorProvider trait + cost classifier (keystone, AD-4/AD-5).
//!
//! The `SensorProvider` trait is the single contract every adapter implements.
//! The `classify_for_v1` gate filters providers by cost class (NFR-1) and
//! tier before they enter the v1 registry.

pub mod classifier;
pub mod descriptor;
pub mod provider;

/// Story 0.1 smoke marker — proves the crate is reachable via `cargo test`.
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
