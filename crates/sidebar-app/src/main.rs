//! `sidebar-app` — Binary entry point.
//!
//! Story 0.1 stub. Real wiring lands in Epic 7 (provider registry, poller,
//! tier probe, event channel, shutdown orchestrator) and Epic 8 (GUI).
//!
//! The binary owns the GUI thread (egui/eframe), spawns the tokio runtime,
//! and hosts the BandwidthAccountant task per architecture.md §1.

fn main() {
    // Story 0.1 stub main — does nothing useful yet. The real main lands in
    // Story 7.3 (two-tier auto-detect probe) and Story 8.1 (AppState + egui).
    println!(
        "sidebar v{} — Story 0.1 stub. crate_present={}",
        env!("CARGO_PKG_VERSION"),
        sidebar_app::crate_present()
    );
}
