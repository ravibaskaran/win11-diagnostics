//! Story 7.5 — Graceful Shutdown Orchestrator (T-39 phase hierarchy).
//!
//! Handles `Ctrl+C` / `SIGTERM` / `WM_CLOSE` with the T-39 timeout hierarchy:
//!   1. `t=0ms`: cancel token fires (instant).
//!   2. `t=0–500ms`: accountant force-flush (synchronous, G15 panic-safe).
//!   3. `t=500–2000ms`: OhmSupervisor teardown (G10 Job Object reap).
//!   4. `t=2000–3000ms`: runtime drop (tokio runtime teardown).
//!   5. `t=3000ms`: forced `std::process::exit(0)` if any phase exceeds budget.
//!
//! ## Injectable targets (F13 testability)
//!
//! The orchestrator does NOT hard-couple to `BandwidthAccountant` or
//! `OhmSupervisor`. Instead it takes a [`ShutdownTargets`] trait with two
//! async methods — `force_flush` and `teardown_ohm`. Production wires real
//! adapters that forward to the accountant's final-flush path and the
//! supervisor's `shutdown()`; F13 tests inject mocks that complete immediately
//! (or hang, to exercise the per-phase timeout). This keeps the orchestrator
//! unit-testable without real UAC / SQLite.
//!
//! ## Double-signal guard
//!
//! A second `Ctrl+C` while shutdown is in progress is a no-op: the orchestrator
//! gates entry via an `AtomicBool` (`shutdown_started`). The first signal runs
//! the phases; subsequent signals return immediately. This is the documented
//! Boundary #4 contract.
//!
//! ## Cited
//!
//! - Story 7.5 TDD contract (F13: force-flush within 500ms; total ≤3000ms)
//! - architecture.md §6 (shutdown flow), §1 (binary owns runtime lifecycle)
//! - nfr-thresholds.md T-19 (3000ms total grace), T-39 (phase hierarchy)
//! - guardrails.md G10 (Job Object orphan prevention), G14 (shutdown align),
//!   G15 (flush errors caught + logged, shutdown continues)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sidebar_domain::event::Event;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// T-39 phase 2 budget: accountant force-flush must complete within 500ms of
/// the shutdown signal. Cited: nfr-thresholds.md T-39.
pub const PHASE_FLUSH_BUDGET: Duration = Duration::from_millis(500);

/// T-39 phase 3 budget: OhmSupervisor teardown must complete within 2s
/// (cumulative from t=0). Cited: nfr-thresholds.md T-39.
pub const PHASE_OHM_BUDGET: Duration = Duration::from_secs(2);

/// T-19 / T-39 total shutdown grace. The orchestrator MUST finish all phases
/// within 3s; anything still running at this boundary is force-exited via
/// `std::process::exit(0)`. Cited: nfr-thresholds.md T-19, T-39.
pub const TOTAL_SHUTDOWN_BUDGET: Duration = Duration::from_secs(3);

/// Shared shutdown trigger used by GUI close, Ctrl+C, and the main-loop
/// fallback. The first request cancels workers and emits one Shutdown event;
/// repeated requests are harmless no-ops.
#[derive(Clone)]
pub struct ShutdownSignal {
    cancel: CancellationToken,
    events: broadcast::Sender<Event>,
    emitted: Arc<AtomicBool>,
}

impl ShutdownSignal {
    /// Create a shutdown signal bound to a cancellation token and event bus.
    #[must_use]
    pub fn new(cancel: CancellationToken, events: broadcast::Sender<Event>) -> Self {
        Self {
            cancel,
            events,
            emitted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Clone the cancellation token observed by worker tasks.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Request shutdown exactly once. Returns true for the caller that won
    /// the race and false for subsequent requests.
    pub fn request(&self) -> bool {
        if self.emitted.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.cancel.cancel();
        let _ = self.events.send(Event::Shutdown);
        true
    }
}

/// Outcome of a single T-39 phase. Captured in [`ShutdownReport`] so the
/// caller (and tests) can assert which phases succeeded vs timed out vs
/// errored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseOutcome {
    /// The phase completed within its budget. For `force_flush`, this means
    /// the accountant's flush returned Ok (or a G15-logged error — the phase
    /// still "completed" because the accountant continued).
    Completed,
    /// The phase exceeded its T-39 budget. The orchestrator logged an error
    /// and advanced to the next phase (Boundary #1, #2). For phase 3 (OHM
    /// teardown), the Job Object (G10) reaps the child via kernel on process
    /// exit, so a timeout here is non-fatal.
    TimedOut,
    /// No target was wired for this phase (e.g. the app launched without an
    /// accountant, or OHM was never started). The phase is a no-op.
    NotConfigured,
}

/// Report returned by [`run_shutdown`]. Each field records the outcome of one
/// T-39 phase, so tests + the caller can inspect what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    /// Phase 2 — accountant force-flush (≤500ms per T-39).
    pub flush: PhaseOutcome,
    /// Phase 3 — OhmSupervisor teardown (≤2000ms per T-39).
    pub ohm: PhaseOutcome,
}

