//! `sidebar-app` binary entry point — full integration wiring.
//!
//! Launch sequence: load config → tier probe → provider registry → poller
//! spawn → bandwidth accountant spawn → event channel → AppState → eframe →
//! graceful shutdown. The GUI thread (eframe) blocks on the main thread;
//! background tasks (poller, accountant, coalescer, signal handler) run on a
//! tokio multi-thread runtime + a dedicated OS thread for the accountant's
//! `!Send` `LocalSet`.
//!
//! ## Thread model
//!
//! - **Main thread (eframe)**: `eframe::run_native` blocks on the OS message
//!   loop. The GUI thread does NOT own the tokio runtime — it just renders +
//!   drains the AppState's broadcast receivers each frame.
//! - **Tokio runtime (2 workers per T-18)**: owns the poller task, the event
//!   coalescer, the Ctrl+C signal handler.
//! - **Accountant OS thread + LocalSet**: the BandwidthAccountant's `run()` is
//!   `!Send` (rusqlite Connection is `!Sync`). A dedicated OS thread hosts a
//!   `current_thread` tokio runtime + a `LocalSet` so the accountant can run.
//!   The CancellationToken coordinates shutdown across all threads.
//!
//! ## First-run wizard gate (Story 8.10, G24)
//!
//! If `config.first_run_complete != true`, the SidebarApp renders the wizard
//! instead of the live sidebar, and the poller + accountant are NOT spawned.
//! When the wizard completes, it writes `first_run_complete = true`; the user
//! restarts sidebar and the poller spawns.
//!
//! ## Don't crash on missing resources (G15)
//!
//! - Tier probe fails (LHM not installed) → Basic tier (normal first launch).
//! - SQLite open fails → log + skip the accountant (app still works).
//! - `%APPDATA%` missing → fall back to `./sidebar_config` next to the binary.
//! - DWM capture exclusion: configured from eframe's CreationContext root
//!   HWND. If the platform handle is unavailable, the app logs and continues;
//!   capture exclusion is a non-fatal visual refinement.
//!
//! Cited: architecture.md §1/§6, nfr-thresholds.md T-3/T-14/T-18/T-19/T-39,
//! guardrails.md G15/G24, Stories 7.1-7.5 + 8.1/8.5/8.10.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sidebar_adapter_ohm::http::RealHttpClient;
use sidebar_app::event_channel::EventChannel;
use sidebar_app::gui::first_run;
use sidebar_app::gui::{AppState, SidebarApp, SidebarView};
use sidebar_app::poller::Poller;
use sidebar_app::provider_registry::build_registry;
use sidebar_app::shutdown::{
    run_shutdown_with_signal, spawn_signal_handler_with_signal, ShutdownReport, ShutdownSignal,
    ShutdownTargets,
};
use sidebar_app::tier_probe::run_launch_probe;
use sidebar_bandwidth::accountant::{AccountantConfig, BandwidthAccountant};
use sidebar_bandwidth::clock::SystemClock;
use sidebar_domain::config::Config;
use sidebar_domain::reading::Reading;
use sidebar_platform::ohm_supervisor::OhmSupervisor;
use sidebar_sensor::classifier::ActiveTier;
use sidebar_sensor::descriptor::ProviderTier;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Broadcast capacity for the readings channel (T-14: cap 8).
const READINGS_CHANNEL_CAPACITY: usize = 8;

/// Number of tokio worker threads (T-18: 2 workers).
const TOKIO_WORKERS: usize = 2;

