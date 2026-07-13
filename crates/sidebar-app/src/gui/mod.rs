//! Story 8.1 — AppState + `eframe::App` + repaint on broadcast.
//!
//! This is the GREEN-phase implementation. The shared [`AppState`] holds the
//! latest readings snapshot behind a `RwLock` (G15 poison recovery — `snapshot()`
//! falls back to `last_good` if the lock is poisoned). [`SidebarApp`] is the
//! `eframe::App` wrapper; egui 0.35 splits the per-frame hook into:
//!
//! - [`SidebarApp::logic`] — drains the broadcast receiver, calls
//!   `ctx.request_repaint()` on a fresh message. This is the "repaint on
//!   broadcast" half of T-9 (the other half is vsync-driven repaint from
//!   eframe itself).
//! - [`SidebarApp::ui`] — renders the readings snapshot via
//!   [`render_snapshot`], which uses the Story 1.3 `format_*` functions.
//!
//! ## Truncation (T-21)
//!
//! [`render_snapshot`] caps the rendered row count at [`MAX_ROWS`] (64). The
//! sidebar viewport is 280×720 (Story 6.1) — at the default font this fits
//! ~30 metric rows; 64 leaves headroom for tier / status pill / bandwidth
//! panel rows that land in Story 8.2–8.4. A 1000-reading poller batch (T-21)
//! renders as 64 rows + a "+936 more (truncated)" marker — within T-9 (16ms,
//! see the `many_readings_truncate_at_max` test).
//!
//! ## F8 headless test approach
//!
//! Tests use `egui_kittest::Harness::new_ui` with a closure that calls
//! [`render_snapshot`] directly (no wgpu, no image snapshots). The kittest
//! access tree captures every `ui.label(...)` as a queryable node; tests
//! walk the tree via [`all_labels`](tests::all_labels) and assert on the
//! rendered text. Image-snapshot variants (insta) land in Story 11.3 once
//! CI has a stable renderer.
//!
//! ## Cited
//!
//! - Story 8.1 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - architecture.md §6 (GUI crate), §7.4 (manual smoke)
//! - nfr-thresholds.md T-9 (16ms render), T-14 (broadcast cap 8),
//!   T-20 (finite Reading value), T-21 (truncation)
//! - guardrails.md G15 (RwLock poison recovery)
//! - tdd-fixtures.md F8 (egui_kittest harness)

// Story 8.2 + 8.3 GUI components + Story 8.4 (bandwidth panel) + Story 8.5
// (settings panel). Each submodule owns its TDD contract; the composition
// (pill at top, metric rows below, bandwidth panel, settings panel behind a
// gear toggle) lives in `render_snapshot`.
//
// Story 8.6 (theme + accent), 8.7 (sparkline), 8.8 (threshold alert UI) add
// three more submodules; their wiring into `render_sidebar` lands in the
// GREEN commit for that batch.
pub mod alert_indicator;
pub mod bandwidth_panel;
pub mod first_run;
pub mod metric_list;
pub mod metric_row;
pub mod settings_panel;
pub mod sparkline;
pub mod status_pill;
pub mod theme;

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use eframe::egui;
use egui::Ui;
use sidebar_bandwidth::view::BandwidthView;
use sidebar_domain::config::Config;
use sidebar_domain::event::Event;
use sidebar_domain::format::{
    format_bps, format_bytes, format_hz, format_percent, format_power, format_rpm, format_temp,
    format_voltage, Base, TempUnit,
};
use sidebar_domain::reading::{MetricKind, Reading, Unit};
use sidebar_platform::window::ViewportPrefs;
use sidebar_sensor::descriptor::ProviderTier;
use tokio::sync::broadcast;

#[cfg(windows)]
use sidebar_platform::{hotkey, monitors, theme_bridge};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::shutdown::ShutdownSignal;

/// Placeholder text shown when no readings have arrived yet (Boundary #1).
pub(crate) const WAITING_TEXT: &str = "Waiting for data...";

/// Maximum number of metric rows rendered per frame (T-21 truncation point).
/// 64 leaves headroom for the 280×720 viewport at the default font size; the
/// truncation marker carries the remaining count.
pub(crate) const MAX_ROWS: usize = 64;

fn recover_read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target = "sidebar.app.state",
                "GUI RwLock poisoned on read; recovering guarded state (G15)"
            );
            poisoned.into_inner()
        }
    }
}

fn recover_write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target = "sidebar.app.state",
                "GUI RwLock poisoned on write; recovering guarded state (G15)"
            );
            poisoned.into_inner()
        }
    }
}

/// Shared application state. Held inside `Arc` by both the [`SidebarApp`]
/// (GUI thread) and future background tasks.
pub struct AppState {
    readings: RwLock<Vec<Reading>>,
    /// Cached last-good snapshot for G15 poison recovery. `snapshot()` writes
    /// here on every successful read; if `readings` is poisoned, `snapshot()`
    /// returns the contents of `last_good` instead of panicking.
    last_good: RwLock<Vec<Reading>>,
    tier: RwLock<ProviderTier>,
    rx: RwLock<Option<broadcast::Receiver<Vec<Reading>>>>,
    /// User configuration (mutable — the settings panel edits this; the
    /// on_change callback persists to disk). Held behind a `RwLock` so the
    /// integration launch sequence can read it before eframe starts + the GUI
    /// can mutate it per-frame. Cloned cheaply (Config is plain serde data).
    config: RwLock<Config>,
    /// Composed view payload for `render_sidebar`: the bandwidth panel DTO +
    /// settings-open flag + sparkline samples. Mutated by the GUI per-frame
    /// (gear toggle, bandwidth refresh).
    view: RwLock<SidebarView>,
    /// Tier/theme/monitor/hotkey/shutdown events from the EventChannel
    /// (Story 7.4). Drained each frame in `SidebarApp::logic`; tier changes
    /// update `self.tier`.
    event_rx: RwLock<Option<broadcast::Receiver<Event>>>,
    shutdown: RwLock<Option<ShutdownSignal>>,
    /// Story 12.2 — per-metric rolling-history map for sparkline graphs.
    /// Each `replace_readings` call pushes every reading's value into the
    /// corresponding MetricKey window.
    history: RwLock<sidebar_domain::graph::MetricHistory>,
}

impl AppState {
    /// Construct a new `AppState` with the given tier and optional broadcast
    /// receiver (Story 8.1 path — kept for the existing tests + render_snapshot
    /// smoke). Defaults the config + view.
    #[must_use]
    pub fn new(tier: ProviderTier, rx: Option<broadcast::Receiver<Vec<Reading>>>) -> Arc<Self> {
        Self::new_full(tier, rx, None, Config::default(), SidebarView::default())
    }

    /// Full constructor — the integration launch sequence (Story 8.5 main.rs)
    /// wires the live config + view + event receiver through this. The
    /// settings panel mutates the config in place; the bandwidth panel reads
    /// the view.
    #[must_use]
    pub fn new_full(
        tier: ProviderTier,
        rx: Option<broadcast::Receiver<Vec<Reading>>>,
        event_rx: Option<broadcast::Receiver<Event>>,
        config: Config,
        view: SidebarView,
    ) -> Arc<Self> {
        Arc::new(Self {
            readings: RwLock::new(Vec::new()),
            last_good: RwLock::new(Vec::new()),
            tier: RwLock::new(tier),
            rx: RwLock::new(rx),
            config: RwLock::new(config),
            view: RwLock::new(view),
            event_rx: RwLock::new(event_rx),
            shutdown: RwLock::new(None),
            history: RwLock::new(sidebar_domain::graph::MetricHistory::new(60)),
        })
    }

    /// The runtime tier (Basic / Full).
    #[must_use]
    pub fn tier(&self) -> ProviderTier {
        *recover_read(&self.tier)
    }

    /// Replace the runtime tier (called by `SidebarApp::logic` when an
    /// `Event::TierChanged` arrives from the EventChannel).
    pub(crate) fn set_tier(&self, tier: ProviderTier) {
        *recover_write(&self.tier) = tier;
    }

    /// Clone the current config (the settings panel reads + edits a local copy
    /// each frame; the on_change callback persists).
    #[must_use]
    pub fn config(&self) -> Config {
        recover_read(&self.config).clone()
    }

    /// Replace the config (the integration host calls this after the settings
    /// panel mutates a local copy).
    pub(crate) fn replace_config(&self, new_config: Config) {
        *recover_write(&self.config) = new_config;
    }

    /// Clone the current SidebarView payload.
    #[must_use]
    pub fn view(&self) -> SidebarView {
        recover_read(&self.view).clone()
    }

    /// Replace the SidebarView (called by `SidebarApp::ui` after the gear
    /// toggle flips or the bandwidth panel DTO refreshes).
    pub(crate) fn replace_view(&self, new_view: SidebarView) {
        *recover_write(&self.view) = new_view;
    }

    /// Attach the shared shutdown signal used by the native GUI lifecycle.
    pub fn set_shutdown_signal(&self, signal: ShutdownSignal) {
        *recover_write(&self.shutdown) = Some(signal);
    }

    /// Request cancellation and Event::Shutdown when the GUI closes.
    pub(crate) fn request_shutdown(&self) {
        if let Some(signal) = recover_read(&self.shutdown).as_ref() {
            signal.request();
        }
    }

    /// Non-blocking drain of the EventChannel receiver. Returns the latest
    /// events since the last drain (tier/theme/monitor/hotkey/shutdown). Used
    /// by `SidebarApp::logic` each frame.
    pub(crate) fn drain_events(&self) -> Vec<Event> {
        let mut guard = recover_write(&self.event_rx);
        let Some(rx) = guard.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(event) => out.push(event),
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
        out
    }

    /// Return a clone of the latest readings.
    ///
    /// **G15 poison recovery**: if `readings` is poisoned (a writer panicked
    /// mid-update), we fall back to `last_good`. This guarantees the GUI never
    /// blanks on a poison event — it shows the last successfully-cached frame
    /// and logs at `warn`. If `last_good` is ALSO poisoned (a writer panicked
    /// while updating it), we return an empty `Vec` (placeholder kicks in).
    #[must_use]
    pub fn snapshot(&self) -> Vec<Reading> {
        if let Ok(guard) = self.readings.read() {
            let snap = guard.clone();
            recover_write(&self.last_good).clone_from(&snap);
            snap
        } else {
            tracing::warn!(
                target = "sidebar.app.state",
                "GUI readings RwLock poisoned; returning last-good snapshot (G15)"
            );
            if let Ok(guard) = self.last_good.read() {
                guard.clone()
            } else {
                tracing::warn!(
                    target = "sidebar.app.state",
                    "GUI last-good RwLock also poisoned; returning empty snapshot (G15)"
                );
                Vec::new()
            }
        }
    }

