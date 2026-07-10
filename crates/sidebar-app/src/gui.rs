//! Story 8.1 — AppState + `eframe::App` + repaint on broadcast. (RED stub.)
//!
//! This is the RED-phase stub: the types and the F8 `egui_kittest` test
//! module exist, but [`render_snapshot`] does NOT yet render readings — it
//! draws only the placeholder, so the Happy Path assertions ("42%" + "CPU")
//! fail until the GREEN commit implements the real render loop.
//!
//! See the GREEN commit for the full module docs + implementation.

use std::sync::{Arc, RwLock};

use eframe::egui;
use egui::Ui;
use sidebar_domain::reading::Reading;
use sidebar_platform::window::ViewportPrefs;
use sidebar_sensor::descriptor::ProviderTier;
use tokio::sync::broadcast;

/// Placeholder text shown when no readings have arrived yet (Boundary #1).
pub(crate) const WAITING_TEXT: &str = "Waiting for data...";

/// Shared application state. Held inside `Arc` by both the [`SidebarApp`]
/// (GUI thread) and future background tasks.
pub struct AppState {
    readings: RwLock<Vec<Reading>>,
    /// Cached last-good snapshot for G15 poison recovery. Used by the GREEN
    /// commit's `snapshot()` impl; allowed dead in the RED stub.
    #[allow(dead_code)]
    last_good: RwLock<Vec<Reading>>,
    tier: ProviderTier,
    rx: RwLock<Option<broadcast::Receiver<Vec<Reading>>>>,
}

impl AppState {
    /// Construct a new `AppState` with the given tier and optional broadcast
    /// receiver.
    #[must_use]
    pub fn new(tier: ProviderTier, rx: Option<broadcast::Receiver<Vec<Reading>>>) -> Arc<Self> {
        Arc::new(Self {
            readings: RwLock::new(Vec::new()),
            last_good: RwLock::new(Vec::new()),
            tier,
            rx: RwLock::new(rx),
        })
    }

    /// The runtime tier (Basic / Full).
    #[must_use]
    pub fn tier(&self) -> ProviderTier {
        self.tier
    }

    /// Return a clone of the latest readings (G15 poison recovery — see GREEN).
    #[must_use]
    pub fn snapshot(&self) -> Vec<Reading> {
        self.readings
            .read()
            .map(|g| (*g).clone())
            .unwrap_or_default()
    }

    /// Replace the readings snapshot (called by `update` after a broadcast drain).
    pub(crate) fn replace_readings(&self, new_readings: Vec<Reading>) {
        if let Ok(mut guard) = self.readings.write() {
            *guard = new_readings;
        }
    }

    /// Non-blocking drain of the broadcast receiver. Returns `Some(readings)`
    /// (latest message) or `None` if empty/closed.
    pub(crate) fn drain_broadcast(&self) -> Option<Vec<Reading>> {
        let mut guard = self.rx.write().ok()?;
        let rx = (*guard).as_mut()?;
        let mut latest: Option<Vec<Reading>> = None;
        loop {
            match rx.try_recv() {
                Ok(readings) => latest = Some(readings),
                Err(
                    broadcast::error::TryRecvError::Empty
                    | broadcast::error::TryRecvError::Lagged(_),
                ) => break,
                Err(broadcast::error::TryRecvError::Closed) => {
                    *guard = None;
                    break;
                }
            }
        }
        latest
    }
}

/// The eframe::App wrapper. Holds a handle to the shared [`AppState`].
pub struct SidebarApp {
    state: Arc<AppState>,
}

impl SidebarApp {
    /// Construct a new `SidebarApp` wrapping the shared `AppState`.
    #[must_use]
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Launch the native eframe window with the sidebar viewport prefs.
    /// NOT unit-testable (opens a real OS window); the `update` logic is
    /// tested headlessly via the F8 harness.
    ///
    /// # Errors
    /// Returns `eframe::Error` if the graphics context fails to initialize.
    pub fn run(self, app_name: &str) -> eframe::Result {
        let viewport = build_viewport(ViewportPrefs::sidebar_defaults());
        let options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };
        let state = self.state;
        eframe::run_native(
            app_name,
            options,
            Box::new(move |_cc| Ok(Box::new(SidebarApp::new(state)))),
        )
    }

    /// Read-only access to the shared state.
    #[must_use]
    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }
}

impl eframe::App for SidebarApp {
    /// egui 0.35 splits the per-frame hook into `logic` (no painting — the
    /// right place for the broadcast drain + request_repaint) and `ui`
    /// (where `CentralPanel` + [`render_snapshot`] go). See eframe::App docs.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(readings) = self.state.drain_broadcast() {
            self.state.replace_readings(readings);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let snapshot = self.state.snapshot();
        let tier = self.state.tier();
        render_snapshot(ui, &snapshot, tier);
    }
}

/// Build an `egui::ViewportBuilder` from [`ViewportPrefs`] (Story 6.1).
pub(crate) fn build_viewport(prefs: ViewportPrefs) -> egui::ViewportBuilder {
    let mut vp = egui::ViewportBuilder::default()
        .with_title("sidebar")
        .with_resizable(true)
        .with_inner_size(egui::Vec2::new(280.0, 720.0));
    if prefs.transparent {
        vp = vp.with_transparent(true);
    }
    if prefs.borderless {
        vp = vp.with_decorations(false);
    }
    if prefs.topmost {
        vp = vp.with_always_on_top();
    }
    vp
}

