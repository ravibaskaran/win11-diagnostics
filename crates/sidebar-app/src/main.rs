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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
// `TOTAL_SHUTDOWN_BUDGET` is only consumed by the production watchdog thread
// (gated `cfg(not(test))`); import it only in production builds to avoid an
// unused-import warning in test builds.
#[cfg(not(test))]
use sidebar_app::shutdown::TOTAL_SHUTDOWN_BUDGET;
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

#[allow(clippy::too_many_lines)]
fn main() -> eframe::Result {
    init_tracing();
    if std::env::args().any(|arg| arg == "--bench-cold-start") {
        run_cold_start_bench();
        return Ok(());
    }
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
    let mut event_channel = EventChannel::new();
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

    let mut signal_join = spawn_signal_handler_with_signal(shutdown_signal.clone());

    let state = AppState::new_full(
        tier,
        Some(readings_rx_for_gui),
        Some(event_rx_for_gui),
        config,
        SidebarView::default(),
    );
    state.set_shutdown_signal(shutdown_signal.clone());
    let app = SidebarApp::with_config_path(state, config_path, wizard_active)
        .with_event_sender(event_channel.raw_tx.clone());
    // Story 12.8 Gap 2 — wire the accountant's BandwidthView receiver.
    let app = if let Some(rx) = background_tasks.bandwidth_view_rx.take() {
        app.with_bandwidth_view_rx(rx)
    } else {
        app
    };

    tracing::info!("sidebar binary launching — entering eframe GUI loop");
    let eframe_result = app.run("sidebar");

    run_graceful_shutdown(
        &runtime,
        &shutdown_signal,
        accountant_flush_flag,
        supervisor.as_mut(),
        &mut background_tasks,
        &mut event_channel.coalescer,
        &mut signal_join,
    );

    eframe_result.map(|()| {
        std::process::exit(0);
    })
}

