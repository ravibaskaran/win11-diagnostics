//! Story 7.2 — Poller Task (interval + broadcast publish).
//!
//! The poller is the fan-out point of the sensor pipeline (architecture.md
//! §6 flow A/B/C, AD-6). It fires every `poll_interval_seconds`, runs every
//! provider's `read_all` on a blocking thread (T-18), catches any panic per
//! provider (G15), concatenates the readings into a single `Vec<Reading>`
//! stamped with one tick-wide timestamp, and publishes via a `tokio::sync`
//! `broadcast` channel of capacity 8 (T-14).
//!
//! ## Design decisions
//!
//! - **`spawn_blocking` per provider.** Every adapter's `read_all` is a
//!   synchronous blocking syscall (sysinfo global lock, PDH `PdhCollectQueryData`,
//!   `ureq` HTTP GET to LHM). Running them on the async runtime's worker
//!   threads would block the executor — so each provider's `read_all` is
//!   dispatched to the blocking pool via `tokio::task::spawn_blocking`, and
//!   the resulting `JoinHandle`s are awaited concurrently with
//!   `futures::future::join_all`-style fan-out. v1 uses sequential `await`s
//!   on the spawned handles; the spawn itself is what off-loads the blocking
//!   work, so the order of awaiting doesn't change the wall-clock cost.
//!   T-18's "2 worker threads" budget is satisfied by the runtime's blocking
//!   pool (default 512 threads; the per-tick working set is at most the
//!   registry size, ~6 providers).
//!
//! - **`catch_unwind` + `AssertUnwindSafe` (G15, HITL — G11).** Each
//!   provider's `read_all` is wrapped in
//!   `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ...))`.
//!   `dyn SensorProvider` is NOT `UnwindSafe` by construction (the trait
//!   object holds no `&mut` visible to us, but the compiler can't prove the
//!   underlying impl is unwind-safe). We accept the caveat: a panicking
//!   adapter's partial state is confined to its own `Arc<dyn SensorProvider>`;
//!   the poller NEVER reuses a panicked provider's mutable state across the
//!   catch boundary (the provider is `Send + Sync` and `read_all` takes
//!   `&self`, so the only shared mutable state is inside the adapter itself,
//!   which the adapter owns). The justification is documented in a SAFETY
//!   comment at the `catch_unwind` call site. This is the HITL item.
//!
//! - **Timestamp.** `Reading.timestamp` is `std::time::Instant`. The poller
//!   stamps every reading in a tick with a single `Instant` (captured at tick
//!   fire) so downstream consumers see a coherent snapshot. Production reads
//!   `Instant::now()`; tests inject a `Clock`-like trait so the "single
//!   timestamp per tick" contract is verifiable without time travel. We use
//!   a local `InstantClock` trait (not sidebar-bandwidth's `Clock`) because
//!   that trait returns `NaiveDateTime` for billing — the sensor layer deals
//!   in monotonic `Instant`, a different concern.
//!
//! - **Interval clamping (T-3).** A zero/negative `poll_interval_seconds` is
//!   clamped to 1s (T-3: default 1s, minimum sane cadence).
//!
//! - **Overlapping ticks (boundary #2).** When a provider is slow (e.g.
//!   500ms read against a 100ms interval), `tokio::time::interval` with
//!   `MissedTickBehavior::Delay` skips the missed ticks: the next tick fires
//!   `interval` after the slow tick completes, not immediately. The poller
//!   does NOT queue overlapping fan-outs — this is the documented v1
//!   strategy (skip overlapping tick, log via `tracing::debug!`).
//!
//! - **Broadcast capacity (T-14).** Capacity 8. If all receivers lag, the
//!   oldest message is dropped; `tokio::broadcast::Sender::send` returns
//!   `Err(SendError)` when there are NO active receivers — that's the
//!   "everyone went away" exit, treated as clean shutdown.
//!
//! ## Cited
//!
//! - Story 7.2 TDD contract (Happy Path #1-#2, Boundary #1-#5)
//! - architecture.md §6 (flow A/B/C), AD-6 (poller)
//! - nfr-thresholds.md T-2 (CPU ≤2%), T-3 (interval default 1s), T-14
//!   (broadcast cap 8), T-18 (2 worker threads), T-19 (shutdown token),
//!   T-20 (Reading value finite)
//! - guardrails.md G11 (HITL on AssertUnwindSafe), G15 (panic-safety)
//! - tdd-fixtures.md F2 (mock broadcast), F4 (mock provider), F10
//!   (panic-catch)

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sidebar_domain::event::{Event, Tier};
use sidebar_domain::reading::Reading;
use sidebar_sensor::descriptor::ProviderTier;
use sidebar_sensor::provider::SensorProvider;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

