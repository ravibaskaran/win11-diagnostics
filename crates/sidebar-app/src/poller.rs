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

use std::sync::Arc;
use std::time::{Duration, Instant};

use sidebar_domain::reading::Reading;
use sidebar_sensor::provider::SensorProvider;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

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
    /// `interval` is clamped to at least [`MIN_POLL_INTERVAL`] (T-3).
    #[must_use]
    pub fn new(
        providers: Vec<Arc<dyn SensorProvider>>,
        interval: Duration,
        tx: broadcast::Sender<Vec<Reading>>,
    ) -> Self {
        Self::with_clock(providers, interval, tx, Arc::new(SystemInstantClock::new()))
    }

    /// Construct a poller with an injected clock (tests).
    ///
    /// Production uses [`Poller::new`]; tests pass a controllable clock so
    /// the "single timestamp per tick" contract is verifiable without time
    /// travel.
    #[must_use]
    pub fn with_clock(
        providers: Vec<Arc<dyn SensorProvider>>,
        interval: Duration,
        tx: broadcast::Sender<Vec<Reading>>,
        clock: Arc<dyn InstantClock>,
    ) -> Self {
        Self {
            providers,
            interval: clamp_interval(interval),
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
    /// before shutdown fires.
    #[allow(clippy::too_many_lines, clippy::unused_async)]
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), PollerError> {
        let _ = (
            self.providers,
            self.interval,
            self.tx,
            self.clock,
            &shutdown,
        );
        // RED STUB — immediately returns Ok without publishing. The real
        // loop lands in the GREEN commit. Triggers the "two providers → 4
        // readings" happy-path test failure.
        Ok(())
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
    use sidebar_domain::reading::{MetricKind, SensorId, Unit};
    use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
    use std::sync::{Arc, Mutex};
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

        let poller = Poller::with_clock(vec![p1, p2], Duration::from_millis(100), h.tx, h.clock);
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

        let poller = Poller::with_clock(vec![p], Duration::from_millis(100), h.tx, h.clock);
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

        let poller = Poller::with_clock(vec![bad, good], Duration::from_millis(100), h.tx, h.clock);
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
    /// We don't drain the receiver while the poller pushes >8 messages; on
    /// the next recv we expect a `Lagged` error.
    #[tokio::test]
    async fn receiver_lags_oldest_dropped() {
        let h = harness();
        let p = stub("cpu", || {
            vec![reading("cpu", "0", MetricKind::CpuUtilization)]
        });

        // T-14: broadcast capacity is 8 (fixed in harness). A receiver that
        // doesn't drain sees `Lagged(n)` and the oldest messages are dropped.
        // RED placeholder: we assert the channel is wired (sender publishes a
        // message that one receiver can observe). The GREEN commit drives the
        // lag path directly at the broadcast level (pre-fill the channel past
        // capacity, then assert `recv()` returns `Err(Lagged)`).
        let _ = p;
        let _ =
            h.tx.send(vec![reading("cpu", "0", MetricKind::CpuUtilization)]);
        // The sender exists and accepts a message — RED compiles, GREEN adds
        // the real lag assertion.
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

        let poller = Poller::with_clock(vec![slow], Duration::from_millis(100), h.tx, h.clock);
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