/// Run the minimal egui frame path used by Story 10.1's cold-start, RSS, and
/// egress checks. It intentionally bypasses configuration, sensor discovery,
/// graphics initialization, and the LHM probe so the acceptance harness
/// measures the host's Basic-mode startup path without requiring elevation or
/// hardware. The frame is still executed through egui's real frame API.
fn run_cold_start_bench() {
    let output_path = std::env::var_os("SIDEBAR_BENCH_COLD_START_FILE").map_or_else(
        || std::env::temp_dir().join("sidebar-cold-start.txt"),
        PathBuf::from,
    );
    let hold_ms = std::env::var("SIDEBAR_BENCH_HOLD_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let started = Instant::now();
    let start_unix_ms = unix_time_ms();
    let _ = std::fs::write(&output_path, format!("start_unix_ms={start_unix_ms}\n"));
    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| ui.label("cold-start probe"));
    });
    let first_frame_unix_ms = unix_time_ms();
    let elapsed_ms = started.elapsed().as_millis();
    let _ = std::fs::write(
        &output_path,
        format!(
            "start_unix_ms={start_unix_ms}\nfirst_frame_unix_ms={first_frame_unix_ms}\nelapsed_ms={elapsed_ms}\n"
        ),
    );
    if hold_ms > 0 {
        std::thread::sleep(Duration::from_millis(hold_ms));
    }
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
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
    /// Story 12.8 Gap 2 — watch receiver for live BandwidthView from the
    /// accountant thread. `None` when the wizard gate skipped the accountant.
    bandwidth_view_rx:
        Option<tokio::sync::watch::Receiver<Option<sidebar_bandwidth::view::BandwidthView>>>,
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
            bandwidth_view_rx: None,
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
    let (accountant, bandwidth_view_rx) = spawn_accountant(
        readings_rx_for_accountant,
        cancel.clone(),
        config_dir,
        cycle_start_day,
        Arc::clone(accountant_flush_flag),
    );
    BackgroundTaskHandles {
        poller: Some(poller),
        accountant: Some(accountant),
        bandwidth_view_rx,
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
) -> (
    std::thread::JoinHandle<()>,
    Option<tokio::sync::watch::Receiver<Option<sidebar_bandwidth::view::BandwidthView>>>,
) {
    let db_path = config_dir.join("bandwidth.db");
    let cycle_day = sidebar_domain::billing::CycleStartDay::from(cycle_start_day);
    // Story 12.8 Gap 2 — create the watch pair here so the receiver can be
    // returned to the GUI thread. The sender moves into the accountant.
    let (view_tx, view_rx) =
        tokio::sync::watch::channel::<Option<sidebar_bandwidth::view::BandwidthView>>(None);
    let accountant_handle = std::thread::Builder::new()
        .name("sidebar-accountant".to_string())
        .spawn(move || {
            run_accountant_on_thread(
                readings_rx,
                cancel,
                &db_path,
                cycle_day,
                flush_flag,
                view_tx,
            );
        })
        .expect("failed to spawn accountant thread");
    (accountant_handle, Some(view_rx))
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
    view_tx: tokio::sync::watch::Sender<Option<sidebar_bandwidth::view::BandwidthView>>,
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
    )
    .with_view_sender(view_tx);
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
///
/// T-19/T-39 H3 — total shutdown MUST complete within `TOTAL_SHUTDOWN_BUDGET`
/// (3000ms). The orchestrator phases run cumulatively (≤2000ms after M6),
/// then the post-orchestrator joins are bounded so the sum stays inside
/// 3000ms: poller ≤500ms, accountant ≤250ms (bounded via wait-thread +
/// oneshot), coalescer ≤150ms, signal-handler ≤100ms. A top-level
/// watchdog thread enforces the hard ceiling: if any join overruns, the
/// watchdog calls `std::process::exit(0)` at t=3000ms so the elevated
/// LHM child (G10 Job Object) is reaped by the kernel.
fn run_graceful_shutdown(
    runtime: &tokio::runtime::Runtime,
    signal: &ShutdownSignal,
    accountant_flush_flag: Arc<AtomicBool>,
    supervisor: Option<&mut OhmSupervisor<RealHttpClient>>,
    background_tasks: &mut BackgroundTaskHandles,
    coalescer: &mut tokio::task::JoinHandle<()>,
    signal_join: &mut tokio::task::JoinHandle<()>,
) {
    tracing::info!("eframe returned — running shutdown orchestrator");

    // T-19/T-39 H3 — top-level watchdog. The shutdown sequence below is
    // bounded, but a buggy/edge-case overrun could still exceed 3000ms.
    // The watchdog is the hard backstop: at t=TOTAL_SHUTDOWN_BUDGET it
    // calls process::exit(0) so the kernel reaps the elevated LHM child
    // (G10 Job Object) and no host state is left dangling. The watchdog
    // is detached and observes a completion flag so a successful shutdown
    // never triggers a delayed process exit.
    //
    // Disabled under `cfg(test)` so unit tests in main.rs (and any test
    // that could call into the shutdown path) don't have the host process
    // killed by the watchdog. Production builds always spawn it.
    #[cfg(not(test))]
    let watchdog_done = spawn_shutdown_watchdog(TOTAL_SHUTDOWN_BUDGET);

    let mut targets = SidebarShutdownTargets {
        accountant_flush_done: accountant_flush_flag,
        // T-39 phase 2 budget is 500ms (PHASE_FLUSH_BUDGET). The inner
        // deadline MUST sit under that so the orchestrator's outer timeout
        // is the authoritative bound (not this inner one). 450ms leaves
        // 50ms margin for the spin-poll sleep + final flag observation.
        accountant_thread_deadline: Duration::from_millis(450),
        supervisor,
    };
    let shutdown_guard = Arc::new(AtomicBool::new(false));
    let report: ShutdownReport = runtime
        .block_on(async { run_shutdown_with_signal(signal, &mut targets, &shutdown_guard).await });
    tracing::info!(?report, "shutdown orchestrator complete");
    if let Some(mut poller) = background_tasks.poller.take() {
        // T-19 post-orchestrator join budget: 500ms (down from 1000ms).
        // The poller respects CancellationToken; it should join near-
        // instantly once cancel fires. 500ms is generous headroom.
        let result = runtime.block_on(join_poller_with_timeout(
            &mut poller,
            Duration::from_millis(500),
        ));
        if let Err(error) = result {
            tracing::warn!(?error, "poller task did not join cleanly during shutdown");
        }
    }
    // T-19/T-39 H3 — accountant join is bounded via the wait-thread helper
    // (was an unbounded `accountant.join()`). 250ms leaves the sum
    // (orchestrator 2000 + poller 500 + accountant 250 + coalescer 150 +
    // signal 100 = 3000ms) inside T-19. A SQLite/antivirus stall that
    // exceeds 250ms leaks the accountant thread — acceptable per T-39
    // because the watchdog (above) will force-exit at t=3000ms.
    let accountant = background_tasks.accountant.take();
    let _ = runtime.block_on(join_thread_with_timeout(
        accountant,
        Duration::from_millis(250),
        "accountant",
    ));
    for (name, handle) in [
        ("event coalescer", coalescer),
        ("signal handler", signal_join),
    ] {
        // T-19 — coalescer/signal joins tightened to 150ms / 100ms (down
        // from 1000ms each). Both tasks are async + cancel-aware; they
        // should join in microseconds.
        let budget = if name == "event coalescer" {
            Duration::from_millis(150)
        } else {
            Duration::from_millis(100)
        };
        let result = runtime.block_on(join_poller_with_timeout(handle, budget));
        if let Err(error) = result {
            tracing::warn!(?error, task = name, "shutdown task did not join cleanly");
        }
    }
    #[cfg(not(test))]
    watchdog_done.store(true, Ordering::SeqCst);
}

/// Return whether the watchdog should force termination after its deadline.
/// Kept pure so the completion decision is testable without ever calling
/// `process::exit` from a test binary.
fn watchdog_should_force_exit(shutdown_completed: bool) -> bool {
    !shutdown_completed
}