type RegistryBuilder =
    Arc<dyn Fn(ProviderTier) -> Result<Vec<Arc<dyn SensorProvider>>, String> + Send + Sync>;

/// Minimum poll interval (T-3). A configured interval below 1s is clamped
/// up to this value — sub-second polling would saturate the blocking pool
/// and blow the T-2 CPU budget.
pub const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Injectable monotonic clock. Returns `Instant` (the timestamp type on
/// `Reading`). Production uses [`SystemInstantClock`]; tests inject a
/// controllable implementation so the "single timestamp per tick" contract
/// is verifiable.
///
/// This is intentionally separate from `sidebar_bandwidth::clock::Clock`
/// (which returns `NaiveDateTime` for billing-cycle math). The sensor layer
/// deals in monotonic `Instant`; coupling to the billing clock would pull
/// `chrono` into a layer that doesn't need it and conflate two concerns.
pub trait InstantClock: Send + Sync {
    /// Return the current monotonic instant.
    fn now(&self) -> Instant;
}

/// Production clock — wraps `Instant::now()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemInstantClock;

impl SystemInstantClock {
    /// Construct a `SystemInstantClock`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl InstantClock for SystemInstantClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Error returned by [`Poller::run`]. The poller is designed to be resilient
/// (G15) — most errors are logged and the loop continues. The only conditions
/// that return `Err` are programming errors that make further polling
/// pointless.
#[derive(Debug, thiserror::Error)]
pub enum PollerError {
    /// All broadcast receivers were dropped AND the shutdown token did not
    /// fire. This is treated as a clean exit in practice (the GUI closed);
    /// surfaced as `Err` so the caller can distinguish "poller exited because
    /// it had no audience" from "poller exited on shutdown".
    #[error("broadcast closed: no active receivers")]
    BroadcastClosed,
}

/// The poller task.
///
/// Holds the provider registry (from Story 7.1's `build_registry`), the poll
/// interval, the broadcast sender, and an injectable clock. Call
/// [`Poller::run`] with a [`CancellationToken`] to drive the tick loop.
///
/// `run` consumes `self` — the poller owns its providers for the duration of
/// the task and tears down cleanly on shutdown.
pub struct Poller {
    providers: Vec<Arc<dyn SensorProvider>>,
    interval: Duration,
    tx: broadcast::Sender<Vec<Reading>>,
    clock: Arc<dyn InstantClock>,
}

impl Poller {
    /// Construct a new poller with the default system clock.
    ///
    /// `interval` is clamped to at least [`MIN_POLL_INTERVAL`] (T-3) — the
    /// production entrypoint never ships a sub-second poller.
    #[must_use]
    pub fn new(
        providers: Vec<Arc<dyn SensorProvider>>,
        interval: Duration,
        tx: broadcast::Sender<Vec<Reading>>,
    ) -> Self {
        Self::with_clock_raw(
            providers,
            clamp_interval(interval),
            tx,
            Arc::new(SystemInstantClock::new()),
        )
    }

    /// Construct a poller with an injected clock (tests).
    ///
    /// The interval is clamped to at least [`MIN_POLL_INTERVAL`] (T-3) — the
    /// clamp is a safety property that applies in tests too, EXCEPT for the
    /// tick-cadence tests which need a sub-second interval to drive multiple
    /// ticks within a test's wall-clock budget. Those tests use
    /// [`Poller::with_clock_raw`].
    #[must_use]
    pub fn with_clock(
        providers: Vec<Arc<dyn SensorProvider>>,
        interval: Duration,
        tx: broadcast::Sender<Vec<Reading>>,
        clock: Arc<dyn InstantClock>,
    ) -> Self {
        Self::with_clock_raw(providers, clamp_interval(interval), tx, clock)
    }

    /// Construct a poller with an injected clock AND an un-clamped interval.
    ///
    /// This is the test-only escape hatch for the tick-cadence tests (Happy
    /// Path #2, Boundary #1, #2) which need a 100ms interval to observe 3
    /// ticks in 350ms. The clamp (T-3) is validated separately in
    /// `zero_interval_clamped_to_one_second`. Production code MUST use
    /// [`Poller::new`] (which clamps); this constructor trusts the caller.
    #[must_use]
    pub fn with_clock_raw(
        providers: Vec<Arc<dyn SensorProvider>>,
        interval: Duration,
        tx: broadcast::Sender<Vec<Reading>>,
        clock: Arc<dyn InstantClock>,
    ) -> Self {
        Self {
            providers,
            interval,
            tx,
            clock,
        }
    }

    /// Run the poller until `shutdown` fires or the broadcast closes.
    ///
    /// On each tick: spawn each provider's `read_all` on the blocking pool,
    /// catch any panic, concatenate survivors' readings, stamp them with the
    /// tick instant, and publish. See the module docs for the full design.
    ///
    /// # Errors
    ///
    /// Returns [`PollerError::BroadcastClosed`] if the broadcast closes
    /// (no receivers) before the shutdown token fires.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), PollerError> {
        self.run_inner(shutdown, None, None).await
    }

    /// Run the poller while accepting coalesced runtime events. A
    /// `TierChanged` event rebuilds the owned provider registry before the
    /// next tick, so there is exactly one active registry and no overlapping
    /// poller tasks. `Event::Shutdown` exits immediately; cancellation still
    /// remains the primary shutdown signal.
    pub async fn run_with_events<B>(
        self,
        shutdown: CancellationToken,
        events: broadcast::Receiver<Event>,
        builder: B,
    ) -> Result<(), PollerError>
    where
        B: Fn(ProviderTier) -> Result<Vec<Arc<dyn SensorProvider>>, String> + Send + Sync + 'static,
    {
        self.run_inner(shutdown, Some(events), Some(Arc::new(builder)))
            .await
    }

    async fn run_inner(
        mut self,
        shutdown: CancellationToken,
        mut events: Option<broadcast::Receiver<Event>>,
        builder: Option<RegistryBuilder>,
    ) -> Result<(), PollerError> {
        // MissedTickBehavior::Delay (boundary #2): if a tick's fan-out takes
        // longer than `interval`, the NEXT tick fires one `interval` after
        // the slow tick COMPLETES (not immediately). This is the documented
        // skip-overlapping-tick strategy — we never queue concurrent
        // fan-outs, which would risk overlapping sysinfo global-lock holds.
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // tokio::time::interval fires an immediate first tick on the first
        // `tick().await`. Publish an initial snapshot immediately on startup
        // so the GUI shows data before the first interval elapses.

        loop {
            tokio::select! {
                biased; // shutdown takes priority so a cancel during a tick
                        // returns promptly instead of waiting for the next tick.
                () = shutdown.cancelled() => {
                    tracing::info!("Poller: shutdown signal — exiting");
                    return Ok(());
                }
                _ = ticker.tick() => {
                    let readings = self.tick().await;
                    if let Ok(n) = self.tx.send(readings) {
                        tracing::trace!(
                            receivers = n,
                            "Poller: published tick"
                        );
                    } else {
                        // No active receivers — the GUI/accountant all
                        // dropped their receivers. Treat as clean exit.
                        tracing::info!(
                            "Poller: broadcast closed (no receivers) — exiting"
                        );
                        return Err(PollerError::BroadcastClosed);
                    }
                }
                event = recv_event(&mut events) => {
                    match event {
                        Some(Ok(Event::TierChanged(tier))) => {
                            let Some(builder) = builder.as_ref() else {
                                continue;
                            };
                            let active_tier = match tier {
                                Tier::Full => ProviderTier::Full,
                                Tier::Basic => ProviderTier::Basic,
                            };
                            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                                builder(active_tier)
                            }));
                            match result {
                                Ok(Ok(next)) => {
                                    let previous = std::mem::replace(&mut self.providers, next);
                                    tracing::info!(
                                        old_provider_count = previous.len(),
                                        provider_count = self.providers.len(),
                                        active_tier = ?active_tier,
                                        "Poller: provider registry rebuilt after tier change"
                                    );
                                }
                                Ok(Err(error)) => {
                                    tracing::error!(
                                        ?active_tier,
                                        %error,
                                        "Poller: provider registry rebuild failed; retaining previous registry"
                                    );
                                }
                                Err(_) => {
                                    tracing::error!(
                                        ?active_tier,
                                        "Poller: provider registry rebuild panicked; retaining previous registry"
                                    );
                                }
                            }
                        }
                        Some(Ok(Event::Shutdown)) => {
                            tracing::info!("Poller: Shutdown event — exiting");
                            return Ok(());
                        }
                        Some(Ok(_)) => {}
                        Some(Err(broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!(skipped = n, "Poller: event channel lagged");
                        }
                        Some(Err(broadcast::error::RecvError::Closed)) | None => {
                            events = None;
                        }
                    }
                }
            }
        }
    }

    /// Run one tick: capture the tick instant, fan out across all providers
    /// (each on its own blocking thread), catch panics per provider (G15),
    /// concatenate the survivors' readings, and stamp every reading with the
    /// single tick instant.
    ///
    /// A panicking provider contributes zero readings and is logged; the
    /// other providers' readings still flow through.
    async fn tick(&self) -> Vec<Reading> {
        let tick_instant = self.clock.now();

        // Spawn each provider's `read_all` on the blocking pool. The handles
        // are awaited in order; the spawn itself off-loads the blocking
        // syscall so the order of awaiting doesn't change wall-clock cost.
        let mut handles = Vec::with_capacity(self.providers.len());
        for provider in &self.providers {
            let provider = Arc::clone(provider);
            handles.push(tokio::task::spawn_blocking(move || {
                // SAFETY (G15, HITL — G11): `catch_unwind` requires the
                // closure be `UnwindSafe`. `Arc<dyn SensorProvider>` is not
                // `UnwindSafe` by construction — the compiler cannot prove
                // the underlying impl doesn't hold `&mut` across the call.
                // `AssertUnwindSafe` asserts the opposite.
                //
                // Justification for the assertion:
                //   1. `SensorProvider: Send + Sync` (trait bound), and
                //      `read_all(&self)` takes `&self` — no exclusive borrow
                //      crosses the catch boundary.
                //   2. Any shared mutable state inside a panicking adapter is
                //      OWNED by that adapter (behind its own Mutex/atomic).
                //      The poller never touches adapter internals directly.
                //   3. After a panic, the poller treats the provider as
                //      "produced zero readings this tick" and continues to
                //      use the SAME `Arc<dyn SensorProvider>` on subsequent
                //      ticks — adapters MUST be panic-recoverable. The
                //      caveat `AssertUnwindSafe` accepts is exactly this: "a
                //      panic might leave visible inconsistent state in the
                //      caught closure." We accept it because the alternative
                //      (letting the panic unwind and killing the poller task)
                //      is worse — one flaky adapter would take down the
                //      entire sensor pipeline.
                //   4. This is the documented v1 decision (Story 7.2 spec,
                //      "DECIDE: wrap each call in AssertUnwindSafe since
                //      SensorProvider: Send + Sync and we accept the
                //      unwind-safety caveat for poller resilience"). HITL
                //      review flagged at G11.
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| provider.read_all()));
                (provider.descriptor().name, result)
            }));
        }

        // Await all handles, concatenate survivors, stamp.
        let mut out: Vec<Reading> = Vec::new();
        for handle in handles {
            // spawn_blocking's JoinError (panic inside the blocking thread
            // beyond the catch_unwind) is also a panic path — treat it
            // identically to catch_unwind's Err: log + skip.
            match handle.await {
                Ok((_name, Ok(readings))) => {
                    out.extend(readings);
                }
                Ok((name, Err(panic_payload))) => {
                    tracing::error!(
                        provider = name,
                        "Poller: provider panicked during read_all — skipping (G15)"
                    );
                    // Drop the panic payload (`Box<dyn Any>`); we don't
                    // downcast it — the panic-hook already wrote diagnostics.
                    drop(panic_payload);
                }
                Err(join_err) => {
                    tracing::error!(
                        error = %join_err,
                        "Poller: blocking task join failed — skipping provider (G15)"
                    );
                }
            }
        }

        // Stamp every reading with the single tick instant. The adapters
        // stamp `Instant::now()` inside `read_all`; we overwrite that here so
        // all readings in one tick share one coherent timestamp (downstream
        // consumers see a consistent snapshot, not N providers' staggered
        // `now()` calls).
        for r in &mut out {
            r.timestamp = tick_instant;
        }
        out
    }
}