fn main() -> eframe::Result {
    init_tracing();
    let config_dir = resolve_config_dir();
    let config_path = config_dir.join("config.toml");
    let config = load_config(&config_path);
    tracing::info!(
        path = %config_path.display(),
        poll_interval_seconds = config.poll_interval_seconds,
        first_run_complete = config.first_run_complete,
        "config loaded"
    );

    let wizard_active = first_run::should_show(&config);
    let runtime = build_runtime();
    // Enter the runtime context so EventChannel::new() (which calls
    // tokio::spawn for the coalescer task) and signal-handler spawn work from
    // the synchronous main thread. Without `_guard`, tokio::spawn panics with
    // "there is no reactor running".
    let _runtime_guard = runtime.enter();
    let cancel = CancellationToken::new();
    let event_channel = EventChannel::new();
    let shutdown_signal = ShutdownSignal::new(cancel.clone(), event_channel.raw_tx.clone());
    let event_rx_for_gui = event_channel.subscribe();
    let event_rx_for_poller = event_channel.subscribe();

    // Tier probe: start at Basic + probe ASYNCHRONOUSLY. The probe does up to
    // 11 sequential HTTP GETs (ports 17127-17137) which can block for the OS
    // TCP timeout (~21s on Windows) per port if the firewall drops the SYN
    // rather than sending RST. Running it on the main thread would block
    // eframe launch for minutes. Instead: start at Basic, spawn the probe on
    // `spawn_blocking`, and if it resolves Full, fire a TierChanged event via
    // the EventChannel — the GUI's event drain flips the tier + status pill
    // without blocking startup. (Architecture AD-6: blocking work on
    // spawn_blocking; the GUI never blocks on I/O.)
    let tier = ProviderTier::Basic;
    let supervisor: Option<OhmSupervisor<RealHttpClient>> = if wizard_active {
        None
    } else {
        // Construct the supervisor synchronously (cheap — just opens the HTTP
        // agent). The tier-change callback is wired so a later Full resolution
        // reaches the GUI. The probe itself runs on spawn_blocking below.
        let client = RealHttpClient::new();
        let supervisor = OhmSupervisor::new(client, &config_dir);
        let raw_tx = event_channel.raw_tx.clone();
        let cb: sidebar_platform::ohm_supervisor::TierChangeCallback = Box::new(move |new_tier| {
            let mapped = if matches!(new_tier, ProviderTier::Full) {
                sidebar_domain::event::Tier::Full
            } else {
                sidebar_domain::event::Tier::Basic
            };
            let _ = raw_tx.send(sidebar_domain::event::Event::TierChanged(mapped));
        });
        let mut supervisor = supervisor;
        supervisor.set_tier_change_broadcaster(Some(cb));

        // Spawn the probe on a blocking thread. If it resolves Full, fire the
        // event so the GUI flips; if Basic, no event (we're already Basic).
        // The probe constructs its OWN throwaway supervisor (cheap — just a
        // ureq Agent) so the main-thread supervisor stays available for the
        // shutdown teardown.
        let probe_port = config.ohm.http_port;
        let probe_dir = config_dir.clone();
        let probe_tx = event_channel.raw_tx.clone();
        runtime.spawn_blocking(move || {
            let probe_client = RealHttpClient::new();
            let probe_supervisor = OhmSupervisor::new(probe_client, &probe_dir);
            let probe = run_launch_probe(&probe_supervisor, probe_port, None, None);
            if let Some(port) = probe.resolved_port {
                tracing::info!(resolved_port = port, "async tier probe resolved OHM port");
            }
            if let Some(hint) = &probe.hint {
                tracing::info!(hint, "async tier probe hint");
            }
            tracing::info!(tier = ?probe.tier, "async tier probe complete");
            if matches!(probe.tier, ProviderTier::Full) {
                let _ = probe_tx.send(sidebar_domain::event::Event::TierChanged(
                    sidebar_domain::event::Tier::Full,
                ));
            }
        });

        Some(supervisor)
    };
    let mut supervisor = supervisor;

    let readings_tx = broadcast::channel::<Vec<Reading>>(READINGS_CHANNEL_CAPACITY).0;
    let readings_rx_for_gui = readings_tx.subscribe();
    let readings_rx_for_accountant = readings_tx.subscribe();

    let accountant_flush_flag = Arc::new(AtomicBool::new(false));
    let mut background_tasks = spawn_background_tasks(
        &runtime,
        &cancel,
        wizard_active,
        tier,
        config.poll_interval_seconds,
        readings_tx,
        readings_rx_for_accountant,
        event_rx_for_poller,
        &config_dir,
        &config.bandwidth.cycle_start_day,
        &accountant_flush_flag,
    );

    let _signal_join = runtime.spawn(spawn_signal_handler_with_signal(shutdown_signal.clone()));

    let state = AppState::new_full(
        tier,
        Some(readings_rx_for_gui),
        Some(event_rx_for_gui),
        config,
        SidebarView::default(),
    );
    state.set_shutdown_signal(shutdown_signal.clone());
    let app = SidebarApp::with_config_path(state, config_path, wizard_active);

    tracing::info!("sidebar binary launching — entering eframe GUI loop");
    let eframe_result = app.run("sidebar");

    run_graceful_shutdown(
        &runtime,
        &shutdown_signal,
        accountant_flush_flag,
        supervisor.as_mut(),
        &mut background_tasks,
    );

    eframe_result.map(|()| {
        std::process::exit(0);
    })
}