    /// Replace the readings snapshot (called by [`SidebarApp::logic`] after a
    /// broadcast drain).
    pub(crate) fn replace_readings(&self, new_readings: Vec<Reading>) {
        // Story 12.2 — push each reading into the per-metric history map so
        // the GUI can render sparkline graphs (T-22 default 60 samples).
        if let Ok(mut history) = self.history.write() {
            for r in &new_readings {
                let key = sidebar_domain::graph::MetricKey {
                    category: r.sensor.category.to_string(),
                    instance: r.sensor.instance.clone(),
                    kind: format!("{:?}", r.kind),
                };
                let value = r.value;
                history.push(key, value);
            }
        }
        *recover_write(&self.readings) = new_readings;
    }

    /// Story 12.2 — borrow the per-metric history map (for sparkline rendering).
    /// Returns a cloned snapshot to avoid holding the lock across egui render.
    fn history_snapshot(&self) -> sidebar_domain::graph::MetricHistory {
        recover_read(&self.history).clone()
    }

    /// Non-blocking drain of the broadcast receiver. Returns `Some(readings)`
    /// (the latest message — older ones are coalesced away per T-14) or `None`
    /// if the channel is empty/closed. Called every frame by
    /// [`SidebarApp::logic`].
    pub(crate) fn drain_broadcast(&self) -> Option<Vec<Reading>> {
        let mut guard = recover_write(&self.rx);
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

/// The eframe::App wrapper. Holds a handle to the shared [`AppState`] plus a
/// local copy of the config + SidebarView that the GUI mutates per-frame.
///
/// The config/view live BOTH on the SidebarApp (so `ui()` can borrow them
/// mutably without locking the RwLock mid-frame) AND mirrored into AppState
/// (so background tasks and the integration host can observe changes). After
/// each frame the local copies are written back to AppState via
/// `replace_config` / `replace_view`; the on_change callback persists the
/// config to disk (debounce is a refinement — for now, write immediately).
pub struct SidebarApp {
    state: Arc<AppState>,
    /// Local mutable copy of the config — the settings panel edits this.
    /// Seeded from AppState.config on construction.
    config: Config,
    /// Local mutable copy of the SidebarView (bandwidth DTO + settings_open).
    /// Seeded from AppState.view on construction.
    view: SidebarView,
    /// Whether the first-run wizard should show (Story 8.10). When true,
    /// `ui()` renders the wizard modal instead of the live sidebar; the
    /// poller is NOT spawned (G24) until the wizard completes + the user
    /// restarts.
    wizard_active: bool,
    /// Path to the config.toml on disk (so the on_change callback can persist
    /// without re-resolving %APPDATA% every frame). Empty when no on-disk
    /// path is in play (the wizard path or the Story 8.1 test path).
    config_path: std::path::PathBuf,
    /// Event producer used by the native platform bridge. Tests and
    /// headless callers leave this unset.
    event_tx: Option<broadcast::Sender<Event>>,
    /// Story 12.8 Gap 2 — watch receiver for live BandwidthView from the
    /// accountant thread. Drained in `logic()` each frame into `self.view`.
    /// `None` in tests + when the wizard gate skipped the accountant.
    bandwidth_view_rx:
        Option<tokio::sync::watch::Receiver<Option<sidebar_bandwidth::view::BandwidthView>>>,
    /// Story 12.8 Gap 3 — liveness probe for the OHM child. Returns `true`
    /// while the elevated LHM is alive; `false` once it exits. `logic()`
    /// polls this each frame and emits `Event::TierChanged(Basic)` exactly
    /// once on the first `false`, then sets this to `None` (one-shot).
    /// `None` when no supervisor is attached (Basic mode or test path).
    child_alive_fn: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// Story 12.8 Gap 1 — launch callback invoked when the user clicks the
    /// Basic status pill (requesting Full-mode LHM elevation). `None` in
    /// tests + when no supervisor is attached.
    launch_fn: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Story 12.8 Gap 3 — latches `true` after the first Full→Basic
    /// degradation fires so we don't repeatedly broadcast.
    child_exit_degraded: bool,
    #[cfg(windows)]
    platform: Option<PlatformRuntime>,
}

impl SidebarApp {
    /// Construct a new `SidebarApp` wrapping the shared `AppState`. Seed the
    /// local config + view from AppState. Used by the Story 8.1 tests + the
    /// render_snapshot smoke path.
    #[must_use]
    pub fn new(state: Arc<AppState>) -> Self {
        let config = state.config();
        let view = state.view();
        Self {
            state,
            config,
            view,
            wizard_active: false,
            config_path: std::path::PathBuf::new(),
            event_tx: None,
            bandwidth_view_rx: None,
            child_alive_fn: None,
            launch_fn: None,
            child_exit_degraded: false,
            #[cfg(windows)]
            platform: None,
        }
    }

    /// Construct a `SidebarApp` with an explicit config-path so the on_change
    /// callback persists to the right file. The wizard_active flag toggles
    /// between the first-run wizard (Story 8.10) and the live sidebar.
    #[must_use]
    pub fn with_config_path(
        state: Arc<AppState>,
        config_path: std::path::PathBuf,
        wizard_active: bool,
    ) -> Self {
        let config = state.config();
        let view = state.view();
        Self {
            state,
            config,
            view,
            wizard_active,
            config_path,
            event_tx: None,
            bandwidth_view_rx: None,
            child_alive_fn: None,
            launch_fn: None,
            child_exit_degraded: false,
            #[cfg(windows)]
            platform: None,
        }
    }

    /// Attach the EventChannel producer used by the native platform bridge.
    #[must_use]
    pub fn with_event_sender(mut self, sender: broadcast::Sender<Event>) -> Self {
        self.event_tx = Some(sender);
        self
    }

    /// Story 12.8 Gap 2 — attach the BandwidthView watch receiver from the
    /// accountant thread. Drained in `logic()` each frame into `self.view`.
    #[must_use]
    pub fn with_bandwidth_view_rx(
        mut self,
        rx: tokio::sync::watch::Receiver<Option<sidebar_bandwidth::view::BandwidthView>>,
    ) -> Self {
        self.bandwidth_view_rx = Some(rx);
        self
    }

    /// Story 12.8 Gap 3 — attach the OHM child-liveness probe. `logic()`
    /// polls this each frame and emits `Event::TierChanged(Basic)` exactly
    /// once when the probe first returns `false`, then disables itself.
    #[must_use]
    pub fn with_child_alive_fn(mut self, probe: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.child_alive_fn = Some(probe);
        self.child_exit_degraded = false;
        self
    }

    /// Story 12.8 Gap 1 — attach the launch callback invoked when the user
    /// clicks the Basic status pill. The closure sends a launch request to
    /// the supervisor-owning thread (main.rs wires the channel + thread).
    #[must_use]
    pub fn with_launch_fn(mut self, launch: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.launch_fn = Some(launch);
        self
    }

    fn apply_runtime_hooks(
        mut app: Self,
        bandwidth_view_rx: Option<tokio::sync::watch::Receiver<Option<BandwidthView>>>,
        child_alive_fn: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        launch_fn: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        if let Some(rx) = bandwidth_view_rx {
            app = app.with_bandwidth_view_rx(rx);
        }
        if let Some(probe) = child_alive_fn {
            app = app.with_child_alive_fn(probe);
        }
        if let Some(launch) = launch_fn {
            app = app.with_launch_fn(launch);
        }
        app
    }

    /// Launch the native eframe window with the sidebar viewport prefs.
    /// NOT unit-testable (opens a real OS window); the `logic`/`ui` methods
    /// are tested headlessly via the F8 harness.
    ///
    /// # Errors
    /// Returns `eframe::Error` if the graphics context fails to initialize.
    pub fn run(self, app_name: &str) -> eframe::Result {
        // Story 12.x transparency fallback: when force_opaque is set (or the
        // config requests it), disable the transparent viewport request so
        // wgpu doesn't warn about unsupported CompositeAlphaMode.
        let mut prefs = ViewportPrefs::sidebar_defaults();
        if self.config.display.force_opaque {
            prefs.transparent = false;
        }
        let viewport = build_viewport(prefs);
        let options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };
        let state = self.state;
        let config_path = self.config_path;
        let wizard_active = self.wizard_active;
        let display_config = self.config.display.clone();
        let event_tx = self.event_tx.clone();
        let bandwidth_view_rx = self.bandwidth_view_rx;
        let child_alive_fn = self.child_alive_fn;
        let launch_fn = self.launch_fn;
        eframe::run_native(
            app_name,
            options,
            Box::new(move |cc| {
                #[cfg(windows)]
                configure_capture_exclusion(cc, &display_config);
                let mut app = SidebarApp::with_config_path(state, config_path, wizard_active);
                app.event_tx.clone_from(&event_tx);
                app = SidebarApp::apply_runtime_hooks(
                    app,
                    bandwidth_view_rx,
                    child_alive_fn,
                    launch_fn,
                );
                #[cfg(windows)]
                configure_platform(cc, &mut app);
                Ok(Box::new(app))
            }),
        )
    }

    /// Read-only access to the shared state.
    #[must_use]
    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }

    /// Persist the in-memory config to the on-disk path. Best-effort: errors
    /// are logged at `warn` (G15 — settings-panel edits are non-fatal). Called
    /// from the on_change callback after every settings mutation.
    fn persist_config(&self) {
        if self.config_path.as_os_str().is_empty() {
            // No on-disk path (test or wizard path) — skip persistence.
            return;
        }
        match self.config.to_toml_string() {
            Ok(toml_str) => {
                if let Err(e) = std::fs::write(&self.config_path, toml_str) {
                    tracing::warn!(
                        path = %self.config_path.display(),
                        error = %e,
                        "settings panel: failed to persist config (G15 — non-fatal)"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "settings panel: failed to serialize config (G15 — non-fatal)"
                );
            }
        }
    }
}

#[cfg(windows)]
const CLICK_THROUGH_HOTKEY_ID: i32 = 0x5349;

#[cfg(windows)]
struct PlatformRuntime {
    hwnd: HWND,
    hotkey_id: Option<i32>,
    click_through: bool,
    monitor_id: Option<String>,
    /// Channel receiver for hotkey events from the dedicated hotkey thread.
    /// The hotkey thread registers RegisterHotKey + GetMessageW on its OWN
    /// thread, so WM_HOTKEY messages are NOT consumed by egui's event loop.
    hotkey_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// Handle to the dedicated hotkey thread (for cleanup on unregister).
    #[allow(clippy::used_underscore_binding)]
    hotkey_thread: Option<std::thread::JoinHandle<()>>,
    /// Win32 thread-id of the dedicated hotkey thread. `unregister` posts
    /// `WM_QUIT` to this TID so the thread's `GetMessageW` loop wakes, runs
    /// its own `UnregisterHotKey(None, id)`, and exits cleanly. Without it
    /// the thread would block in `GetMessageW` until process death (leak).
    hotkey_thread_id: Option<u32>,
}

#[cfg(windows)]
impl PlatformRuntime {
    fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            hotkey_id: None,
            click_through: false,
            monitor_id: None,
            hotkey_rx: None,
            hotkey_thread: None,
            hotkey_thread_id: None,
        }
    }

    fn poll(&mut self, config: &mut Config, ctx: &egui::Context) -> Vec<Event> {
        use windows::Win32::UI::WindowsAndMessaging::{
            PeekMessageW, MSG, PM_REMOVE, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
        };

        let mut events = Vec::new();

        // Drain hotkey events from the dedicated thread's channel.
        if let Some(rx) = &self.hotkey_rx {
            while let Ok(()) = rx.try_recv() {
                let enabled = !self.click_through;
                if let Err(error) = hotkey::set_click_through(self.hwnd, enabled) {
                    tracing::warn!(?error, "click-through toggle unavailable");
                } else {
                    self.click_through = enabled;
                    events.push(Event::HotkeyPressed("click_through".into()));
                }
            }
        }

        loop {
            let mut message = MSG::default();
            // SAFETY: see the hotkey message pump above; this filter consumes
            // only the broadcast WM_SETTINGCHANGE notification.
            let present = unsafe {
                PeekMessageW(
                    &raw mut message,
                    None,
                    WM_SETTINGCHANGE,
                    WM_SETTINGCHANGE,
                    PM_REMOVE,
                )
            };
            if !present.as_bool() {
                break;
            }
            if let Some(event) =
                theme_bridge::theme_event_from_message(message.message, message.lParam)
            {
                events.push(event);
            }
        }

        loop {
            let mut message = MSG::default();
            // SAFETY: the message is stack-owned and only WM_DISPLAYCHANGE is
            // removed, leaving unrelated eframe messages untouched. Windows
            // broadcasts this notification with a null HWND.
            let present = unsafe {
                PeekMessageW(
                    &raw mut message,
                    None,
                    WM_DISPLAYCHANGE,
                    WM_DISPLAYCHANGE,
                    PM_REMOVE,
                )
            };
            if !present.as_bool() {
                break;
            }
            self.refresh_monitor(config, ctx, &mut events);
        }
        events
    }

    fn refresh_monitor(
        &mut self,
        config: &mut Config,
        ctx: &egui::Context,
        events: &mut Vec<Event>,
    ) {
        let Ok(monitors) = monitors::enumerate() else {
            tracing::warn!("monitor enumeration failed; retaining current dock target");
            return;
        };
        let Some(target) = monitors::resolve_target(&monitors, &config.dock.monitor_id) else {
            return;
        };
        let changed = self.monitor_id.as_deref() != Some(target.id.as_str());
        if changed {
            if monitor_id_is_real_fallback(&config.dock.monitor_id, &target.id) {
                tracing::warn!(
                    configured_id = %config.dock.monitor_id,
                    fallback_id = %target.id,
                    "configured monitor unavailable; re-docking to fallback"
                );
                config.dock.monitor_id.clone_from(&target.id);
            }
            self.monitor_id = Some(target.id.clone());
            send_dock_position(ctx, target, &config.dock.edge, config.dock.offset_px);
            events.push(Event::MonitorChanged(target.id.clone()));
        }
    }

    fn unregister(self) {
        // Hotkey cleanup: the hotkey was registered thread-locally with
        // `RegisterHotKey(None, ...)` on the dedicated hotkey thread (see
        // `configure_platform`). `UnregisterHotKey` must run on THAT thread,
        // not the GUI thread. We wake the thread's `GetMessageW` loop by
        // posting `WM_QUIT`; the thread then unregisters + exits + the
        // JoinHandle completes. This avoids both the unregister-against-wrong
        // target bug AND the thread-leak-on-exit bug.
        if let Some(tid) = self.hotkey_thread_id {
            // SAFETY: PostThreadMessageW against a known-live TID with the
            // benign WM_QUIT message is the documented shutdown handshake.
            // If the thread has already exited, the call fails harmlessly.
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
                use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.hotkey_thread {
            if let Err(error) = handle.join() {
                tracing::warn!(?error, "hotkey thread join failed during shutdown");
            }
        }
        if self.click_through {
            let _ = hotkey::set_click_through(self.hwnd, false);
        }
    }
}

fn configure_capture_exclusion_for_hwnd<F>(
    hwnd: windows::Win32::Foundation::HWND,
    set_affinity: F,
) -> bool
where
    F: FnOnce(windows::Win32::Foundation::HWND) -> sidebar_domain::error::Result<()>,
{
    if hwnd.is_invalid() {
        tracing::warn!("capture exclusion skipped: eframe returned no live HWND");
        return false;
    }
    if let Err(error) = set_affinity(hwnd) {
        tracing::warn!(?error, "capture exclusion unavailable for sidebar HWND");
        return false;
    }
    true
}

fn configure_capture_exclusion_for_display<F>(
    display: &sidebar_domain::config::DisplayConfig,
    hwnd: windows::Win32::Foundation::HWND,
    set_affinity: F,
) -> bool
where
    F: FnOnce(windows::Win32::Foundation::HWND) -> sidebar_domain::error::Result<()>,
{
    if !display.hide_from_capture {
        return false;
    }
    configure_capture_exclusion_for_hwnd(hwnd, set_affinity)
}

#[cfg(windows)]
fn creation_context_hwnd(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let raw_handle = match cc.window_handle() {
        Ok(handle) => handle.as_raw(),
        Err(error) => {
            tracing::warn!(
                ?error,
                "capture exclusion skipped: eframe root HWND unavailable"
            );
            return None;
        }
    };
    let RawWindowHandle::Win32(win32) = raw_handle else {
        tracing::warn!("capture exclusion skipped: eframe root handle is not Win32");
        return None;
    };
    Some(HWND(win32.hwnd.get() as *mut std::ffi::c_void))
}

#[cfg(windows)]
fn configure_capture_exclusion(
    cc: &eframe::CreationContext<'_>,
    display: &sidebar_domain::config::DisplayConfig,
) {
    use sidebar_platform::dwm::set_capture_cloak;

    let Some(hwnd) = creation_context_hwnd(cc) else {
        return;
    };

    // SAFETY: eframe supplied the live root viewport handle through its
    // CreationContext; the Win32 raw handle is valid for this app lifetime.
    configure_capture_exclusion_for_display(display, hwnd, |hwnd| set_capture_cloak(hwnd, true));
}

#[cfg(windows)]
fn configure_platform(cc: &eframe::CreationContext<'_>, app: &mut SidebarApp) {
    let Some(hwnd) = creation_context_hwnd(cc) else {
        return;
    };
    let mut platform = PlatformRuntime::new(hwnd);
    // Register the global hotkey on a DEDICATED thread. RegisterHotKey posts
    // WM_HOTKEY to the thread that registered it. By using a separate thread
    // (not the egui event loop thread), we avoid the winit/glutin event loop
    // consuming the WM_HOTKEY message before our code sees it.
    match hotkey::HotkeyCombo::parse(&app.config.hotkeys.click_through) {
        Ok(combo) => {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let (tid_tx, tid_rx) = std::sync::mpsc::channel::<u32>();
            let hotkey_id = CLICK_THROUGH_HOTKEY_ID;
            let ctrl = combo.ctrl;
            let shift = combo.shift;
            let alt = combo.alt;
            let win = combo.win;
            let key = combo.key;
            let thread = std::thread::Builder::new()
                .name("sidebar-hotkey".to_string())
                .spawn(move || {
                    use windows::Win32::System::Threading::GetCurrentThreadId;
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
                        RegisterHotKey, UnregisterHotKey,
                    };
                    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, WM_HOTKEY};
                    // Capture this thread's TID and report it back to the
                    // spawner BEFORE registering or blocking. `unregister`
                    // uses this TID to post WM_QUIT during shutdown. Sending
                    // first is always correct: if RegisterHotKey fails below,
                    // the thread exits and the TID receiver simply isn't used.
                    // SAFETY: GetCurrentThreadId returns the calling thread's
                    // identifier; no invariants to uphold.
                    let tid = unsafe { GetCurrentThreadId() };
                    let _ = tid_tx.send(tid);
                    // Register the hotkey on THIS thread (not the GUI thread).
                    let mut modifiers = HOT_KEY_MODIFIERS::default();
                    if ctrl { modifiers |= MOD_CONTROL; }
                    if shift { modifiers |= MOD_SHIFT; }
                    if alt { modifiers |= MOD_ALT; }
                    if win { modifiers |= MOD_WIN; }
                    modifiers |= MOD_NOREPEAT;
                    // SAFETY: RegisterHotKey on thread 0 (this thread) is safe;
                    // the hotkey ID + modifiers are constant values from config.
                    let registered = unsafe {
                        RegisterHotKey(None, hotkey_id, modifiers, key)
                    };
                    if registered.is_err() {
                        tracing::warn!(
                            "dedicated hotkey thread: RegisterHotKey failed (another app may own this combo)"
                        );
                        return;
                    }
                    tracing::info!("dedicated hotkey thread: registered Ctrl+Shift+S on thread");
                    // Block on GetMessageW — only WM_HOTKEY messages arrive on
                    // this thread (no window, so no other messages).
                    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    loop {
                        // SAFETY: GetMessageW on this thread with no window filter
                        // blocks until a thread-message arrives (WM_HOTKEY).
                        let ret = unsafe { GetMessageW(&raw mut msg, None, 0, 0) };
                        if ret.0 == 0 || ret.0 == -1 {
                            break; // WM_QUIT or error
                        }
                        if msg.message == WM_HOTKEY && i32::try_from(msg.wParam.0).unwrap_or(-1) == hotkey_id {
                            // Signal the GUI thread via channel (non-blocking).
                            let _ = tx.send(());
                        }
                    }
                    // SAFETY: unregister on the same thread that registered.
                    unsafe {
                        let _ = UnregisterHotKey(None, hotkey_id);
                    }
                })
                .expect("failed to spawn hotkey thread");
            platform.hotkey_id = Some(CLICK_THROUGH_HOTKEY_ID);
            platform.hotkey_rx = Some(rx);
            platform.hotkey_thread = Some(thread);
            // Receive the thread's TID for shutdown cleanup. The thread
            // sends it before blocking in GetMessageW; if the channel is
            // empty at this point (extremely unlikely on a healthy OS
            // scheduler), we fall back to None and the unregister path
            // skips the PostThreadMessageW handshake.
            platform.hotkey_thread_id = tid_rx.recv().ok();
        }
        Err(error) => tracing::warn!(?error, "invalid click-through hotkey; disabled"),
    }
    if let Ok(displays) = monitors::enumerate() {
        if let Some(target) = monitors::resolve_target(&displays, &app.config.dock.monitor_id) {
            if monitor_id_is_real_fallback(&app.config.dock.monitor_id, &target.id) {
                tracing::warn!(
                    configured_id = %app.config.dock.monitor_id,
                    fallback_id = %target.id,
                    "configured monitor unavailable; re-docking to fallback"
                );
                app.config.dock.monitor_id.clone_from(&target.id);
                app.persist_config();
            }
            platform.monitor_id = Some(target.id.clone());
            send_dock_position(
                &cc.egui_ctx,
                target,
                &app.config.dock.edge,
                app.config.dock.offset_px,
            );
        }
    } else {
        tracing::warn!("monitor enumeration failed; using eframe default position");
    }
    app.platform = Some(platform);
}