/// Injectable shutdown targets. Production wires closures that forward to the
/// real `BandwidthAccountant` final-flush path + `OhmSupervisor::shutdown()`;
/// F13 tests inject mocks that complete immediately (or hang, to exercise the
/// per-phase timeout).
///
/// The methods are `async` so the orchestrator can race them against
/// `tokio::time::timeout`. They return `Result<(), String>`: `Ok` means the
/// phase completed cleanly (or a G15-logged error was swallowed by the target
/// itself); `Err(msg)` means the target surfaced a non-recoverable failure.
/// Either way the orchestrator continues — the report records the outcome.
///
/// Making this a trait (rather than taking two `Box<dyn Future>` params) keeps
/// the call site clean and lets a single mock object record call counts across
/// both methods.
pub trait ShutdownTargets: Send {
    /// Phase 2 — force-flush the bandwidth accountant to SQLite. MUST complete
    /// within [`PHASE_FLUSH_BUDGET`] (500ms). G15: the implementor catches
    /// SQLite errors internally and returns `Ok` (the accountant continues);
    /// the orchestrator treats `Ok` as "phase done". An `Err` is reserved for
    /// "the accountant task itself is gone" (non-recoverable).
    ///
    /// # Errors
    /// Returns `Err(msg)` if the flush target is unrecoverable (e.g. the
    /// accountant task panicked or its handle is closed). The orchestrator
    /// logs + continues.
    fn force_flush(&mut self) -> impl std::future::Future<Output = Result<(), String>> + Send;