/// Install the tracing subscriber (RUST_LOG env var, default to a readable
/// info/warn split). Idempotent — `try_init` ignores the error if a global
/// subscriber is already installed (e.g. by a test harness).
fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "sidebar_app=info,sidebar_bandwidth=warn,sidebar_persistence=warn,warn".to_string()
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(&filter)
        .try_init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        target = "sidebar.app.main",
        "sidebar binary launching (integration main wiring)"
    );
}

/// Build the tokio multi-thread runtime (2 workers per T-18).
fn build_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(TOKIO_WORKERS)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
}

/// Spawn the poller + bandwidth-accountant tasks (gated by the wizard flag).
/// The accountant runs on a dedicated OS thread with a LocalSet (it is
/// `!Send`).
struct BackgroundTaskHandles {
    poller: Option<tokio::task::JoinHandle<()>>,
    accountant: Option<std::thread::JoinHandle<()>>,
}

#[allow(clippy::too_many_arguments)]
fn spawn_background_tasks(
    runtime: &tokio::runtime::Runtime,
    cancel: &CancellationToken,
    wizard_active: bool,
    tier: ProviderTier,
    poll_interval_seconds: u32,
    readings_tx: broadcast::Sender<Vec<Reading>>,
    readings_rx_for_accountant: broadcast::Receiver<Vec<Reading>>,
    event_rx_for_poller: broadcast::Receiver<sidebar_domain::event::Event>,
    config_dir: &Path,
    cycle_start_day: &sidebar_domain::config::CycleStartDaySerde,
    accountant_flush_flag: &Arc<AtomicBool>,
) -> BackgroundTaskHandles {
    if wizard_active {
        // No poller, no accountant — wizard gate (G24). Mark flush done so the
        // shutdown orchestrator doesn't wait for an accountant that isn't there.
        accountant_flush_flag.store(true, Ordering::SeqCst);
        drop(readings_tx);
        return BackgroundTaskHandles {
            poller: None,
            accountant: None,
        };
    }
    let poller = spawn_poller(
        runtime,
        cancel,
        tier,
        poll_interval_seconds,
        readings_tx,
        event_rx_for_poller,
    );
    let accountant = spawn_accountant(
        readings_rx_for_accountant,
        cancel.clone(),
        config_dir,
        cycle_start_day,
        Arc::clone(accountant_flush_flag),
    );
    BackgroundTaskHandles {
        poller: Some(poller),
        accountant: Some(accountant),
    }
}

/// Spawn the poller task on the tokio runtime.
fn spawn_poller(
    runtime: &tokio::runtime::Runtime,
    cancel: &CancellationToken,
    tier: ProviderTier,
    poll_interval_seconds: u32,
    readings_tx: broadcast::Sender<Vec<Reading>>,
    event_rx: broadcast::Receiver<sidebar_domain::event::Event>,
) -> tokio::task::JoinHandle<()> {
    let active_tier = match tier {
        ProviderTier::Full => ActiveTier::Full,
        ProviderTier::Basic | ProviderTier::Both => ActiveTier::Basic,
    };
    let providers = build_registry(active_tier);
    tracing::info!(
        provider_count = providers.len(),
        active_tier = ?active_tier,
        "provider registry built"
    );
    let interval = Duration::from_secs(u64::from(poll_interval_seconds));
    let poller = Poller::new(providers, interval, readings_tx);
    let cancel_for_poller = cancel.clone();
    runtime.spawn(async move {
        let registry_builder = |tier: ProviderTier| {
            let active_tier = match tier {
                ProviderTier::Full => ActiveTier::Full,
                ProviderTier::Basic | ProviderTier::Both => ActiveTier::Basic,
            };
            Ok(build_registry(active_tier))
        };
        match poller
            .run_with_events(cancel_for_poller, event_rx, registry_builder)
            .await
        {
            Ok(()) => tracing::info!("poller task exited cleanly"),
            Err(e) => tracing::error!(error = ?e, "poller task exited with error"),
        }
    })
}