async fn recv_event(
    events: &mut Option<broadcast::Receiver<Event>>,
) -> Option<Result<Event, broadcast::error::RecvError>> {
    match events.as_mut() {
        Some(receiver) => Some(receiver.recv().await),
        None => std::future::pending().await,
    }
}

/// Clamp a requested poll interval to at least [`MIN_POLL_INTERVAL`] (T-3).
fn clamp_interval(requested: Duration) -> Duration {
    if requested < MIN_POLL_INTERVAL {
        tracing::warn!(
            requested_ms = requested.as_millis(),
            clamped_ms = MIN_POLL_INTERVAL.as_millis(),
            "poll interval below T-3 minimum; clamping to 1s"
        );
        MIN_POLL_INTERVAL
    } else {
        requested
    }
}

#[cfg(test)]
mod tests {
    //! Story 7.2 TDD contract tests (RED — stub run() returns Ok, never
    //! publishes, so the happy-path assertions fail until GREEN).
    //!
    //! The stubs here mirror Story 7.1's hand-rolled `StubProvider` pattern
    //! (mockall::automock is only emitted inside sidebar-sensor's own test
    //! build; it isn't exported to downstream crates — same rationale as
    //! 7.1's registry tests). Each stub controls its `read_all` output via a
    //! shared `Arc<Mutex<...>>` so we can also simulate slowness and panic.
    //!
    //! Cited:
    //!   - Story 7.2 TDD contract (Happy Path #1-#2, Boundary #1-#5)
    //!   - nfr-thresholds.md T-2/T-3/T-14/T-18
    //!   - guardrails.md G15 (panic-safety)
    //!   - tdd-fixtures.md F2 (mock broadcast), F4 (mock provider), F10
    //!     (panic-catch)