    /// Phase 3 — tear down the OhmSupervisor (G10 Job Object reap of the
    /// elevated LHM child). MUST complete within [`PHASE_OHM_BUDGET`] (2s).
    ///
    /// # Errors
    /// Returns `Err(msg)` if the supervisor's `shutdown()` fails. The
    /// orchestrator logs + continues — the Job Object still reaps the child
    /// on process exit (G10 safety net).
    fn teardown_ohm(&mut self) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

/// Execute the T-39 graceful-shutdown phase hierarchy.
///
/// Phase sequence (nfr-thresholds.md T-39):
///   1. `t=0ms`: the cancel token is ALREADY fired by the caller (the signal
///      handler / WM_CLOSE hook calls `cancel.cancel()` before invoking this
///      function). Nothing to do here — the poller/accountant tasks observing
///      the token begin unwinding concurrently.
///   2. `t=0–500ms`: `targets.force_flush()` — the accountant's final flush.
///      Race against [`PHASE_FLUSH_BUDGET`]; on timeout log `error!` + advance
///      (Boundary #1).
///   3. `t=500–2000ms`: `targets.teardown_ohm()` — OhmSupervisor shutdown.
///      Race against [`PHASE_OHM_BUDGET`]; on timeout log `error!` + return
///      `TimedOut` (the Job Object G10 reaps the elevated child on process
///      exit, so a timeout here is non-fatal).
///   4. `t=2000–3000ms`: runtime drop is the caller's responsibility (the
///      binary drops the tokio runtime after this function returns).
///   5. `t=3000ms`: forced exit is the caller's last-resort — if the runtime
///      drop itself hangs, the binary calls `std::process::exit(0)`.
///
/// G15: a flush that returns `Err` is logged and the phase is recorded as
/// `Completed` (the accountant continued; the error is the accountant's
/// concern). Shutdown never aborts on a target error.
///
/// Boundary #4: the `shutdown_started` flag gates re-entry. The second call
/// returns a `NotConfigured` report immediately (no-op).
#[allow(clippy::missing_panics_doc)]
pub async fn run_shutdown<T: ShutdownTargets>(
    cancel: CancellationToken,
    targets: &mut T,
    shutdown_started: &AtomicBool,
) -> ShutdownReport {
    cancel.cancel();
    run_shutdown_phases(targets, shutdown_started).await
}

/// Run the shutdown phases after emitting the idempotent cancellation/event
/// signal. Production callers should use this entry point so every trigger
/// reaches both worker cancellation and the Event channel.
pub async fn run_shutdown_with_signal<T: ShutdownTargets>(
    signal: &ShutdownSignal,
    targets: &mut T,
    shutdown_started: &AtomicBool,
) -> ShutdownReport {
    signal.request();
    run_shutdown_phases(targets, shutdown_started).await
}

async fn run_shutdown_phases<T: ShutdownTargets>(
    targets: &mut T,
    shutdown_started: &AtomicBool,
) -> ShutdownReport {
    // Boundary #4 — double-signal guard. The first caller flips the flag and
    // proceeds; a second caller (Ctrl+C twice) sees the flag already set and
    // returns immediately with a "no-op" report.
    if shutdown_started.swap(true, Ordering::SeqCst) {
        tracing::warn!("shutdown already in progress — second signal is a no-op");
        return ShutdownReport {
            flush: PhaseOutcome::NotConfigured,
            ohm: PhaseOutcome::NotConfigured,
        };
    }

    tracing::info!("shutdown: T-39 phase hierarchy starting");

    // ---- Phase 2: accountant force-flush (≤500ms per T-39) ----
    let flush_outcome = run_phase("force_flush", PHASE_FLUSH_BUDGET, targets.force_flush()).await;

    // ---- Phase 3: OhmSupervisor teardown (≤2000ms per T-39) ----
    // This runs unconditionally — even if flush timed out (Boundary #1) the
    // OHM child must still be torn down.
    let ohm_outcome = run_phase("teardown_ohm", PHASE_OHM_BUDGET, targets.teardown_ohm()).await;

    tracing::info!(
        flush = ?flush_outcome,
        ohm = ?ohm_outcome,
        "shutdown: T-39 phases complete"
    );

    ShutdownReport {
        flush: flush_outcome,
        ohm: ohm_outcome,
    }
}

/// Run one T-39 phase with a per-phase timeout. Returns:
/// - [`PhaseOutcome::Completed`] if the phase returned `Ok` OR `Err` within
///   budget (G15: an `Err` is logged but the phase still "ran to completion"
///   from the orchestrator's view — shutdown continues).
/// - [`PhaseOutcome::TimedOut`] if the phase exceeded its budget. The future
///   is dropped on timeout (cancelling it); for the accountant this means
///   the in-memory accumulator state may be partially flushed (data loss
///   accepted per R11). For OHM, the Job Object (G10) reaps the child on
///   process exit regardless.
async fn run_phase<F>(name: &'static str, budget: Duration, phase: F) -> PhaseOutcome
where
    F: std::future::Future<Output = Result<(), String>>,
{
    // `budget.as_millis()` is `u128`; the budgets here are ≤3000ms so the
    // cast to u64 is safe. The allow is scoped to this function.
    #[allow(clippy::cast_possible_truncation)]
    let budget_ms = budget.as_millis() as u64;
    match tokio::time::timeout(budget, phase).await {
        Ok(Ok(())) => {
            tracing::info!(phase = name, budget_ms, "phase completed");
            PhaseOutcome::Completed
        }
        Ok(Err(msg)) => {
            // G15 — the target surfaced a non-recoverable error (e.g. SQLite
            // disk full). Log + record as Completed (the phase ran; the error
            // is the target's concern). Shutdown continues.
            tracing::error!(phase = name, error = %msg, "phase errored (G15 — continuing)");
            PhaseOutcome::Completed
        }
        Err(_elapsed) => {
            // T-39 budget exceeded. Log + advance. The future is dropped
            // (cancelled) by tokio::time::timeout on Elapsed.
            tracing::error!(
                phase = name,
                budget_ms,
                "phase TIMED OUT (T-39) — advancing (G15/G10 safety net applies)"
            );
            PhaseOutcome::TimedOut
        }
    }
}

/// Spawn a tokio task that listens for `Ctrl+C` (`tokio::signal::ctrl_c()`)
/// and cancels the supplied [`CancellationToken`] on receipt. On Windows the
/// tokio `ctrl_c` handler hooks the console SIGBREAK handler; `WM_CLOSE` (the
/// eframe close button) is wired separately by the GUI layer calling
/// `cancel.cancel()` directly.
///
/// Returns the `JoinHandle` so the caller can detach (production) or await
/// (tests). The task runs until the token is cancelled OR a Ctrl+C arrives.
///
/// # Panics
/// The task logs (does not panic) if `ctrl_c()` returns an error (e.g. not a
/// console-attached process). The caller's shutdown still works via the other
/// trigger sources (WM_CLOSE, Event::Shutdown).
#[must_use]
pub fn spawn_signal_handler(cancel: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::select! {
            result = tokio::signal::ctrl_c() => match result {
                Ok(()) => {
                    tracing::info!("Ctrl+C received — cancelling shutdown token");
                    cancel.cancel();
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "ctrl_c() listener failed — shutdown still works via WM_CLOSE / Event::Shutdown"
                    );
                }
            },
            () = cancel.cancelled() => {
                tracing::debug!("shutdown signal handler observed cancellation — exiting");
            }
        }
    })
}