/// Spawn the bandwidth accountant on a dedicated OS thread + LocalSet (the
/// accountant is `!Send` because rusqlite Connection is `!Sync`).
fn spawn_accountant(
    readings_rx: broadcast::Receiver<Vec<Reading>>,
    cancel: CancellationToken,
    config_dir: &Path,
    cycle_start_day: &sidebar_domain::config::CycleStartDaySerde,
    flush_flag: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let db_path = config_dir.join("bandwidth.db");
    let cycle_day = sidebar_domain::billing::CycleStartDay::from(cycle_start_day);
    std::thread::Builder::new()
        .name("sidebar-accountant".to_string())
        .spawn(move || {
            run_accountant_on_thread(readings_rx, cancel, &db_path, cycle_day, flush_flag);
        })
        .expect("failed to spawn accountant thread")
}

/// Open the SQLite connection + run the accountant on a current-thread
/// runtime + LocalSet. If SQLite open or schema::init fails, log + mark the
/// flush flag done (G15 — bandwidth tracking is non-fatal).
fn run_accountant_on_thread(
    readings_rx: broadcast::Receiver<Vec<Reading>>,
    cancel: CancellationToken,
    db_path: &Path,
    cycle_day: sidebar_domain::billing::CycleStartDay,
    flush_flag: Arc<AtomicBool>,
) {
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                path = %db_path.display(),
                error = %e,
                "SQLite open failed — bandwidth accountant disabled (G15 non-fatal)"
            );
            flush_flag.store(true, Ordering::SeqCst);
            return;
        }
    };
    if let Err(e) = sidebar_persistence::schema::init(&conn) {
        tracing::warn!(
            error = %e,
            "schema::init failed — bandwidth accountant disabled (G15 non-fatal)"
        );
        flush_flag.store(true, Ordering::SeqCst);
        return;
    }
    let accountant_config = AccountantConfig::production(cycle_day);
    let accountant = BandwidthAccountant::new(
        readings_rx,
        conn,
        Box::new(SystemClock::new()),
        accountant_config,
    );
    let local_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build accountant runtime");
    let local = tokio::task::LocalSet::new();
    local.block_on(&local_rt, async move {
        match accountant.run(cancel).await {
            Ok(()) => tracing::info!("bandwidth accountant exited cleanly"),
            Err(e) => tracing::error!(error = ?e, "bandwidth accountant exited with error"),
        }
        flush_flag.store(true, Ordering::SeqCst);
    });
}

/// Run the graceful-shutdown orchestrator after eframe returns (window
/// closed). Force-flushes the accountant + tears down the OhmSupervisor per
/// the T-39 phase hierarchy.
fn run_graceful_shutdown(
    runtime: &tokio::runtime::Runtime,
    signal: &ShutdownSignal,
    accountant_flush_flag: Arc<AtomicBool>,
    supervisor: Option<&mut OhmSupervisor<RealHttpClient>>,
    background_tasks: &mut BackgroundTaskHandles,
) {
    tracing::info!("eframe returned — running shutdown orchestrator");
    let mut targets = SidebarShutdownTargets {
        accountant_flush_done: accountant_flush_flag,
        accountant_thread_deadline: Duration::from_millis(600),
        supervisor,
    };
    let shutdown_guard = Arc::new(AtomicBool::new(false));
    let report: ShutdownReport = runtime
        .block_on(async { run_shutdown_with_signal(signal, &mut targets, &shutdown_guard).await });
    tracing::info!(?report, "shutdown orchestrator complete");
    if let Some(mut poller) = background_tasks.poller.take() {
        let result = runtime.block_on(join_poller_with_timeout(
            &mut poller,
            Duration::from_secs(1),
        ));
        if let Err(error) = result {
            tracing::warn!(?error, "poller task did not join cleanly during shutdown");
        }
    }
    if let Some(accountant) = background_tasks.accountant.take() {
        let _ = accountant.join();
    }
}