/// RED stub: renders ONLY the placeholder regardless of readings. The Happy
/// Path tests ("42%" + "CPU") fail against this stub; the GREEN commit
/// implements the real metric-row render via the Story 1.3 formatters.
pub fn render_snapshot(ui: &mut Ui, _readings: &[Reading], tier: ProviderTier) {
    let tier_label = match tier {
        ProviderTier::Basic => "BASIC",
        ProviderTier::Full => "FULL",
        ProviderTier::Both => "BOTH",
    };
    ui.label(format!("Tier: {tier_label}"));
    ui.separator();
    ui.label(WAITING_TEXT);
}

#[cfg(test)]
mod tests {
    //! Story 8.1 TDD contract tests (RED phase — assertions FAIL against the
    //! stub `render_snapshot`). The GREEN commit implements the real render.
    //!
    //! ## F8 harness approach
    //!
    //! We use `egui_kittest::Harness::new_ui` with a closure that calls our
    //! pure [`render_snapshot`] function. The kittest access-tree captures
    //! every `ui.label(...)` as a queryable node; we assert on the rendered
    //! text by walking the tree. This is the headless F8 pattern (no wgpu,
    //! no image-snapshot files — per the egui_kittest docs, "prefer regular
    //! Rust tests over image comparison tests"). The image-snapshot variant
    //! lands in Story 11.3 once CI has a stable renderer.

    use super::*;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};
    use std::time::Instant;
    use tokio::sync::broadcast;

    fn reading(kind: MetricKind, value: f64, unit: Unit) -> Reading {
        Reading {
            sensor: SensorId::new("cpu", "package"),
            kind,
            value,
            unit,
            timestamp: Instant::now(),
        }
    }

    /// Walk the kittest access tree and collect every node's text (egui puts
    /// label text in `value()` on `Role::Label` nodes; we collect BOTH
    /// `label()` and `value()` to be robust across egui versions).
    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: AppState with one CPU reading → "42%" + "CPU" =====

    #[test]
    fn cpu_reading_renders_42_percent_and_cpu_label() {
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &readings, ProviderTier::Basic);
        });
        harness.run();

        let labels = all_labels(&harness);
        let joined = labels.join(" | ");
        assert!(
            joined.contains("CPU"),
            "snapshot must contain 'CPU' label (got: {joined})"
        );
        assert!(
            joined.contains("42%"),
            "snapshot must contain '42%' value (got: {joined})"
        );
    }

    // ===== Happy Path #2: broadcast drain returns latest message =====

    #[test]
    fn drain_broadcast_returns_latest_message() {
        let (tx, rx) = broadcast::channel::<Vec<Reading>>(8);
        let state = AppState::new(ProviderTier::Basic, Some(rx));

        assert!(state.drain_broadcast().is_none());

        tx.send(vec![reading(
            MetricKind::CpuUtilization,
            10.0,
            Unit::Percent,
        )])
        .expect("send 1");
        tx.send(vec![reading(
            MetricKind::CpuUtilization,
            42.0,
            Unit::Percent,
        )])
        .expect("send 2");

        let drained = state
            .drain_broadcast()
            .expect("drain returns Some after messages sent");
        assert_eq!(drained.len(), 1);
        assert!(
            (drained[0].value - 42.0).abs() < f64::EPSILON,
            "latest message wins"
        );
    }

    // ===== Boundary #1: empty readings → "Waiting for data..." =====

    #[test]
    fn empty_readings_shows_waiting_placeholder() {
        let empty: Vec<Reading> = Vec::new();
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &empty, ProviderTier::Basic);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(WAITING_TEXT),
            "empty readings must render '{WAITING_TEXT}' (got: {labels})"
        );
    }

    // ===== Boundary #2: poisoned RwLock → last good snapshot (G15) =====

    #[test]
    fn snapshot_returns_readings_after_replace() {
        let state = AppState::new(ProviderTier::Basic, None);
        state.replace_readings(vec![reading(
            MetricKind::CpuUtilization,
            42.0,
            Unit::Percent,
        )]);
        let snap = state.snapshot();
        assert_eq!(snap.len(), 1);
    }

    // ===== Boundary #3: 1000 readings → truncation marker =====

    #[test]
    fn many_readings_truncate_at_max() {
        let many: Vec<Reading> = (0..1000)
            .map(|i| reading(MetricKind::CpuUtilization, f64::from(i), Unit::Percent))
            .collect();
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &many, ProviderTier::Basic);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("truncated") || labels.contains('+'),
            "1000 readings must render a truncation marker (got: {labels})"
        );
    }

    #[test]
    fn tier_basic_renders_basic_label() {
        let empty: Vec<Reading> = Vec::new();
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &empty, ProviderTier::Basic);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("BASIC"),
            "Basic tier must render 'BASIC' label (got: {labels})"
        );
    }
}