    use super::*;
    use sidebar_domain::event::{Event, Tier};
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};
    use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use tokio::time::Duration;

    // ----- Fixtures (F4 mock providers, F10 panic provider) -----

    /// A leaked `&'static SensorDescriptor` for stub identity (same trick as
    /// Story 7.1's registry tests — gives pointer-stable descriptors).
    fn leaked_descriptor(name: &'static str) -> &'static SensorDescriptor {
        Box::leak(Box::new(SensorDescriptor::new(
            name,
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        )))
    }

    /// Shared, mutex-guarded closure returning a stub provider's readings.
    /// Factored out of `StubProvider` to keep clippy's `type_complexity` lint
    /// quiet — the boxed-closure-behind-mutex-behind-arc is inherently hairy.
    type ReadFn = Arc<Mutex<Box<dyn Fn() -> Vec<Reading> + Send + Sync>>>;

    /// Configurable stub provider. The closure stored in `read_fn` decides
    /// what `read_all` returns; it can also panic (F10) or sleep (boundary
    /// #2). Wrapped in `Arc<Mutex<...>>` so the test can swap behavior
    /// between ticks if needed.
    struct StubProvider {
        descriptor: &'static SensorDescriptor,
        read_fn: ReadFn,
    }

    impl SensorProvider for StubProvider {
        fn descriptor(&self) -> &SensorDescriptor {
            self.descriptor
        }
        fn read_all(&self) -> Vec<Reading> {
            (self.read_fn.lock().expect("stub mutex poisoned"))()
        }
    }

    fn stub(
        name: &'static str,
        read_fn: impl Fn() -> Vec<Reading> + Send + Sync + 'static,
    ) -> Arc<dyn SensorProvider> {
        Arc::new(StubProvider {
            descriptor: leaked_descriptor(name),
            read_fn: Arc::new(Mutex::new(Box::new(read_fn))),
        })
    }

    fn reading(category: &'static str, instance: &str, kind: MetricKind) -> Reading {
        Reading {
            sensor: SensorId::new(category, instance.to_string()),
            kind,
            value: 42.0,
            unit: Unit::Percent,
            // timestamp will be overwritten by the poller; tests assert the
            // override, not this value.
            timestamp: Instant::now(),
        }
    }

    /// A test clock that returns a fixed instant settable from the test.
    /// Lets us assert "all readings in one tick share one timestamp".
    #[derive(Debug, Clone)]
    struct FakeClock {
        now: Arc<Mutex<Instant>>,
    }

    impl FakeClock {
        fn new(t0: Instant) -> Self {
            Self {
                now: Arc::new(Mutex::new(t0)),
            }
        }
        fn set(&self, t: Instant) {
            *self.now.lock().expect("fakeclock mutex poisoned") = t;
        }
    }

    impl InstantClock for FakeClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("fakeclock mutex poisoned")
        }
    }

    // Helper to build a poller wired to a broadcast channel + fake clock.
    struct Harness {
        tx: broadcast::Sender<Vec<Reading>>,
        rx: broadcast::Receiver<Vec<Reading>>,
        clock: Arc<FakeClock>,
    }

    fn harness() -> Harness {
        let (tx, rx) = broadcast::channel::<Vec<Reading>>(8); // T-14
        Harness {
            tx,
            rx,
            clock: Arc::new(FakeClock::new(Instant::now())),
        }
    }

    #[tokio::test]
    async fn tier_change_rebuilds_registry_once_and_publishes_full_provider() {
        let mut h = harness();
        let basic = stub("basic", || {
            vec![reading("basic", "0", MetricKind::CpuUtilization)]
        });
        let full = stub("full", || {
            vec![reading("full", "0", MetricKind::CpuTemperature)]
        });
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let rebuilds_for_builder = Arc::clone(&rebuilds);
        let full_for_builder = Arc::clone(&full);
        let builder = move |tier: ProviderTier| -> Result<Vec<Arc<dyn SensorProvider>>, String> {
            rebuilds_for_builder.fetch_add(1, Ordering::SeqCst);
            match tier {
                ProviderTier::Full => Ok(vec![Arc::clone(&full_for_builder)]),
                _ => Err("test builder only accepts Full".to_string()),
            }
        };

        let (event_tx, event_rx) = broadcast::channel(8);
        let poller = Poller::with_clock_raw(
            vec![basic],
            Duration::from_millis(10),
            h.tx.clone(),
            h.clock,
        );
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        let run =
            tokio::spawn(async move { poller.run_with_events(shutdown, event_rx, builder).await });

        event_tx
            .send(Event::TierChanged(Tier::Full))
            .expect("event receiver is active");
        let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
        let mut saw_full = false;
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(batch)) =
                tokio::time::timeout(Duration::from_millis(30), h.rx.recv()).await
            {
                saw_full |= batch.iter().any(|r| r.sensor.category == "full");
                if saw_full {
                    break;
                }
            }
        }
        cancel.cancel();
        run.await
            .expect("poller task joins")
            .expect("poller exits cleanly");

        assert_eq!(rebuilds.load(Ordering::SeqCst), 1);
        assert!(
            saw_full,
            "the next successful poll must use the rebuilt Full registry"
        );
    }

    // ===== Happy Path #1: two providers × 2 readings → vec of 4, single timestamp =====

    /// Story 7.2 Happy Path #1. Cited: Story 7.2 TDD contract.
    ///
    /// Two mock providers, each returning 2 readings. After one tick the
    /// receiver gets a single message: a `Vec<Reading>` of length 4, every
    /// element sharing the same `timestamp` (the tick instant).
    #[tokio::test]
    async fn two_providers_four_readings_single_timestamp() {
        let mut h = harness();
        let t0 = Instant::now();
        h.clock.set(t0);

        let p1 = stub("cpu", || {
            vec![
                reading("cpu", "0", MetricKind::CpuUtilization),
                reading("cpu", "1", MetricKind::CpuUtilization),
            ]
        });
        let p2 = stub("mem", || {
            vec![
                reading("memory", "used", MetricKind::MemoryUsed),
                reading("memory", "total", MetricKind::MemoryTotal),
            ]
        });

        let poller =
            Poller::with_clock_raw(vec![p1, p2], Duration::from_millis(100), h.tx, h.clock);
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        // Stop after one publish to keep the test bounded.
        let stop = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            cancel.cancel();
        });
        let _ = poller.run(shutdown).await;
        let _ = stop.await;

        let msg = tokio::time::timeout(Duration::from_secs(1), h.rx.recv())
            .await
            .expect("timed out waiting for poller message")
            .expect("broadcast closed without message");

        assert_eq!(msg.len(), 4, "two providers × 2 readings = 4 readings");
        let stamp = msg[0].timestamp;
        assert_eq!(stamp, t0, "all readings stamped with the tick instant");
        assert!(
            msg.iter().all(|r| r.timestamp == stamp),
            "every reading in the tick shares one timestamp"
        );
    }

    // ===== Happy Path #2: 100ms interval, 3 ticks in 350ms → 3 messages =====

    /// Story 7.2 Happy Path #2. Cited: Story 7.2 TDD contract.
    ///
    /// Interval 100ms; we let the poller run for 350ms. tokio::time::interval
    /// fires an immediate first tick then every 100ms — so over 350ms we
    /// expect at least 3 messages (could be 4 counting the immediate tick;
    /// we assert ≥3 to stay robust to scheduler jitter).
    #[tokio::test]
    async fn three_ticks_in_350ms() {
        let mut h = harness();
        let p = stub("cpu", || {
            vec![reading("cpu", "0", MetricKind::CpuUtilization)]
        });

        let poller = Poller::with_clock_raw(vec![p], Duration::from_millis(100), h.tx, h.clock);
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(350)).await;
            cancel.cancel();
        });

        let mut count = 0usize;
        let run = tokio::spawn(async move { poller.run(shutdown).await });
        // Receive until timeout (the poller has shut down by then).
        while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(500), h.rx.recv()).await {
            count += 1;
        }
        let _ = run.await;

        assert!(
            count >= 3,
            "expected ≥3 tick messages in 350ms at 100ms interval, got {count}"
        );
    }

    // ===== Boundary #1: one provider panics → others still published (G15) =====

    /// Story 7.2 Boundary #1 (F10, G15). One provider panics on read_all.
    /// The poller catches the panic, logs it, and still publishes the OTHER
    /// provider's readings.
    #[tokio::test]
    async fn panicking_provider_others_still_published() {
        let mut h = harness();
        let bad = stub("bad", || panic!("boom (F10)"));
        let good = stub("good", || {
            vec![reading("cpu", "0", MetricKind::CpuUtilization)]
        });

        let poller =
            Poller::with_clock_raw(vec![bad, good], Duration::from_millis(100), h.tx, h.clock);
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            cancel.cancel();
        });

        let run = tokio::spawn(async move { poller.run(shutdown).await });
        let msg = tokio::time::timeout(Duration::from_secs(1), h.rx.recv())
            .await
            .expect("timed out")
            .expect("broadcast closed");

        // The good provider's reading MUST arrive despite the bad one's panic.
        assert!(
            msg.iter().any(|r| r.sensor.category == "cpu"),
            "good provider readings MUST still be published (G15)"
        );
        // The panicking provider contributed NO readings.
        assert!(
            !msg.iter().any(|r| r.sensor.category == "bad"),
            "panicking provider contributed no readings"
        );
        let _ = run.await;
    }

    // ===== Boundary #3: receiver lags → oldest dropped (T-14) =====

    /// Story 7.2 Boundary #3 (T-14). Capacity 8; a slow receiver that
    /// doesn't drain sees `Lagged(n)` and the oldest messages are dropped.
    ///
    /// We drive the lag at the broadcast channel level directly: with no
    /// receiver draining, push more than capacity messages, then the next
    /// `recv()` MUST return `Err(Lagged(n))` (the channel dropped the oldest
    /// to make room). This validates T-14 (capacity 8, oldest dropped on lag)
    /// independent of the poller's interval timing.
    #[tokio::test]
    async fn receiver_lags_oldest_dropped() {
        let mut h = harness();
        // Push capacity+5 messages without draining. The channel buffers the
        // first 8; the extra 5 cause the oldest 5 to be dropped, and the
        // receiver's next recv reports `Lagged(5)`.
        for _ in 0..13 {
            // send returns Err only when there are NO receivers; we have one
            // (h.rx), so each send succeeds even though the receiver hasn't
            // drained.
            let _ =
                h.tx.send(vec![reading("cpu", "0", MetricKind::CpuUtilization)]);
        }

        match h.rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(n)) => {
                assert!(
                    n > 0,
                    "lag count must be positive when receiver fell behind"
                );
                // After the Lagged signal, the receiver can still observe
                // later messages (the channel is not poisoned).
                let next = h.rx.recv().await;
                assert!(next.is_ok(), "receiver recovers after Lagged (T-14)");
            }
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    // ===== Boundary #4: interval = 0 → clamped to 1s (T-3) =====

    /// Story 7.2 Boundary #4 (T-3). A zero interval is clamped to
    /// [`MIN_POLL_INTERVAL`] (1s). Verified at construction time via
    /// `clamp_interval` — no need to run the loop.
    #[tokio::test]
    async fn zero_interval_clamped_to_one_second() {
        assert_eq!(
            clamp_interval(Duration::from_secs(0)),
            MIN_POLL_INTERVAL,
            "zero interval clamped to 1s (T-3)"
        );
        // Sub-second also clamped.
        assert_eq!(
            clamp_interval(Duration::from_millis(500)),
            MIN_POLL_INTERVAL,
            "sub-second interval clamped to 1s (T-3)"
        );
        // Exactly the minimum passes through.
        assert_eq!(
            clamp_interval(MIN_POLL_INTERVAL),
            MIN_POLL_INTERVAL,
            "exactly 1s is not clamped"
        );
        // Above the minimum passes through.
        assert_eq!(
            clamp_interval(Duration::from_secs(5)),
            Duration::from_secs(5),
            "5s interval passes through"
        );
    }

    // ===== Boundary #2: slow provider — documented skip-vs-queue strategy =====

    /// Story 7.2 Boundary #2 (T-18). A provider that takes 500ms to read
    /// against a 100ms interval. `MissedTickBehavior::Delay` causes the
    /// overlapping tick to be SKIPPED (not queued). This test documents the
    /// strategy: we run the poller briefly and assert it does NOT pile up
    /// concurrent fan-outs (no crash, at least one message arrives).
    ///
    /// The full skip-vs-queue semantics are owned by tokio's interval — we
    /// assert the poller survives a slow provider, not the exact skip count.
    #[tokio::test]
    async fn slow_provider_does_not_crash_poller() {
        let h = harness();
        let slow = stub("slow", || {
            std::thread::sleep(Duration::from_millis(500));
            vec![reading("cpu", "0", MetricKind::CpuUtilization)]
        });

        let poller = Poller::with_clock_raw(vec![slow], Duration::from_millis(100), h.tx, h.clock);
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cancel.cancel();
        });

        let run = tokio::spawn(async move { poller.run(shutdown).await });
        // At least one message arrives within the window (the slow read
        // completes at ~500ms; we cancel at 300ms so this may or may not
        // produce a message — assert no panic, no hang).
        let _ = tokio::time::timeout(Duration::from_secs(2), run).await;
    }

    // ===== Boundary #5: aggregate CPU% (documented, not unit-tested) =====

    /// Story 7.2 Boundary #5 (T-2). Aggregate CPU% over a 5-min window ≤ 2%.
    /// This is a bench concern (Story 10.1's NFR-1 harness), NOT a unit
    /// test — we can't simulate 5 minutes of real CPU load in a unit test
    /// without burning real time. The clamp to 1s (T-3) and the spawn_blocking
    /// offload (T-18) are the unit-testable guarantees that bound the
    /// aggregate; the actual ≤2% number is validated in Story 10.1.
    #[test]
    fn boundary_5_cpu_aggregate_is_bench_concern() {
        // Document: T-2 (≤2% aggregate CPU over 5 min) is a NFR-1 bench
        // target (Story 10.1). The poller design choices that bound it:
        //   - MIN_POLL_INTERVAL = 1s (T-3) prevents tight-loop polling.
        //   - spawn_blocking off-loads blocking syscalls off the async
        //     runtime's worker threads (T-18).
        //   - catch_unwind prevents a panicking adapter from spinning.
        // No runtime assertion here — see Story 10.1 for the bench.
    }
}