/// Spawn a Ctrl+C listener that requests the shared idempotent shutdown
/// signal, including Event::Shutdown propagation.
#[must_use]
pub fn spawn_signal_handler_with_signal(signal: ShutdownSignal) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cancel = signal.cancellation_token();
        tokio::select! {
            result = tokio::signal::ctrl_c() => match result {
                Ok(()) => {
                    tracing::info!("Ctrl+C received — requesting shutdown");
                    signal.request();
                }
                Err(e) => {
                    tracing::error!(error = %e, "ctrl_c() listener failed");
                }
            },
            () = cancel.cancelled() => {
                tracing::debug!("shutdown signal handler observed cancellation — exiting");
            }
        }
    })
}

/// Convenience wrapper that combines the double-signal guard + orchestrator.
/// Constructs a fresh `AtomicBool`, runs the shutdown, and returns the report.
/// Production callers that want to share the guard across trigger sources
/// (Ctrl+C task + WM_CLOSE handler) should call [`run_shutdown`] directly with
/// their own `Arc<AtomicBool>`.
///
/// # Errors
/// This function does not return an error — T-39/G15 mandate that shutdown
/// always continues. Per-phase failures are recorded in the [`ShutdownReport`].
#[allow(clippy::missing_panics_doc)]
pub async fn shutdown_once<T: ShutdownTargets>(
    cancel: CancellationToken,
    targets: &mut T,
) -> ShutdownReport {
    let guard = Arc::new(AtomicBool::new(false));
    run_shutdown(cancel, targets, &guard).await
}

#[cfg(test)]
mod tests {
    //! Story 7.5 TDD contract tests (F13 graceful shutdown harness).
    //!
    //! Happy Path:
    //!   #1 — Trigger shutdown → accountant force-flush completes within
    //!        500ms (T-39 phase 2). Asserts the flush was CALLED (mock
    //!        counter > 0) — this fails on the RED stub, which never calls
    //!        the target.
    //!   #2 — Full shutdown completes within 3000ms (T-19).
    //!
    //! Boundary (cite T-19, T-39, G15):
    //!   #1 — Accountant hangs (simulated) → phase 2 budget exceeded →
    //!        forced transition to phase 3, `error!` logged, report records
    //!        `TimedOut`.
    //!   #2 — OhmSupervisor hangs → phase 3 budget exceeded → orchestrator
    //!        returns `TimedOut` (Job Object reaps OHM via kernel on exit).
    //!   #3 — Force-flush fails (returns Err) → logged, shutdown continues
    //!        (G15 data-loss-accepted per R11). Report records `Completed`
    //!        (the phase ran to completion — the error is G15's concern).
    //!   #4 — Double shutdown signal (Ctrl+C twice) → second is no-op; first
    //!        is already in progress.
    //!
    //! Fixtures: F13 (graceful shutdown harness). Uses `tokio::time::timeout`
    //! wrappers (not `start_paused`) so the hang-mock tests don't consume
    //! real wall-clock beyond their phase budgets.

    use super::*;
    use sidebar_domain::event::Event;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tokio::sync::broadcast;

    /// A boxed future returned by a mock behavior closure. Using
    /// `Pin<Box<dyn Future>>` lets each test return either an immediately-
    /// ready future (`async { Ok(()) }`), an error future, or a NEVER-resolving
    /// future (`std::future::pending()`) to simulate a hang that the
    /// orchestrator's `tokio::time::timeout` can cancel.
    type MockFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

