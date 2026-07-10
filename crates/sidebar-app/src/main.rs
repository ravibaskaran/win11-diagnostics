//! `sidebar-app` binary entry point.
//!
//! Story 8.1 wiring: construct [`AppState`] + [`SidebarApp`] and launch the
//! native eframe window with the sidebar viewport prefs (transparent +
//! borderless + topmost from Story 6.1).
//!
//! ## Thread model
//!
//! The binary owns the GUI thread (eframe requires the main thread on Windows
//! for the OS message loop). A single-worker tokio runtime is constructed so
//! that future background work (the poller in Story 7.2, the bandwidth
//! accountant in Story 5.2) can be spawned on it — for Story 8.1 the runtime
//! is created but no tasks are spawned yet (the launch sequence that wires
//! tier probe → registry → poller → AppState lands in Story 7.3 + Story 8.5).
//!
//! A `broadcast::channel::<Vec<Reading>>(8)` (T-14) is constructed here so
//! the AppState drain loop is real from day one; nothing sends to it yet
//! until Story 8.5 wires the poller. The GUI therefore launches into the
//! `Waiting for data...` placeholder state — this is the correct first-launch
//! shape per the Story 8.1 Boundary #1 contract.
//!
//! ## Cited
//!
//! - Story 8.1 TDD contract ( AppState + egui::App )
//! - architecture.md §6 (GUI crate), §1 (binary owns GUI thread + tokio)
//! - nfr-thresholds.md T-14 (broadcast cap 8)
//! - guardrails.md HITL smoke (§7.4 manual on Win11)

use std::sync::Arc;

use sidebar_app::gui::first_run;
use sidebar_app::gui::{AppState, SidebarApp};
use sidebar_domain::config::Config;
use sidebar_domain::reading::Reading;
use sidebar_sensor::descriptor::ProviderTier;
use tokio::sync::broadcast;

fn main() -> eframe::Result {
    tracing::info!(
        target = "sidebar.app.main",
        version = env!("CARGO_PKG_VERSION"),
        "sidebar binary launching (Story 8.1 + 8.10 first-run wizard)"
    );

    // tokio runtime for background work (poller, accountant). Story 8.1 does
    // NOT spawn any tasks yet — the launch sequence that owns the runtime
    // lifecycle (tier probe → registry → poller → shutdown) lands in Story 8.5.
    // The runtime is built here so the binary's process shape matches the
    // production layout from day one; the GUI thread does not need it.
    let _runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    // Story 8.10: first-run wizard gate. Load the config (default if absent),
    // and if the wizard should show (`first_run_complete != true`), the
    // SidebarApp renders the wizard modal instead of the live sidebar. G24:
    // the poller must NOT start until the wizard completes — when Story 8.5
    // wires the poller spawn, it goes BEHIND this gate (only spawn after
    // `first_run::should_show(&config)` returns false). For Story 8.1 the
    // poller isn't spawned yet, so the gate is structural: it documents the
    // invariant + ensures the wizard is the first thing a new user sees.
    let config = Config::default();
    if first_run::should_show(&config) {
        tracing::info!(
            target = "sidebar.app.main",
            "first-run wizard active — poller gated (G24) until wizard completes"
        );
    }

    // Broadcast channel for the poller → AppState pipe (T-14 cap = 8).
    // Story 8.5 will move the Sender into the poller task; for now both ends
    // live in main, so the AppState sees an empty channel and renders the
    // "Waiting for data..." placeholder (Boundary #1).
    let (_tx, rx) = broadcast::channel::<Vec<Reading>>(8);

    let state: Arc<AppState> = AppState::new(ProviderTier::Basic, Some(rx));
    let app = SidebarApp::new(state);
    app.run("sidebar")
}