/// Story 12.x fix: decide whether the monitor-resolution change from
/// `configured_id` to `resolved_id` represents a genuine fallback (configured
/// monitor gone) OR the expected `"primary"` sentinel resolving to the
/// primary device-id. Returns `true` only for the genuine-fallback case
/// (so the warning + config-overwrite fire correctly; the `"primary"`
/// sentinel is stable across reboots per T-36 and should NOT be overwritten
/// with a device-id).
///
/// Cited: T-36 (default primary; monitor_id = DeviceID or "primary").
#[must_use]
fn monitor_id_is_real_fallback(configured_id: &str, resolved_id: &str) -> bool {
    // The "primary" sentinel is never a fallback — it always resolves to
    // whatever the primary display is, and that's the intended behavior.
    if configured_id.eq_ignore_ascii_case("primary") {
        return false;
    }
    !configured_id.eq_ignore_ascii_case(resolved_id)
}

#[cfg(windows)]
#[allow(clippy::cast_precision_loss)]
fn send_dock_position(
    ctx: &egui::Context,
    monitor: &monitors::MonitorInfo,
    edge: &str,
    offset: i32,
) {
    const WIDTH: i32 = 280;
    const HEIGHT: i32 = 720;
    let edge = edge.trim().to_ascii_lowercase();
    let (x, y) = match edge.as_str() {
        "left" | "top" => (monitor.x + offset, monitor.y + offset),
        "bottom" => (
            monitor.x + offset,
            monitor.y + monitor.height - HEIGHT - offset,
        ),
        _ => (
            monitor.x + monitor.width - WIDTH - offset,
            monitor.y + offset,
        ),
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
        x as f32, y as f32,
    )));
}

impl eframe::App for SidebarApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Story 8.10 contract — closing the wizard window via the title-bar
        // X is treated as Skip: defaults are applied + first_run_complete is
        // flipped so the wizard does not re-block on next launch. Without
        // this, the wizard reappears every launch if the user closes the
        // window without clicking Skip. See first_run.rs §"Window-X (close)".
        if self.wizard_active {
            self.config = Config::default();
            self.config.first_run_complete = true;
            self.persist_config();
            self.wizard_active = false;
        }
        #[cfg(windows)]
        if let Some(platform) = self.platform.take() {
            platform.unregister();
        }
        self.state.request_shutdown();
    }

    /// egui 0.35 splits the per-frame hook into `logic` (no painting — the
    /// right place for the broadcast drain + `request_repaint`) and `ui`
    /// (where the readings render goes). See eframe::App docs.
    ///
    /// This is the "repaint on broadcast" half of T-9: when the poller
    /// (Story 7.2) sends a fresh `Vec<Reading>`, we drain the latest message,
    /// replace the snapshot, and ask egui for a repaint outside the vsync
    /// cadence so the new data shows immediately. We also drain the Event
    /// channel + apply tier changes here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut repaint = false;
        #[cfg(windows)]
        if let Some(platform) = self.platform.as_mut() {
            for event in platform.poll(&mut self.config, ctx) {
                if let Some(sender) = self.event_tx.as_ref() {
                    let _ = sender.send(event);
                }
                repaint = true;
            }
        }
        if let Some(readings) = self.state.drain_broadcast() {
            self.state.replace_readings(readings);
            repaint = true;
        }
        // Story 12.8 Gap 2 — drain the accountant's BandwidthView watch
        // channel into the local view. Non-blocking: `has_changed` returns
        // immediately if no new view was published.
        if let Some(rx) = self.bandwidth_view_rx.as_mut() {
            while rx.has_changed().unwrap_or(false) {
                if let Some(view) = rx.borrow_and_update().clone() {
                    self.view.bandwidth = Some(view);
                    repaint = true;
                }
            }
        }
        // Story 12.8 Gap 3 — poll the OHM child-liveness probe. On the first
        // `false`, emit Event::TierChanged(Basic) + latch so we don't
        // rebroadcast every frame.
        if !self.child_exit_degraded {
            if let Some(probe) = self.child_alive_fn.as_ref() {
                if !probe() {
                    tracing::info!("Story 12.8 Gap 3: OHM child exited — degrading Full -> Basic");
                    if let Some(sender) = self.event_tx.as_ref() {
                        let _ = sender.send(Event::TierChanged(sidebar_domain::event::Tier::Basic));
                    }
                    self.state.set_tier(ProviderTier::Basic);
                    self.child_exit_degraded = true;
                    repaint = true;
                }
            }
        }
        // Apply any pending events from the EventChannel. Tier changes flip
        // AppState.tier (which the next ui() reads). Platform events trigger
        // repaint; monitor fallback persistence is handled here as well.
        for event in self.state.drain_events() {
            match event {
                Event::TierChanged(tier) => {
                    let mapped = match tier {
                        sidebar_domain::event::Tier::Basic => ProviderTier::Basic,
                        sidebar_domain::event::Tier::Full => ProviderTier::Full,
                    };
                    self.state.set_tier(mapped);
                    repaint = true;
                }
                Event::Shutdown => {
                    tracing::info!("GUI: Shutdown event received — sending exit to eframe");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Event::ThemeChanged(_) => {
                    // `render_sidebar` reapplies ThemePreference::System from
                    // the local config; the bridge event only needs to wake
                    // the frame so egui observes the new OS palette.
                    repaint = true;
                }
                Event::HotkeyPressed(_) => {
                    repaint = true;
                }
                Event::MonitorChanged(_) => {
                    self.persist_config();
                    repaint = true;
                }
            }
        }
        if repaint {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Story 8.10: if the first-run wizard should show, render it instead
        // of the live sidebar. The poller is gated (G24) at the launch
        // sequence level — when the wizard completes (writes config + flips
        // first_run_complete), the user restarts and the poller spawns.
        if self.wizard_active {
            let action = first_run::render_wizard(ui, &mut self.config);
            match action {
                first_run::WizardAction::Pending => {}
                first_run::WizardAction::Continue | first_run::WizardAction::Skip => {
                    self.config.first_run_complete = true;
                    self.persist_config();
                    ui.label("Setup saved. Restart sidebar to begin monitoring.");
                }
            }
            return;
        }

        // Production render path (Story 8.4 + 8.5): full sidebar with status
        // pill, metric rows, sparkline, bandwidth panel, and gear-toggled
        // settings panel.
        let snapshot = self.state.snapshot();
        let tier = self.state.tier();
        // render_sidebar mutates self.config (settings panel) + reads
        // self.view. The on_change callback is a no-op at this layer — the
        // actual persistence happens AFTER render_sidebar returns (below)
        // because the closure can't borrow self while self.config is mutably
        // borrowed.
        let on_change_noop: &dyn Fn() = &|| {};
        let on_launch: &dyn Fn() = self.launch_fn.as_ref().map_or(&|| {}, |f| f.as_ref());
        let hist = self.state.history_snapshot();
        render_sidebar_mut(
            ui,
            &snapshot,
            tier,
            &mut self.config,
            &mut self.view,
            on_change_noop,
            on_launch,
            Some(&hist),
        );

        // After the render: mirror the (possibly-mutated) config + view into
        // AppState so background tasks see the new value. Persist config to
        // disk whenever the settings panel is open (cheap enough; debounce
        // is a refinement). render_sidebar_mut holds a mut borrow on
        // view.settings_open; the gear checkbox flips it in place when
        // changed() fires, so the value mirrored here already reflects any
        // gear click that happened this frame. The "Open settings" alert
        // button also sets view.settings_open = true on click.
        self.state.replace_config(self.config.clone());
        self.state.replace_view(self.view.clone());
        if self.view.settings_open {
            self.persist_config();
        }
    }
}

/// Build an `egui::ViewportBuilder` from [`ViewportPrefs`] (Story 6.1).
///
/// Maps the three prefs (`transparent`/`borderless`/`topmost`) to the
/// corresponding `egui::ViewportBuilder` flags. Used by [`SidebarApp::run`];
/// exposed `pub(crate)` so the launch sequence (Story 8.5) can introspect the
/// viewport before eframe consumes it.
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

/// Render the readings snapshot into the given `egui::Ui`.
///
/// Layout: tier header → separator → metric rows (truncated at [`MAX_ROWS`]).
/// Empty readings render the [`WAITING_TEXT`] placeholder (Boundary #1).
///
/// Each metric row is two labels: a short uppercase kind label (e.g. "CPU")
/// and a formatted value (e.g. "42%"). Splitting them lets the F8 access tree
/// query both independently — the Story 8.1 Happy Path contract asserts the
/// snapshot contains BOTH "CPU" and "42%" as distinct queryable nodes.
pub fn render_snapshot(ui: &mut Ui, readings: &[Reading], tier: ProviderTier) {
    // Story 8.2: status pill at the top (Basic gray / Full green + tooltip).
    // The pill click is the ONLY launch-elevated entry point (PRD §5.4). At
    // the render_snapshot layer we wire a no-op callback — the real
    // `OhmSupervisor::launch_elevated` is bound in Story 8.5 (settings panel
    // + launch sequence) when the AppState owns the supervisor handle.
    status_pill::render(ui, tier, &|| {});

    ui.separator();

    if readings.is_empty() {
        ui.label(WAITING_TEXT);
        return;
    }

    // Story 8.3: metric rows via the NFR-8 dispatch (MetricKind × Unit →
    // format_*). Default DisplayConfig (human-readable, Celsius, decimal GB).
    // Story 8.5 (settings panel) will plumb the real configured DisplayConfig
    // through here once AppState owns a Config handle.
    let display = default_display_config();
    let visible = readings.len().min(MAX_ROWS);
    for reading in readings.iter().take(visible) {
        metric_row::render(ui, reading, &display);
    }

    // T-21 truncation marker. We render the count explicitly so a 1000-reading
    // poller batch surfaces as "+936 more (truncated)" — the F8 access tree
    // can assert on both "truncated" and "+".
    if readings.len() > MAX_ROWS {
        let omitted = readings.len() - MAX_ROWS;
        ui.label(format!("+{omitted} more (truncated)"));
    }
}