    /// Injectable mock targets for F13 tests. Each method records its call
    /// count in shared counters so tests can assert the orchestrator invoked
    /// the phase. The `flush_behavior` / `ohm_behavior` closures return a
    /// boxed future so a test can simulate a hang via `pending()` (which IS
    /// cancellable by `tokio::time::timeout`, unlike `std::thread::sleep`).
    #[allow(dead_code)]
    struct MockTargets {
        flush_calls: Arc<Mutex<usize>>,
        ohm_calls: Arc<Mutex<usize>>,
        flush_behavior: Box<dyn Fn() -> MockFuture + Send + Sync>,
        ohm_behavior: Box<dyn Fn() -> MockFuture + Send + Sync>,
    }

    impl MockTargets {
        fn new_immediate() -> (Self, Arc<Mutex<usize>>, Arc<Mutex<usize>>) {
            let flush_calls = Arc::new(Mutex::new(0usize));
            let ohm_calls = Arc::new(Mutex::new(0usize));
            let fc = flush_calls.clone();
            let oc = ohm_calls.clone();
            let me = Self {
                flush_calls: flush_calls.clone(),
                ohm_calls: ohm_calls.clone(),
                flush_behavior: Box::new(move || {
                    *fc.lock().unwrap() += 1;
                    Box::pin(async { Ok(()) })
                }),
                ohm_behavior: Box::new(move || {
                    *oc.lock().unwrap() += 1;
                    Box::pin(async { Ok(()) })
                }),
            };
            (me, flush_calls, ohm_calls)
        }
    }

    impl ShutdownTargets for MockTargets {
        async fn force_flush(&mut self) -> Result<(), String> {
            (self.flush_behavior)().await
        }
        async fn teardown_ohm(&mut self) -> Result<(), String> {
            (self.ohm_behavior)().await
        }
    }

    // =================================================================
    // Happy Path #1 — force-flush called within 500ms (T-39 phase 2).
    // RED: stub never calls force_flush → flush_calls == 0 → fails.
    // =================================================================

