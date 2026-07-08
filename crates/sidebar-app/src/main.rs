//! `sidebar-app` — Binary entry point.
//!
//! Story 0.1 stub. Real wiring lands in Epic 7 (provider registry, poller,
//! tier probe, event channel, shutdown orchestrator) and Epic 8 (GUI).
//!
//! The binary owns the GUI thread (egui/eframe), spawns the tokio runtime,
//! and hosts the BandwidthAccountant task per architecture.md §1.

/// Story 0.1 smoke marker — proves the binary is reachable via `cargo test`.
#[must_use]
pub fn crate_present() -> bool {
    true
}

fn main() {
    // Story 0.1 stub main — does nothing useful yet. The real main lands in
    // Story 7.3 (two-tier auto-detect probe) and Story 8.1 (AppState + egui).
    // We print the smoke marker so `cargo run` produces a discoverable output.
    println!(
        "sidebar v{} — Story 0.1 stub. crate_present={}",
        env!("CARGO_PKG_VERSION"),
        crate_present()
    );
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