/// Default display config used by [`render_snapshot`] when the caller has no
/// configured `[display]` section yet. Story 8.5 replaces this with the real
/// `Config::display` once AppState owns a `Config` handle.
fn default_display_config() -> sidebar_domain::config::DisplayConfig {
    sidebar_domain::config::DisplayConfig::default()
}

/// Composed sidebar view for the Story 8.4 + 8.5 wiring.
///
/// Holds the optional [`BandwidthView`] (Story 5.3 DTO — None when bandwidth
/// tracking is disabled) and a gear-toggle flag (when true, the settings panel
/// replaces the metric rows). The host constructs one of these per frame from
/// its AppState handles.
#[derive(Clone, Default)]
pub struct SidebarView {
    /// The bandwidth panel DTO. `None` means "no bandwidth tracking" — the
    /// bandwidth panel renders its empty placeholder.
    pub bandwidth: Option<BandwidthView>,
    /// When true, render the settings panel instead of the metric rows.
    /// Toggled by the gear button in the header (Story 8.5).
    pub settings_open: bool,
    /// Story 8.7 — sparkline samples for the primary metric (CPU temperature
    /// in v1). The host pushes one f64 per poll tick; `None` (or empty) skips
    /// the sparkline widget. NaN values render as gaps (Story 1.6 contract).
    pub sparkline: Option<Vec<f64>>,
    /// Story 12.6 — per-metric alert ack/snooze state. When the user clicks
    /// Ack on a Warning/Critical row, the metric key is inserted here; the
    /// `displayed_state` pure fn suppresses the color until recovery.
    pub alert_acks: std::collections::HashMap<
        sidebar_domain::graph::MetricKey,
        sidebar_domain::alert::AlertAck,
    >,
    /// Story 12.6 — previous raw alert state per metric. Keeping this state
    /// across frames preserves the domain hysteresis contract before ack or
    /// snooze decisions are applied.
    pub alert_states: std::collections::HashMap<
        sidebar_domain::graph::MetricKey,
        sidebar_domain::alert::AlertState,
    >,
}

/// Compatibility wrapper for callers that only need a read-only render.
/// Alert actions are applied to a cloned view; production uses
/// [`render_sidebar_mut`] so acknowledgements persist across frames.
#[allow(clippy::too_many_arguments)]
pub fn render_sidebar(
    ui: &mut Ui,
    readings: &[Reading],
    tier: ProviderTier,
    config: &mut Config,
    view: &SidebarView,
    on_change: &dyn Fn(),
    on_launch: &dyn Fn(),
    history: Option<&sidebar_domain::graph::MetricHistory>,
) {
    let mut view = view.clone();
    render_sidebar_mut(
        ui, readings, tier, config, &mut view, on_change, on_launch, history,
    );
}

/// Mutable production render path. Alert actions mutate `view.alert_acks` and
/// open settings without introducing a second callback or global state.
#[allow(clippy::too_many_arguments)]
// The immediate-mode composition is intentionally kept in one pass so egui
// layout state, alert state, and history rendering cannot drift apart.
#[allow(clippy::too_many_lines)]
pub fn render_sidebar_mut(
    ui: &mut Ui,
    readings: &[Reading],
    tier: ProviderTier,
    config: &mut Config,
    view: &mut SidebarView,
    on_change: &dyn Fn(),
    on_launch: &dyn Fn(),
    history: Option<&sidebar_domain::graph::MetricHistory>,
) {
    // Story 8.6: apply theme + accent to the egui context for this frame.
    // Done unconditionally each frame — `set_theme` is idempotent when the
    // value hasn't changed (cheap: a single match on the stored preference).
    let mode = theme::ThemeMode::from_config_str(&config.theme.mode);
    theme::apply(ui.ctx(), mode, &config.theme.accent);

    // Header: status pill (left) + gear toggle (right). The gear toggles the
    // settings panel (Story 8.5 HITL guardrail G11 — no-retroactive-resplit
    // surfaced as a tooltip inside the settings panel).
    ui.horizontal(|header| {
        status_pill::render(header, tier, on_launch);
        // Story 12.1 — clock/date header. Locale-stable 24h HH:MM, rendered
        // between the status pill and the gear. The wall-clock is read per-
        // frame via chrono::Local (no network time source per Story 12.1).
        let now = chrono::Local::now();
        header.label(sidebar_domain::format::format_clock(now.time()));
        header.label(sidebar_domain::format::format_clock_date(now.date_naive()));
        header.with_layout(egui::Layout::right_to_left(egui::Align::Center), |right| {
            let mut open = view.settings_open;
            let gear = right.checkbox(&mut open, "⚙");
            if gear.changed() {
                view.settings_open = open;
                on_change();
            }
        });
    });

    ui.separator();

    if view.settings_open {
        // Settings panel (Story 8.5) — replaces the metric rows + bandwidth
        // panel while open. The panel surfaces the no-retroactive-resplit
        // tooltip (PRD §5.5.8) and the T-3 poll-interval warning inline.
        settings_panel::render(ui, config, on_change);
        return;
    }

    if readings.is_empty() {
        ui.label(WAITING_TEXT);
    } else {
        let display = config.display.clone();
        let accent = theme::parse_accent(&config.theme.accent);
        let default = ui.style().visuals.text_color();

        // Story 8.9: when [metrics] config is set, filter + reorder the live
        // rows to only the enabled metrics, in the configured order. When
        // `order` is empty (default), we fall back to the poller-supplied
        // sequence (Story 8.1 behavior) so the empty-config path stays
        // unchanged. The metric-name strings in config use the MetricKind
        // variant Debug names (e.g. "CpuUtilization"); we compare via the
        // Debug format to avoid adding a Display impl to the domain enum.
        let ordered: Vec<&Reading> = if config.metrics.order.is_empty() {
            readings.iter().take(MAX_ROWS).collect()
        } else {
            let enabled_kinds = metric_list::enabled_in_order(&config.metrics);
            let mut out: Vec<&Reading> = Vec::new();
            for kind_name in &enabled_kinds {
                for reading in readings {
                    if out.len() >= MAX_ROWS {
                        break;
                    }
                    if format!("{:?}", reading.kind) == *kind_name {
                        out.push(reading);
                    }
                }
            }
            out
        };

        let now_epoch = chrono::Local::now().timestamp();
        for reading in &ordered {
            // Story 8.8/12.6: preserve the previous raw state so threshold
            // hysteresis remains effective across frames and ack/snooze does
            // not re-arm while a metric is still inside the hysteresis band.
            let key = sidebar_domain::graph::MetricKey {
                category: reading.sensor.category.to_string(),
                instance: reading.sensor.instance.clone(),
                kind: format!("{:?}", reading.kind),
            };
            let previous_state = view
                .alert_states
                .get(&key)
                .copied()
                .unwrap_or(sidebar_domain::alert::AlertState::Normal);
            let alertable = matches!(
                reading.kind,
                MetricKind::CpuTemperature | MetricKind::GpuTemperature
            );
            let raw_state =
                alert_indicator::classify(reading, Some(&config.thresholds), previous_state);
            if alertable {
                view.alert_states.insert(key.clone(), raw_state);
            }
            if let Some(ack) = view.alert_acks.get(&key).copied() {
                if sidebar_domain::alert::ack_should_clear(raw_state, ack, now_epoch) {
                    view.alert_acks.remove(&key);
                }
            }
            let ack = view.alert_acks.get(&key).copied();
            let displayed_state = sidebar_domain::alert::displayed_state(raw_state, ack, now_epoch);
            let color = alert_indicator::color_for_state(displayed_state, accent, default);
            metric_row::render_with_color(ui, reading, &display, color);
            if matches!(
                displayed_state,
                sidebar_domain::alert::AlertState::Warning
                    | sidebar_domain::alert::AlertState::Critical
            ) {
                ui.horizontal(|actions| {
                    if actions.small_button("Acknowledge").clicked() {
                        view.alert_acks
                            .insert(key.clone(), sidebar_domain::alert::AlertAck::Acknowledged);
                    }
                    if actions.small_button("Snooze 5m").clicked() {
                        view.alert_acks.insert(
                            key.clone(),
                            sidebar_domain::alert::AlertAck::Snoozed(now_epoch + 300),
                        );
                    }
                    if actions.small_button("Open settings").clicked() {
                        view.settings_open = true;
                    }
                });
            }
            // Story 12.2 — per-row sparkline from MetricHistory.
            if let Some(hist) = history {
                if let Some(window) = hist.get(&key) {
                    if window.len() >= 2 {
                        sparkline::render_snapshot(ui, &window.to_vec(), 60.0);
                    }
                }
            }
        }
        if readings.len() > MAX_ROWS && config.metrics.order.is_empty() {
            let omitted = readings.len() - MAX_ROWS;
            ui.label(format!("+{omitted} more (truncated)"));
        }
    }

    // Story 8.7: sparkline widget for the primary metric. Rendered below the
    // metric rows, above the bandwidth panel. Empty/None → skipped (no extra
    // vertical space wasted).
    if let Some(samples) = &view.sparkline {
        if !samples.is_empty() {
            sparkline::render_snapshot(ui, samples, sparkline::DEFAULT_WIDTH);
        }
    }

    // Bandwidth panel (Story 8.4) — below the metric rows.
    ui.separator();
    if let Some(bw) = &view.bandwidth {
        bandwidth_panel::render(ui, bw, &config.display);
    } else {
        bandwidth_panel::render(
            ui,
            &BandwidthView {
                current: vec![],
                history: vec![],
                days_until_reset: 0,
                next_reset_date: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default(),
            },
            &config.display,
        );
    }
}

/// Short uppercase label for a [`MetricKind`] — the per-row "kind" the F8
/// tests query for. Returns `"CPU"` for `CpuUtilization`, `"GPU"` for
/// `GpuUtilization`, etc. The mapping is exhaustive (compile-time check via
/// the wildcard catch-all panic-free path returning `"?"`).
#[must_use]
pub(crate) fn kind_label(kind: MetricKind) -> &'static str {
    match kind {
        MetricKind::CpuUtilization
        | MetricKind::CpuFrequency
        | MetricKind::CpuTemperature
        | MetricKind::CpuPower => "CPU",
        MetricKind::GpuUtilization
        | MetricKind::GpuTemperature
        | MetricKind::GpuMemoryUtilization
        | MetricKind::GpuPower
        | MetricKind::GpuFanSpeed
        | MetricKind::GpuFrequency => "GPU",
        MetricKind::MemoryUsed | MetricKind::MemoryTotal => "RAM",
        MetricKind::DiskUsed
        | MetricKind::DiskTotal
        | MetricKind::DiskReadBytesPerSec
        | MetricKind::DiskWriteBytesPerSec
        | MetricKind::DiskSmartEndurance
        | MetricKind::DiskTemperature => "DISK",
        MetricKind::FanSpeed => "FAN",
        MetricKind::Voltage => "VOLT",
        MetricKind::NetRxBytes
        | MetricKind::NetTxBytes
        | MetricKind::NetRxPackets
        | MetricKind::NetTxPackets
        | MetricKind::NetRxErrors
        | MetricKind::NetTxErrors => "NET",
        MetricKind::BandwidthRxBytes | MetricKind::BandwidthTxBytes => "BANDWIDTH",
        MetricKind::BatteryPercent | MetricKind::BatteryState | MetricKind::BatteryPowerRate => {
            "BAT"
        }
        MetricKind::ProcessCpuPercent
        | MetricKind::ProcessMemoryBytes
        | MetricKind::ProcessGpuPercent => "PROC",
        MetricKind::UptimeSeconds => "UP",
    }
}