    /// Cited: Story 7.5 Happy Path #1, F13, T-39 phase 2.
    #[tokio::test]
    async fn shutdown_calls_force_flush_within_500ms() {
        let (mut targets, flush_calls, _ohm_calls) = MockTargets::new_immediate();
        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));

        let start = Instant::now();
        let report = run_shutdown(cancel, &mut targets, &guard).await;
        let elapsed = start.elapsed();

        // The flush MUST have been called (the core RED assertion).
        let calls = *flush_calls.lock().unwrap();
        assert!(
            calls >= 1,
            "force_flush MUST be called during shutdown (got {calls} calls)"
        );

        // T-39 phase 2 budget.
        assert!(
            elapsed < PHASE_FLUSH_BUDGET + Duration::from_millis(100),
            "force-flush phase took {elapsed:?}, expected < {PHASE_FLUSH_BUDGET:?} (+jitter)"
        );

        // Report records the flush as completed.
        assert_eq!(
            report.flush,
            PhaseOutcome::Completed,
            "flush phase completed"
        );
    }

    // =================================================================
    // Happy Path #2 — full shutdown within 3000ms (T-19).
    // RED: stub returns immediately so this passes trivially. GREEN
    // strengthens to assert both phases ran.
    // =================================================================

    /// Cited: Story 7.5 Happy Path #2, T-19.
    #[tokio::test]
    async fn full_shutdown_completes_within_3000ms() {
        let (mut targets, flush_calls, ohm_calls) = MockTargets::new_immediate();
        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));

        let start = Instant::now();
        let report = run_shutdown(cancel, &mut targets, &guard).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < TOTAL_SHUTDOWN_BUDGET,
            "total shutdown took {elapsed:?}, expected < {TOTAL_SHUTDOWN_BUDGET:?}"
        );

        // GREEN strength: both phases were invoked.
        assert!(*flush_calls.lock().unwrap() >= 1, "flush phase ran");
        assert!(*ohm_calls.lock().unwrap() >= 1, "ohm phase ran");
        assert_eq!(report.flush, PhaseOutcome::Completed);
        assert_eq!(report.ohm, PhaseOutcome::Completed);
    }

    // =================================================================
    // Boundary #1 — accountant hangs → phase 2 TimedOut, phase 3 runs.
    // RED: stub doesn't hang (returns immediately) AND doesn't call
    // targets → assertions about TimedOut + ohm_calls fail.
    // =================================================================

    /// Cited: Story 7.5 Boundary #1, T-39, G15.
    #[tokio::test]
    async fn accountant_hang_times_out_phase2_and_continues() {
        // Flush hangs forever (simulated SQLite stuck). The orchestrator MUST
        // time out phase 2 at 500ms and still run phase 3.
        let flush_calls = Arc::new(Mutex::new(0usize));
        let ohm_calls = Arc::new(Mutex::new(0usize));
        let fc = flush_calls.clone();
        let oc = ohm_calls.clone();
        let mut targets = MockTargets {
            flush_calls: flush_calls.clone(),
            ohm_calls: ohm_calls.clone(),
            flush_behavior: Box::new(move || {
                *fc.lock().unwrap() += 1;
                // Signal that we entered, then hang. `pending()` is a
                // never-resolving future that IS cancellable by
                // `tokio::time::timeout` (unlike `std::thread::sleep`, which
                // blocks the OS thread and prevents the timeout from firing).
                Box::pin(std::future::pending())
            }),
            ohm_behavior: Box::new(move || {
                *oc.lock().unwrap() += 1;
                Box::pin(async { Ok(()) })
            }),
        };

        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let report = tokio::time::timeout(
            TOTAL_SHUTDOWN_BUDGET + Duration::from_secs(1),
            run_shutdown(cancel, &mut targets, &guard),
        )
        .await
        .expect("orchestrator must not exceed total budget even when a phase hangs");
        let elapsed = start.elapsed();

        // Phase 2 timed out (the hang exceeded 500ms).
        assert_eq!(
            report.flush,
            PhaseOutcome::TimedOut,
            "flush phase MUST time out when accountant hangs"
        );
        // Phase 3 still ran — the orchestrator advanced.
        assert!(
            *ohm_calls.lock().unwrap() >= 1,
            "ohm teardown MUST run even after flush timed out"
        );
        // Total elapsed bounded by phase 2 + phase 3 budgets (not the 10s hang).
        assert!(
            elapsed < PHASE_FLUSH_BUDGET + PHASE_OHM_BUDGET + Duration::from_millis(500),
            "orchestrator advanced past the hang within budgets: {elapsed:?}"
        );
    }

    // =================================================================
    // Boundary #2 — OhmSupervisor hangs → phase 3 TimedOut.
    // RED: stub doesn't hang + doesn't call ohm → assertions fail.
    // =================================================================

    /// Cited: Story 7.5 Boundary #2, T-39, G10.
    #[tokio::test]
    async fn ohm_hang_times_out_phase3() {
        let flush_calls = Arc::new(Mutex::new(0usize));
        let ohm_calls = Arc::new(Mutex::new(0usize));
        let fc = flush_calls.clone();
        let oc = ohm_calls.clone();
        let mut targets = MockTargets {
            flush_calls: flush_calls.clone(),
            ohm_calls: ohm_calls.clone(),
            flush_behavior: Box::new(move || {
                *fc.lock().unwrap() += 1;
                Box::pin(async { Ok(()) })
            }),
            ohm_behavior: Box::new(move || {
                *oc.lock().unwrap() += 1;
                // `pending()` — never resolves, but IS cancellable by
                // `tokio::time::timeout` (unlike `std::thread::sleep`).
                Box::pin(std::future::pending())
            }),
        };

        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));
        let start = Instant::now();
        let report = tokio::time::timeout(
            TOTAL_SHUTDOWN_BUDGET + Duration::from_secs(1),
            run_shutdown(cancel, &mut targets, &guard),
        )
        .await
        .expect("orchestrator must not hang when ohm teardown hangs");
        let elapsed = start.elapsed();

        assert_eq!(
            report.ohm,
            PhaseOutcome::TimedOut,
            "ohm phase MUST time out when teardown hangs"
        );
        assert!(
            elapsed < TOTAL_SHUTDOWN_BUDGET,
            "total shutdown within T-19 even with ohm hang: {elapsed:?}"
        );
    }

    // =================================================================
    // Boundary #3 — force-flush returns Err → logged, continues (G15).
    // RED: stub doesn't call flush → report.flush is NotConfigured, not
    // the expected outcome.
    // =================================================================

    /// Cited: Story 7.5 Boundary #3, G15, R11.
    #[tokio::test]
    async fn force_flush_error_continues_shutdown() {
        let flush_calls = Arc::new(Mutex::new(0usize));
        let ohm_calls = Arc::new(Mutex::new(0usize));
        let fc = flush_calls.clone();
        let oc = ohm_calls.clone();
        let mut targets = MockTargets {
            flush_calls: flush_calls.clone(),
            ohm_calls: ohm_calls.clone(),
            flush_behavior: Box::new(move || {
                *fc.lock().unwrap() += 1;
                Box::pin(async {
                    Err("SQLite disk full (G15 — data loss accepted per R11)".to_string())
                })
            }),
            ohm_behavior: Box::new(move || {
                *oc.lock().unwrap() += 1;
                Box::pin(async { Ok(()) })
            }),
        };

        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));
        let _report = run_shutdown(cancel, &mut targets, &guard).await;

        // Flush was called and returned Err — the phase still "ran to
        // completion" from the orchestrator's view; G15 logs the error.
        assert!(*flush_calls.lock().unwrap() >= 1, "flush was attempted");
        assert!(
            *ohm_calls.lock().unwrap() >= 1,
            "ohm teardown ran despite flush error (G15 — shutdown continues)"
        );
    }

    // =================================================================
    // Boundary #4 — double signal → second is no-op.
    // RED: the stub's guard logic compiles, so this may pass on the stub.
    // It's the meaningful GREEN assertion (call count stays at 1 after the
    // second invocation).
    // =================================================================

    /// Cited: Story 7.5 Boundary #4.
    #[tokio::test]
    async fn double_shutdown_signal_second_is_noop() {
        let (mut targets, flush_calls, _ohm_calls) = MockTargets::new_immediate();
        let cancel = CancellationToken::new();
        let guard = Arc::new(AtomicBool::new(false));

        // First shutdown.
        let _report1 = run_shutdown(cancel.clone(), &mut targets, &guard).await;
        let calls_after_first = *flush_calls.lock().unwrap();

        // Second shutdown — MUST be a no-op.
        let report2 = run_shutdown(cancel, &mut targets, &guard).await;
        let calls_after_second = *flush_calls.lock().unwrap();

        assert_eq!(
            calls_after_first, calls_after_second,
            "second shutdown signal MUST NOT re-invoke flush (double-signal guard)"
        );
        // The second report records "not configured" for both phases (no-op).
        assert_eq!(report2.flush, PhaseOutcome::NotConfigured);
        assert_eq!(report2.ohm, PhaseOutcome::NotConfigured);
    }

    // =================================================================
    // Signal handler smoke — spawns a task that cancels on Ctrl+C. We can't
    // send a real Ctrl+C in a unit test, so this just asserts the spawn
    // returns a JoinHandle and the task is abortable (the production path).
    // =================================================================

    /// Cited: Story 7.5 signal handler wiring.
    #[tokio::test]
    async fn spawn_signal_handler_returns_abortable_handle() {
        let cancel = CancellationToken::new();
        let handle = spawn_signal_handler(cancel.clone());
        // The task is pending (waiting on ctrl_c). Abort it — production
        // detaches; tests abort to clean up.
        handle.abort();
        let _ = handle.await;
        // Token was NOT cancelled (we aborted before any signal arrived).
        assert!(!cancel.is_cancelled());
    }

    #[tokio::test]
    async fn signal_handler_joins_after_shared_shutdown_request() {
        let cancel = CancellationToken::new();
        let (events, _rx) = broadcast::channel(4);
        let signal = ShutdownSignal::new(cancel.clone(), events);
        let handle = spawn_signal_handler_with_signal(signal.clone());

        signal.request();
        tokio::time::timeout(Duration::from_millis(200), handle)
            .await
            .expect("signal handler must stop after cancellation")
            .expect("signal handler task must join cleanly");
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn shutdown_signal_cancels_and_emits_once() {
        let cancel = CancellationToken::new();
        let (events, mut rx) = broadcast::channel(4);
        let signal = ShutdownSignal::new(cancel.clone(), events);

        assert!(signal.request());
        assert!(cancel.is_cancelled());
        assert_eq!(rx.try_recv(), Ok(Event::Shutdown));
        assert!(!signal.request());
        assert!(
            rx.try_recv().is_err(),
            "repeated requests emit no duplicate event"
        );
    }
}