/// Await a poller task without detaching it when the graceful timeout expires.
/// The timeout borrows the same handle that is then aborted and awaited, so no
/// task is left running after shutdown returns.
async fn join_poller_with_timeout(
    poller: &mut tokio::task::JoinHandle<()>,
    timeout_duration: Duration,
) -> Result<(), tokio::task::JoinError> {
    if let Ok(result) = tokio::time::timeout(timeout_duration, &mut *poller).await {
        result
    } else {
        poller.abort();
        poller.await
    }
}

/// ShutdownTargets implementation for the integration launch sequence.
struct SidebarShutdownTargets<'a> {
    accountant_flush_done: Arc<AtomicBool>,
    accountant_thread_deadline: Duration,
    supervisor: Option<&'a mut OhmSupervisor<RealHttpClient>>,
}

impl ShutdownTargets for SidebarShutdownTargets<'_> {
    async fn force_flush(&mut self) -> Result<(), String> {
        // The accountant auto-flushes on CancellationToken cancel; we spin-poll
        // its flush-done flag bounded by the deadline (600ms — under the
        // orchestrator's 500ms phase 2 budget with margin).
        let start = std::time::Instant::now();
        loop {
            if self.accountant_flush_done.load(Ordering::SeqCst) {
                return Ok(());
            }
            if start.elapsed() >= self.accountant_thread_deadline {
                return Err("accountant flush-done flag not set within deadline".to_string());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn teardown_ohm(&mut self) -> Result<(), String> {
        let Some(sv) = self.supervisor.as_mut() else {
            return Ok(());
        };
        if !sv.sidebar_launched() {
            // Sidebar didn't launch LHM (Basic tier or user-started LHM) —
            // nothing to tear down (G10 ownership check).
            return Ok(());
        }
        match sv.shutdown() {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("OhmSupervisor::shutdown failed: {e}")),
        }
    }
}

/// Resolve the config directory: `%APPDATA%\sidebar` on Windows, falling back
/// to `./sidebar_config` next to the binary if %APPDATA% is unset.
fn resolve_config_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let dir = if let Some(appdata) = base {
        appdata.join("sidebar")
    } else {
        tracing::warn!("%APPDATA% not set — falling back to ./sidebar_config (G15)");
        PathBuf::from("sidebar_config")
    };
    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                path = %dir.display(),
                error = %e,
                "failed to create config directory (G15 — will fall back to defaults)"
            );
        }
    }
    dir
}

/// Load the config from the given path, falling back to `Config::default()`
/// if absent/unreadable. G15: never crash on a malformed config.
fn load_config(path: &PathBuf) -> Config {
    let Ok(content) = std::fs::read_to_string(path) else {
        tracing::info!(
            path = %path.display(),
            "config file absent or unreadable — using defaults"
        );
        return Config::default();
    };
    match Config::from_toml_str(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "config file malformed — using defaults (G15)"
            );
            Config::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::join_poller_with_timeout;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn poller_join_timeout_aborts_and_awaits_same_handle() {
        let completed = Arc::new(AtomicBool::new(false));
        let completed_by_task = Arc::clone(&completed);
        let mut handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            completed_by_task.store(true, Ordering::SeqCst);
        });

        let result = join_poller_with_timeout(&mut handle, Duration::from_millis(1)).await;
        assert!(
            result
                .expect_err("timeout must abort the poller")
                .is_cancelled(),
            "aborted poller should return a cancelled JoinError"
        );
        assert!(
            handle.is_finished(),
            "the same handle must be awaited after abort"
        );
        assert!(!completed.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn poller_join_completes_before_timeout() {
        let mut handle = tokio::spawn(async {});
        let result = join_poller_with_timeout(&mut handle, Duration::from_secs(1)).await;
        assert!(result.is_ok());
        assert!(handle.is_finished());
    }
}