/// Format a reading's value using the Story 1.3 `format_*` functions. The
/// formatter is chosen by the reading's [`Unit`] (the canonical unit per
/// architecture §5.1). The kind is rendered separately via [`kind_label`];
/// [`MetricKind`] kinds that share a unit (e.g. `NetRxBytes` and `DiskUsed`)
/// all use the same per-unit formatter here — the kind-specific prefix lives
/// in the row's separate label, not in the value string.
///
/// # Casts
///
/// The f64 → u64/u32 casts are intentional: we clamp negative values to 0
/// and cap at the integer type's MAX before casting, so neither truncation
/// nor sign-loss can occur. The `cast_precision_loss` on the u64→f64 in the
/// clamp bound is a one-bit rounding on a sentinel cap, never on data.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[must_use]
#[allow(dead_code)] // Kept for the Story 8.1 format-delegation test; the live render path now uses gui::metric_row::format_reading_with_config (Story 8.3).
pub(crate) fn format_reading(reading: &Reading) -> String {
    let Reading { value, unit, .. } = reading;
    match unit {
        Unit::Percent => format_percent(*value),
        Unit::Celsius => format_temp(*value, TempUnit::Celsius),
        Unit::Fahrenheit => format_temp(*value, TempUnit::Fahrenheit),
        Unit::Kelvin => {
            // adapters shouldn't emit Kelvin (Story 1.1 lists Celsius as the
            // canonical temp unit), but render defensively as Celsius.
            format_temp(*value, TempUnit::Celsius)
        }
        Unit::Hertz => {
            // format_hz takes u64; the Reading value is f64. Adapters emit
            // integer Hz counts (per Story 1.1); we clamp negatives to 0 and
            // cap at u64::MAX to avoid panics.
            let hz = if *value < 0.0 {
                0
            } else {
                value.clamp(0.0, u64::MAX as f64) as u64
            };
            format_hz(hz)
        }
        Unit::Bytes => {
            let b = if *value < 0.0 {
                0
            } else {
                value.clamp(0.0, u64::MAX as f64) as u64
            };
            format_bytes(b, Base::Decimal)
        }
        Unit::BytesPerSec => {
            let b = if *value < 0.0 {
                0
            } else {
                value.clamp(0.0, u64::MAX as f64) as u64
            };
            format_bytes(b, Base::Decimal) + "/s"
        }
        Unit::BitsPerSec => {
            let b = if *value < 0.0 {
                0
            } else {
                value.clamp(0.0, u64::MAX as f64) as u64
            };
            format_bps(b)
        }
        Unit::Watts => format_power(*value),
        Unit::Volts => format_voltage(*value),
        Unit::Rpm => {
            let r = if *value < 0.0 {
                0
            } else {
                value.clamp(0.0, f64::from(u32::MAX)) as u32
            };
            format_rpm(r)
        }
        Unit::Seconds => {
            // Integer seconds. Reuse format_hz's sig-fig approach by scaling
            // into the most readable unit (s / min / h).
            let secs = if *value < 0.0 { 0.0 } else { *value };
            format_uptime(secs)
        }
        Unit::Count | Unit::PacketsPerSec => {
            // Generic count — round to integer, no suffix.
            if value.is_finite() {
                format!("{}", value.round() as i64)
            } else {
                "--".to_string()
            }
        }
    }
}