#[cfg(not(test))]
fn spawn_shutdown_watchdog(budget: Duration) -> Arc<AtomicBool> {
    let completed = Arc::new(AtomicBool::new(false));
    let completed_for_thread = Arc::clone(&completed);
    std::thread::spawn(move || {
        std::thread::sleep(budget);
        if watchdog_should_force_exit(completed_for_thread.load(Ordering::SeqCst)) {
            #[allow(clippy::cast_possible_truncation)]
            let budget_ms = budget.as_millis() as u64;
            tracing::error!(
                budget_ms,
                "T-19 watchdog: shutdown exceeded total budget — forcing process exit"
            );
            std::process::exit(0);
        }
    });
    completed
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

/// T-19/T-39 H3 — bounded join for the accountant's OS thread. The
/// accountant runs on a dedicated `std::thread` (it is `!Send` because of
/// the owned rusqlite `Connection`), so `JoinHandle::join()` is blocking
/// and unbounded. A SQLite/antivirus stall could hang shutdown
/// indefinitely, leaving the elevated LHM child (G10 Job Object) alive
/// until the OS finally kills the host.
///
/// We bound the join via a wait-thread + oneshot pattern: spawn a thread
/// that calls `join()`, send the result on a oneshot, and the async side
/// races it against `timeout_duration`. On timeout we log + leak the
/// thread (acceptable per T-39 — the host process is about to exit via
/// the watchdog).
///
/// Returns `Ok(())` if joined cleanly within budget; `Err(())` on timeout.
async fn join_thread_with_timeout(
    handle: Option<std::thread::JoinHandle<()>>,
    timeout_duration: Duration,
    name: &'static str,
) -> Result<(), ()> {
    let Some(handle) = handle else {
        return Ok(());
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = handle.join();
        // Sender errors (receiver dropped on timeout) are harmless — the
        // wait-thread is detached and the JoinHandle is consumed either way.
        let _ = tx.send(result);
    });
    match tokio::time::timeout(timeout_duration, rx).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Err(_)) => {
            tracing::error!(task = name, "accountant wait-thread dropped its sender");
            Err(())
        }
        Ok(Ok(Err(_panic_payload))) => {
            tracing::error!(task = name, "accountant thread panicked during shutdown");
            Err(())
        }
        Err(_elapsed) => {
            #[allow(clippy::cast_possible_truncation)]
            let timeout_ms = timeout_duration.as_millis() as u64;
            tracing::warn!(
                task = name,
                timeout_ms,
                "T-19: accountant thread did not join within budget — leaking (host exit will reap)"
            );
            Err(())
        }
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
        // its flush-done flag bounded by the inner deadline (450ms — under
        // the orchestrator's outer 500ms phase-2 budget with 50ms margin).
        // The outer orchestrator timeout (PHASE_FLUSH_BUDGET) is the
        // authoritative bound; this inner deadline is a defensive secondary
        // cap so the spin-poll returns deterministically even if the
        // accountant sets its flag late in the window.
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
        match sv.shutdown_with_budget(Duration::from_millis(1_400)) {
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
    use super::{join_poller_with_timeout, join_thread_with_timeout, watchdog_should_force_exit};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn watchdog_decision_only_forces_exit_when_shutdown_is_incomplete() {
        assert!(watchdog_should_force_exit(false));
        assert!(!watchdog_should_force_exit(true));
    }

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

    // ----- H3: bounded accountant (OS thread) join -----

    /// Cited: T-19/T-39 H3. A blocking OS thread (simulated SQLite stall)
    /// MUST be joined within the budget; the helper returns `Err(())` and
    /// leaks the thread rather than blocking shutdown indefinitely. Without
    /// this bound, `accountant.join()` hangs the host and the elevated LHM
    /// child (G10 Job Object) stays alive until the OS kills the process.
    #[tokio::test(flavor = "current_thread")]
    async fn join_thread_with_timeout_returns_err_when_thread_blocks() {
        // Spawn a thread that blocks indefinitely (simulated stall).
        let handle = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(30));
        });
        let start = std::time::Instant::now();
        let result =
            join_thread_with_timeout(Some(handle), Duration::from_millis(100), "stall").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "blocking thread must time out, not join");
        assert!(
            elapsed < Duration::from_millis(500),
            "T-19: bounded join must return near the budget (got {elapsed:?})"
        );
        // The thread is leaked — but the test process exits at the end so
        // the OS reaps it. (No assertion needed; we leak intentionally.)
    }

    /// Cited: T-19 H3. A thread that completes quickly MUST join cleanly
    /// within the budget; the helper returns `Ok(())`.
    #[tokio::test(flavor = "current_thread")]
    async fn join_thread_with_timeout_returns_ok_when_thread_completes() {
        let handle = std::thread::spawn(|| {});
        let result =
            join_thread_with_timeout(Some(handle), Duration::from_millis(500), "fast").await;
        assert!(result.is_ok(), "fast thread must join cleanly");
    }

    /// Cited: T-19 H3. `None` (no thread to join) is the wizard-gate path;
    /// helper returns Ok(()) without spawning a wait-thread.
    #[tokio::test(flavor = "current_thread")]
    async fn join_thread_with_timeout_handles_none() {
        let result = join_thread_with_timeout(None, Duration::from_millis(100), "none").await;
        assert!(result.is_ok(), "None handle must short-circuit Ok(())");
    }
}