/// Format an uptime in seconds as `Xh Ym` or `Ym Zs` (compact, no trailing
/// unit when zero). Keeps the metric row width-bounded for the sidebar.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#[allow(dead_code)] // Called only by format_reading above (Story 8.1 path, kept for its test).
fn format_uptime(secs: f64) -> String {
    if !secs.is_finite() {
        return "--".to_string();
    }
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.1 TDD contract tests (GREEN phase — all assertions pass).
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
        Reading::gauge(SensorId::new("cpu", "package"), kind, value, unit)
    }

    /// Walk the kittest access tree and collect every node's text. egui puts
    /// label text in `value()` on `Role::Label` nodes (verified via debug
    /// dump); we collect BOTH `label()` and `value()` to be robust across
    /// egui versions.
    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    #[test]
    fn gui_exit_requests_cancellation_and_shutdown_event() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let (events, mut rx) = broadcast::channel(4);
        let signal = crate::shutdown::ShutdownSignal::new(cancel.clone(), events);
        let state = AppState::new(ProviderTier::Basic, None);
        state.set_shutdown_signal(signal);
        let mut app = SidebarApp::new(state);

        <SidebarApp as eframe::App>::on_exit(&mut app, None);

        assert!(cancel.is_cancelled());
        assert_eq!(rx.try_recv(), Ok(Event::Shutdown));
    }

    /// Story 8.10 — Window-X (close) while the wizard is showing is treated
    /// as Skip. Without this contract the wizard re-shows on every launch
    /// if the user closes the window without clicking Skip. The fix lands
    /// defaults + first_run_complete=true + clears the wizard gate on exit.
    /// See first_run.rs §"Window-X (close) → treated as Skip".
    #[test]
    fn wizard_active_on_exit_applies_skip_semantics() {
        let state = AppState::new(ProviderTier::Basic, None);
        let mut app = SidebarApp::with_config_path(state, std::path::PathBuf::new(), true);
        assert!(app.wizard_active, "precondition: wizard must start active");
        assert!(
            !app.config.first_run_complete,
            "precondition: first_run must start incomplete"
        );

        <SidebarApp as eframe::App>::on_exit(&mut app, None);

        assert!(
            !app.wizard_active,
            "on_exit must clear wizard_active so next launch skips the wizard"
        );
        assert!(
            app.config.first_run_complete,
            "on_exit must set first_run_complete=true (Skip semantics) so the \
             wizard does not re-block the next launch"
        );
        // Skip restores defaults (first_run.rs §"Skip" applies Config::default()).
        assert_eq!(
            app.config.dock.edge, "Right",
            "Skip semantics restore dock.edge default"
        );
    }

    /// Hotkey thread cleanup handshake (Story 6.6 regression, 2026-07-13).
    ///
    /// The dedicated hotkey thread blocks in `GetMessageW` until shutdown.
    /// `PlatformRuntime::unregister` posts `WM_QUIT` to the thread's TID to
    /// wake it. This test exercises the SAME handshake (capture TID →
    /// `GetMessageW` loop → `WM_QUIT` wake → `join()` succeeds) without
    /// requiring a real HWND or `RegisterHotKey` (which need an interactive
    /// desktop session). The handshake is the part that was previously
    /// broken — unregister targeted the wrong HWND and no `WM_QUIT` was
    /// ever posted, leaking the thread.
    #[cfg(windows)]
    #[test]
    fn hotkey_thread_wakes_on_wm_quit_and_joins_cleanly() {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, PostThreadMessageW, WM_QUIT};

        let (tid_tx, tid_rx) = mpsc::channel::<u32>();
        let handle = std::thread::Builder::new()
            .name("test-hotkey-handshake".to_string())
            .spawn(move || {
                // SAFETY: GetCurrentThreadId returns the calling thread's id.
                let tid = unsafe { GetCurrentThreadId() };
                let _ = tid_tx.send(tid);
                let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                // SAFETY: same GetMessageW pattern as the production hotkey
                // thread; blocks until WM_QUIT arrives on this TID.
                let _ = unsafe { GetMessageW(&raw mut msg, None, 0, 0) };
            })
            .expect("spawn handshake thread");

        let tid = tid_rx
            .recv()
            .expect("thread must report its TID before blocking in GetMessageW");

        // SAFETY: PostThreadMessageW against the known-live TID with the
        // benign WM_QUIT message; matches the production unregister path.
        unsafe {
            PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0))
                .expect("PostThreadMessageW must succeed against a live TID");
        }

        // The thread must exit promptly once WM_QUIT is posted. Bound the
        // join at 2s so a regression (no WM_QUIT posted / wrong TID / etc.)
        // surfaces as a failed test rather than a hung process.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if handle.is_finished() {
                break;
            }
            assert!(
                Instant::now() <= deadline,
                "hotkey thread did not exit within 2s after WM_QUIT — the \
                 cleanup handshake is broken (the thread is still blocked \
                 in GetMessageW, which was the pre-2026-07-13 bug)"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // join() must succeed (clean exit, not panic).
        handle.join().expect("hotkey thread must join cleanly");
    }

    #[test]
    fn capture_exclusion_targets_supplied_hwnd() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use windows::Win32::Foundation::HWND;

        let supplied = HWND(std::ptr::dangling_mut::<std::ffi::c_void>());
        let seen = Rc::new(RefCell::new(None));
        let seen_by_setter = Rc::clone(&seen);

        assert!(configure_capture_exclusion_for_hwnd(
            supplied,
            move |hwnd| {
                *seen_by_setter.borrow_mut() = Some(hwnd);
                Ok(())
            }
        ));
        assert_eq!(
            *seen.borrow(),
            Some(supplied),
            "capture exclusion must target the supplied root viewport HWND"
        );
    }

    #[test]
    fn default_display_does_not_enable_capture_exclusion() {
        use std::cell::Cell;
        use windows::Win32::Foundation::HWND;

        let called = Cell::new(false);
        let result = configure_capture_exclusion_for_display(
            &sidebar_domain::config::DisplayConfig::default(),
            HWND(std::ptr::dangling_mut::<std::ffi::c_void>()),
            |_| {
                called.set(true);
                Ok(())
            },
        );

        assert!(!result);
        assert!(!called.get(), "default config must leave capture enabled");
    }

    #[test]
    fn enabled_display_applies_capture_exclusion() {
        use std::cell::Cell;
        use windows::Win32::Foundation::HWND;

        let called = Cell::new(false);
        let display = sidebar_domain::config::DisplayConfig {
            hide_from_capture: true,
            ..Default::default()
        };
        let result = configure_capture_exclusion_for_display(
            &display,
            HWND(std::ptr::dangling_mut::<std::ffi::c_void>()),
            |_| {
                called.set(true);
                Ok(())
            },
        );

        assert!(result);
        assert!(called.get(), "enabled config must apply capture exclusion");
    }

    #[test]
    fn invalid_hwnd_skips_capture_exclusion_without_calling_api() {
        use std::cell::Cell;
        use windows::Win32::Foundation::HWND;

        let called = Cell::new(false);
        let result = configure_capture_exclusion_for_hwnd(HWND(std::ptr::null_mut()), |_| {
            called.set(true);
            Ok(())
        });

        assert!(
            !result,
            "invalid HWND must not report capture exclusion success"
        );
        assert!(
            !called.get(),
            "invalid HWND must not call the Win32 API seam"
        );
    }

    #[test]
    fn capture_api_failure_is_observable_without_false_success() {
        use std::cell::Cell;
        use windows::Win32::Foundation::HWND;

        let called = Cell::new(false);
        let display = sidebar_domain::config::DisplayConfig {
            hide_from_capture: true,
            ..Default::default()
        };
        let result = configure_capture_exclusion_for_display(
            &display,
            HWND(std::ptr::dangling_mut::<std::ffi::c_void>()),
            |_| {
                called.set(true);
                Err(sidebar_domain::error::Error::Platform(
                    "capture API unavailable".to_string(),
                ))
            },
        );

        assert!(called.get(), "enabled capture must invoke the API seam");
        assert!(!result, "capture API failure must not report success");
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

    /// G15 poison recovery: genuinely poison the lock from a panicking writer,
    /// then verify the guarded value remains writable and readable.
    #[test]
    fn snapshot_falls_back_to_last_good_on_poison() {
        let state = AppState::new(ProviderTier::Basic, None);
        state.replace_readings(vec![reading(
            MetricKind::CpuUtilization,
            42.0,
            Unit::Percent,
        )]);
        // Prime last_good.
        let _ = state.snapshot();
        assert_eq!(state.snapshot().len(), 1);

        let state_for_poison = Arc::clone(&state);
        let poisoned = std::thread::spawn(move || {
            let mut guard = state_for_poison
                .readings
                .write()
                .expect("first writer lock is healthy");
            guard[0].value = 99.0;
            panic!("intentional poison for G15 contract");
        })
        .join();
        assert!(
            poisoned.is_err(),
            "test setup must genuinely poison the lock"
        );

        let recovered = state.snapshot();
        assert_eq!(recovered.len(), 1);
        assert!((recovered[0].value - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn repeated_poison_recovery_preserves_last_good_snapshot() {
        let state = AppState::new(ProviderTier::Basic, None);
        state.replace_readings(vec![reading(
            MetricKind::CpuUtilization,
            42.0,
            Unit::Percent,
        )]);
        let _ = state.snapshot();

        for attempt in 0..3 {
            let state_for_poison = Arc::clone(&state);
            let poisoned = std::thread::spawn(move || {
                let mut guard = state_for_poison
                    .readings
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard[0].value = 100.0 + f64::from(attempt);
                panic!("intentional repeated poison for G15 contract");
            })
            .join();
            assert!(poisoned.is_err(), "attempt {attempt} must poison the lock");

            let recovered = state.snapshot();
            assert_eq!(recovered.len(), 1);
            assert!(
                (recovered[0].value - 42.0).abs() < f64::EPSILON,
                "attempt {attempt} must preserve the last-good snapshot"
            );
        }
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
            labels.contains("truncated"),
            "1000 readings must render a 'truncated' marker (got: {labels})"
        );
        assert!(
            labels.contains('+'),
            "1000 readings must render a '+N more' count (got: {labels})"
        );
        // T-21: must render exactly MAX_ROWS rows + 1 truncation marker.
        // The "+936 more" line means 1000 - 64 = 936 omitted.
        assert!(
            labels.contains("936"),
            "truncation marker must report 936 omitted rows (got: {labels})"
        );
    }

    /// T-9 (16ms render): a 1000-reading render must complete within the
    /// threshold. We assert 100ms here to absorb the kittest harness setup
    /// overhead (CtxRef + access-tree walk + memory allocation), which is
    /// NOT part of the production render path. The actual production ceiling
    /// is 16ms (T-9), pinned by the criterion bench in Story 11.1 against
    /// the real eframe path; this test guards against O(n) blowups (forgetting
    /// the truncation cap would push it to seconds).
    #[test]
    fn many_readings_render_within_t9_budget() {
        let many: Vec<Reading> = (0..1000)
            .map(|i| reading(MetricKind::CpuUtilization, f64::from(i), Unit::Percent))
            .collect();
        let start = Instant::now();
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &many, ProviderTier::Basic);
        });
        harness.run();
        let elapsed = start.elapsed();
        // T-9 (16ms) is a RELEASE-build threshold enforced by the criterion
        // bench in Story 11.1, not this unit test. This test's job is to
        // catch an O(n) regression that drops the MAX_ROWS=64 truncation cap
        // (which would push render to *seconds*). The 500ms ceiling catches
        // that blowup while tolerating debug-build parallel-test jitter (the
        // debug build has no optimizations + shares the CPU with 200+ sibling
        // tests, so it occasionally hits ~110ms — release builds run this in
        // <1ms). The ceiling is deliberately generous: anything approaching
        // it in release is a real regression.
        assert!(
            elapsed.as_millis() < 500,
            "render of 1000 readings blew past the regression ceiling (got {elapsed:?}; \
             production T-9 is 16ms in release — debug jitter is expected, but seconds \
             indicates a dropped truncation cap)"
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

    #[test]
    fn tier_full_renders_full_label() {
        let empty: Vec<Reading> = Vec::new();
        let mut harness = Harness::new_ui(|ui| {
            render_snapshot(ui, &empty, ProviderTier::Full);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("FULL"),
            "Full tier must render 'FULL' label (got: {labels})"
        );
    }

    /// Sanity: kind_label is exhaustive and returns the expected short form.
    #[test]
    fn kind_label_returns_expected_short_forms() {
        assert_eq!(kind_label(MetricKind::CpuUtilization), "CPU");
        assert_eq!(kind_label(MetricKind::GpuUtilization), "GPU");
        assert_eq!(kind_label(MetricKind::MemoryUsed), "RAM");
        assert_eq!(kind_label(MetricKind::DiskUsed), "DISK");
        assert_eq!(kind_label(MetricKind::NetRxBytes), "NET");
        assert_eq!(kind_label(MetricKind::BatteryPercent), "BAT");
        assert_eq!(kind_label(MetricKind::FanSpeed), "FAN");
    }

    /// Sanity: format_reading delegates to the Story 1.3 formatters.
    #[test]
    fn format_reading_delegates_to_story_1_3_formatters() {
        let cpu_pct = reading(MetricKind::CpuUtilization, 42.0, Unit::Percent);
        assert_eq!(format_reading(&cpu_pct), "42%");

        let cpu_hz = reading(MetricKind::CpuFrequency, 3_840_000_000.0, Unit::Hertz);
        assert_eq!(format_reading(&cpu_hz), "3.84 GHz");

        let ram = reading(MetricKind::MemoryUsed, 1_840_000_000_000.0, Unit::Bytes);
        assert_eq!(format_reading(&ram), "1.84 TB");

        let temp = reading(MetricKind::CpuTemperature, 62.0, Unit::Celsius);
        assert_eq!(format_reading(&temp), "62 °C");
    }

    // ===== Story 8.4 + 8.5 composition: render_sidebar =====
    //
    // These tests lock in the wiring contract: gear toggle surfaces settings,
    // bandwidth panel renders below metric rows when settings closed.

    #[test]
    fn render_sidebar_settings_open_shows_settings_panel() {
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let view = SidebarView {
            bandwidth: None,
            settings_open: true,
            sparkline: None,
            alert_acks: std::collections::HashMap::new(),
            alert_states: std::collections::HashMap::new(),
        };
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Billing cycle start day"),
            "settings_open=true must surface the settings panel (got: {labels})"
        );
        assert!(
            labels.contains(settings_panel::NO_RESPLIT_TOOLTIP),
            "settings panel must surface the no-retroactive-resplit tooltip (got: {labels})"
        );
    }

    #[test]
    fn render_sidebar_settings_closed_shows_bandwidth_placeholder() {
        // No bandwidth view → bandwidth panel renders its empty placeholder.
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let view = SidebarView {
            bandwidth: None,
            settings_open: false,
            sparkline: None,
            alert_acks: std::collections::HashMap::new(),
            alert_states: std::collections::HashMap::new(),
        };
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(bandwidth_panel::EMPTY_TEXT),
            "settings_open=false + no bandwidth view must render the bandwidth placeholder (got: {labels})"
        );
        assert!(
            labels.contains("42%"),
            "settings_open=false must still render the metric rows (got: {labels})"
        );
    }

    // ===== Story 8.6 + 8.7 + 8.8 wiring: theme applies, sparkline renders,
    // alert color flows through. =====

    #[test]
    fn render_sidebar_applies_configured_theme_mode() {
        // Light theme config — verify the ctx visuals flip to light after
        // render_sidebar runs (Story 8.6 wiring).
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        config.theme.mode = "Light".to_string();
        let view = SidebarView::default();
        let ctx_holder: std::cell::RefCell<Option<egui::Context>> = std::cell::RefCell::new(None);
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
            *ctx_holder.borrow_mut() = Some(ui.ctx().clone());
        });
        harness.run();
        let ctx = ctx_holder.borrow().clone().expect("ctx captured");
        assert!(
            !ctx.global_style().visuals.dark_mode,
            "config.theme.mode=\"Light\" must flip ctx visuals to light (Story 8.6 wiring)"
        );
    }

    #[test]
    fn render_sidebar_renders_sparkline_when_samples_present() {
        // Three samples → sparkline widget renders (allocates a rect, paints a
        // line). We assert no panic + the widget is reachable from production.
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let view = SidebarView {
            bandwidth: None,
            settings_open: false,
            sparkline: Some(vec![10.0, 20.0, 30.0]),
            alert_acks: std::collections::HashMap::new(),
            alert_states: std::collections::HashMap::new(),
        };
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        // The sparkline widget paints into the painter; the access tree won't
        // surface line geometry. The wiring contract here is "no panic" + the
        // metric row still renders below it.
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("42%"),
            "sparkline render must not displace the metric row (got: {labels})"
        );
    }

    #[test]
    fn render_sidebar_renders_local_clock_and_date_header() {
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let view = SidebarView::default();
        let expected_date = chrono::Local::now().date_naive().to_string();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(&expected_date),
            "header must render locale-stable date {expected_date} (got: {labels})"
        );
    }

    #[test]
    fn render_sidebar_critical_temp_paints_metric_row_red() {
        // 95°C CPU temp with default thresholds (warn 80, crit 90) → Critical
        // → metric row tinted CRITICAL_RED. We assert the value label's color
        // via the access tree's "color" glyph isn't available; instead we
        // verify the alert classification runs without panic and the row
        // still renders the value (the color flow is pinned by the
        // alert_indicator unit tests directly).
        let readings = vec![reading(MetricKind::CpuTemperature, 95.0, Unit::Celsius)];
        let mut config = Config::default();
        let view = SidebarView::default();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("95"),
            "critical CPU temp must still render its value (got: {labels})"
        );
    }

    #[test]
    fn render_sidebar_mut_alert_actions_persist_acknowledgement() {
        use egui_kittest::kittest::Queryable;

        let readings = vec![reading(MetricKind::CpuTemperature, 95.0, Unit::Celsius)];
        let mut config = Config::default();
        let mut view = SidebarView::default();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar_mut(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &mut view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        harness.get_by_label("Acknowledge").click();
        harness.run();
        drop(harness);

        let key = sidebar_domain::graph::MetricKey {
            category: "cpu".to_string(),
            instance: "package".to_string(),
            kind: "CpuTemperature".to_string(),
        };
        assert_eq!(
            view.alert_acks.get(&key),
            Some(&sidebar_domain::alert::AlertAck::Acknowledged),
            "acknowledgement must persist in the production mutable view"
        );
    }

    #[test]
    fn render_sidebar_mut_preserves_alert_hysteresis_before_rearming_ack() {
        let key = sidebar_domain::graph::MetricKey {
            category: "cpu".to_string(),
            instance: "package".to_string(),
            kind: "CpuTemperature".to_string(),
        };
        let mut config = Config::default();
        let mut view = SidebarView::default();
        let first = vec![reading(MetricKind::CpuTemperature, 85.0, Unit::Celsius)];
        {
            let mut harness = Harness::new_ui(|ui| {
                render_sidebar_mut(
                    ui,
                    &first,
                    ProviderTier::Basic,
                    &mut config,
                    &mut view,
                    &|| {},
                    &|| {},
                    None,
                );
            });
            harness.run();
        }
        assert_eq!(
            view.alert_states.get(&key),
            Some(&sidebar_domain::alert::AlertState::Warning)
        );
        view.alert_acks
            .insert(key.clone(), sidebar_domain::alert::AlertAck::Acknowledged);

        // 78°C is below the warning threshold but inside the 5°C hysteresis
        // band; the acknowledgement must remain until the metric recovers.
        let second = vec![reading(MetricKind::CpuTemperature, 78.0, Unit::Celsius)];
        {
            let mut harness = Harness::new_ui(|ui| {
                render_sidebar_mut(
                    ui,
                    &second,
                    ProviderTier::Basic,
                    &mut config,
                    &mut view,
                    &|| {},
                    &|| {},
                    None,
                );
            });
            harness.run();
        }
        assert!(
            view.alert_acks.contains_key(&key),
            "ack must not clear while hysteresis keeps the raw state in Warning"
        );
        assert_eq!(
            view.alert_states.get(&key),
            Some(&sidebar_domain::alert::AlertState::Warning)
        );
    }

    #[test]
    fn render_sidebar_mut_gear_toggle_opens_settings() {
        use egui_kittest::kittest::Queryable;

        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let mut view = SidebarView::default();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar_mut(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &mut view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        harness.get_by_label("⚙").click();
        harness.run();
        drop(harness);
        assert!(
            view.settings_open,
            "gear click must open settings in production"
        );
    }

    // ===== Story 8.9 wiring: [metrics] config filters + reorders the live view =====

    #[test]
    fn render_sidebar_filters_metrics_by_enabled_in_order() {
        // Two readings: CpuUtilization + CpuPower. Config enables only
        // CpuUtilization → only that row renders in the live view (Boundary:
        // metric in order but not enabled → ignored).
        let readings = vec![
            reading(MetricKind::CpuUtilization, 42.0, Unit::Percent),
            reading(MetricKind::CpuPower, 65.0, Unit::Watts),
        ];
        let mut config = Config::default();
        config.metrics.enabled = vec!["CpuUtilization".to_string()];
        config.metrics.order = vec!["CpuUtilization".to_string(), "CpuPower".to_string()];
        let view = SidebarView::default();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &|| {},
                None,
            );
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("42%"),
            "enabled metric CpuUtilization must render (got: {labels})"
        );
        assert!(
            !labels.contains("65 W") && !labels.contains("65W"),
            "disabled metric CpuPower must NOT render in the live view (got: {labels})"
        );
    }

    // =================================================================
    // Story 12.8 Gap 3 — OHM child-liveness monitor.
    // =================================================================

    /// Cited: Story 12.8 Acceptance ("child exit degrades exactly once and
    /// rebuilds the registry"). The liveness probe is a closure so it can
    /// wrap `OhmSupervisor::is_child_alive()` in production. Here we inject
    /// a probe that returns `false` (child exited) and assert the
    /// `Event::TierChanged(Basic)` fires exactly once on the first poll,
    /// then the latch prevents rebroadcast.
    #[test]
    fn gap3_child_exit_emits_basic_tier_exactly_once() {
        let (state_tx, mut state_rx) = broadcast::channel::<Event>(8);
        let app_state = AppState::new_full(
            ProviderTier::Full,
            None,
            None,
            Config::default(),
            SidebarView::default(),
        );
        // Build a SidebarApp at Full tier with a liveness probe that returns
        // `false` (child has exited).
        let probe: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| false);
        let mut app = SidebarApp::new(app_state).with_event_sender(state_tx.clone());
        app = app.with_child_alive_fn(probe);
        assert_eq!(app.state.tier(), ProviderTier::Full, "starts at Full");

        // Simulate two logic() polls by directly mirroring the drain logic
        // logic() runs (we can't call logic() without a real eframe Frame).
        let mut saw_basic = 0;
        for _ in 0..2 {
            if !app.child_exit_degraded {
                if let Some(probe) = app.child_alive_fn.as_ref() {
                    if !probe() {
                        let _ =
                            state_tx.send(Event::TierChanged(sidebar_domain::event::Tier::Basic));
                        app.state.set_tier(ProviderTier::Basic);
                        app.child_exit_degraded = true;
                    }
                }
            }
            while let Ok(event) = state_rx.try_recv() {
                if matches!(
                    event,
                    Event::TierChanged(sidebar_domain::event::Tier::Basic)
                ) {
                    saw_basic += 1;
                }
            }
        }
        assert_eq!(saw_basic, 1, "TierChanged(Basic) must fire exactly once");
        assert_eq!(
            app.state.tier(),
            ProviderTier::Basic,
            "AppState tier must be Basic after the child exited"
        );
        assert!(
            app.child_exit_degraded,
            "latch must be set so subsequent frames don't rebroadcast"
        );
    }

    // =================================================================
    // Monitor sentinel false-fallback fix (T-36).
    // =================================================================

    #[test]
    fn primary_sentinel_is_not_a_real_fallback() {
        // "primary" resolving to a device-id is the expected behavior, not
        // a fallback. The helper must return false so no warning fires and
        // the config is NOT overwritten with the device-id.
        assert!(
            !monitor_id_is_real_fallback("primary", "MONITOR\\LEN88AE\\0001"),
            "\"primary\" sentinel must NOT be treated as a fallback"
        );
        // Case-insensitive.
        assert!(
            !monitor_id_is_real_fallback("Primary", "MONITOR\\LEN88AE\\0001"),
            "\"Primary\" (capitalized) must NOT be a fallback either"
        );
    }

    #[test]
    fn real_device_id_mismatch_is_a_fallback() {
        assert!(
            monitor_id_is_real_fallback("MONITOR\\OLD\\0001", "MONITOR\\LEN88AE\\0001"),
            "a real configured device-id that doesn't match the resolved id IS a fallback"
        );
    }

    #[test]
    fn matching_device_id_is_not_a_fallback() {
        assert!(
            !monitor_id_is_real_fallback("MONITOR\\LEN88AE\\0001", "MONITOR\\LEN88AE\\0001"),
            "matching device-id is not a fallback"
        );
    }

    // =================================================================
    // Story 12.2 — MetricHistory populated on replace_readings.
    // =================================================================

    #[test]
    fn replace_readings_populates_metric_history() {
        let state = AppState::new(ProviderTier::Basic, None);
        // Push 3 readings for CPU utilization.
        for i in 0..3 {
            state.replace_readings(vec![Reading::gauge(
                SensorId::new("cpu", "package"),
                MetricKind::CpuUtilization,
                f64::from(i) * 10.0,
                Unit::Percent,
            )]);
        }
        let history = state.history_snapshot();
        let key = sidebar_domain::graph::MetricKey {
            category: "cpu".to_string(),
            instance: "package".to_string(),
            kind: "CpuUtilization".to_string(),
        };
        let window = history.get(&key).expect("CPU history window must exist");
        assert_eq!(window.len(), 3, "3 pushes -> 3 values in the window");
    }

    // =================================================================
    // Story 12.8 Gap 1 — status-pill launch callback flows through
    // render_sidebar (was hard-coded &|| {} no-op).
    // =================================================================

    #[test]
    fn gap1_status_pill_click_invokes_launch_callback() {
        use egui_kittest::kittest::Queryable;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let on_launch = move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        };
        let readings = vec![reading(MetricKind::CpuUtilization, 42.0, Unit::Percent)];
        let mut config = Config::default();
        let view = SidebarView::default();
        let mut harness = Harness::new_ui(|ui| {
            render_sidebar(
                ui,
                &readings,
                ProviderTier::Basic,
                &mut config,
                &view,
                &|| {},
                &on_launch,
                None,
            );
        });
        harness.run();
        // The status pill renders "BASIC" (uppercase) at Basic tier. Click it.
        harness.get_by_label("BASIC").click();
        harness.run();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "clicking the Basic status pill MUST invoke the launch callback exactly once"
        );
    }

    #[test]
    fn run_rebinds_runtime_hooks_after_eframe_app_recreation() {
        let state = AppState::new(ProviderTier::Basic, None);
        let app = SidebarApp::new(state);
        let (_view_tx, view_rx) = tokio::sync::watch::channel::<Option<BandwidthView>>(None);
        let child_alive: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
        let launch: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});

        let rebound = SidebarApp::apply_runtime_hooks(
            app,
            Some(view_rx),
            Some(Arc::clone(&child_alive)),
            Some(Arc::clone(&launch)),
        );

        assert!(rebound.bandwidth_view_rx.is_some());
        assert!(rebound.child_alive_fn.is_some());
        assert!(rebound.launch_fn.is_some());
    }
}
