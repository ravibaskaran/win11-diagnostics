# TDD Fixtures — sidebar-v1

**Reference for setup/teardown patterns used across the backlog.** Every test in `epics-and-stories.md` cites an `F-*` pattern from this file. The swarm MUST use these verbatim patterns; reinventing fixtures per-story is forbidden (causes drift, hides bugs).

Patterns are Rust-flavored. Where a pattern is Windows-only, it's marked `#[cfg(target_os = "windows")]`.

---

## F1 — TempDir for filesystem/SQLite tests
**Use:** Any test that touches `%APPDATA%`, SQLite, or config files.

```rust
use tempfile::TempDir;

#[test]
fn some_persistence_test() {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("bandwidth.db");
    // ... exercise code with db_path ...
    // TempDir auto-cleans on drop; no manual teardown.
}
```
**Rule:** NEVER write to the real `%APPDATA%\sidebar\` from a test. Always TempDir.
**Cited by:** Story 4.1, 4.2, 4.3, 5.2.

---

## F2 — Mock broadcast channel (tokio)
**Use:** Tests of the poller, BandwidthAccountant, or any `broadcast::Receiver` consumer.

```rust
use tokio::sync::broadcast;

#[tokio::test]
async fn consumer_test() {
    let (tx, rx) = broadcast::channel::<Vec<Reading>>(8);  // T-14
    let mut rx = rx;
    // spawn consumer with rx
    // tx.send(test_readings())?;
    // assert consumer behavior
}
```
**Rule:** Capacity MUST match T-14 (`8`) so overflow behavior is exercised identically to prod.
**Cited by:** Story 5.2, Story 7.2.

---

## F3 — Injectable Clock for time-dependent logic
**Use:** Tests of billing rollover, debounce, anything that branches on `Local::now()`.

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> chrono::NaiveDateTime;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::NaiveDateTime { chrono::Local::now().naive_local() }
}

#[cfg(test)]
pub struct FakeClock(Mutex<NaiveDateTime>);
#[cfg(test)]
impl FakeClock {
    pub fn new(t: NaiveDateTime) -> Self { Self(Mutex::new(t)) }
    pub fn advance(&self, dur: chrono::Duration) {
        let mut g = self.0.lock().unwrap();
        *g = *g + dur;
    }
}
#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> NaiveDateTime { *self.0.lock().unwrap() }
}
```
**Rule:** Production code accepts `Arc<dyn Clock>`. Tests inject `FakeClock` and call `.advance()`. NEVER `tokio::time::pause()` for billing logic — that affects tokio timers, not `Local::now()`.
**Cited by:** Story 1.4, Story 5.2.

---

## F4 — MockProvider (mockall) for SensorProvider
**Use:** Tests of the poller, registry, classifier — anything that consumes `dyn SensorProvider`.

```rust
use sidebar_sensor::MockSensorProvider;

fn mock_returning(readings: Vec<Reading>) -> MockSensorProvider {
    let mut m = MockSensorProvider::new();
    m.expect_read_all()
        .returning(move || readings.clone());
    m.expect_descriptor()
        .return_const(test_descriptor());
    m
}
```
**Rule:** Always set BOTH `read_all` AND `descriptor` expectations, even if the test only checks one — the trait contract requires both.
**Cited by:** Story 2.1, Story 7.1, Story 7.2.

---

## F5 — Mock LHM HTTP `/data.json` payload
**Use:** Any test of `sidebar-adapter-ohm`, `OhmSupervisor`, or tier probing.

```rust
#[test]
fn lhm_http_fixture_test() {
    let body = include_str!("../crates/sidebar-adapter-ohm/tests/fixtures/lhm_data.json");
    let client = MockHttpClient::returning(body);
    assert_eq!(OhmSupervisor::new(client, tempdir()).probe(17_127), ProviderTier::Full);
}
```
**Rule:** Unit tests never hit the network. Include refusal, timeout, non-LHM body,
loopback rejection, and redirect-regression cases; real Win11/UAC smoke remains
`#[ignore]`/manual.
**Cited by:** Story 3.6, Story 6.4.

---

## F6 — Idempotency harness for cold-start code
**Use:** Any `init()` / `migrate()` / `register()` function that may be called twice.

```rust
#[test]
fn init_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("x.db");
    let conn = Connection::open(&path).unwrap();
    schema::init(&conn).expect("first init");
    schema::init(&conn).expect("second init");  // MUST NOT error
    // Assert state identical to single init.
    let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 1);
}
```
**Rule:** Cold-start code MUST be safe to call N times. Every init-style function gets this test.
**Cited by:** Story 4.1, Story 0.4, Story 7.1.

---

## F7 — Property-based test (proptest) for arithmetic
**Use:** Pure functions with edge-case-heavy domains (billing, formatting, wraparound).

```rust
proptest! {
    #[test]
    fn cycle_end_invariants(
        d in 1u8..=28,
        year in 2020i32..=2100,
        month in 1u32..=12,
    ) {
        let start = CycleStartDay::Day(d);
        let end = cycle_end(start, year, month).unwrap();
        let len = (end - NaiveDate::from_ymd_opt(year, month, d).unwrap()).num_days();
        prop_assert!(len >= 27 && len <= 31, "len={}", len);  // T-25
    }
}
```
**Rule:** Always assert the documented invariant (T-25 for billing). Proptest finds the Feb 29 / year-boundary bugs.
**Cited by:** Story 1.4, Story 5.1.

---

## F8 — Snapshot test for GUI rendering
**Use:** egui panel tests via `egui_kittest`.

```rust
#[test]
fn metric_row_renders_ghz() {
    let mut harness = egui_kittest::Harness::new(|ui| {
        let reading = Reading { kind: CpuFrequency, value: 3.84e9, unit: Hertz, .. };
        metric_row::render(ui, &reading, &DisplayConfig::default());
    });
    insta::assert_snapshot!(harness);  // '.snap' file under tests/snapshots/
    harness.click("CPU");  // interactive assertions where needed
}
```
**Rule:** Snapshot files (`tests/snapshots/*.snap`) are committed; `insta` review required (`cargo insta accept`) — HITL gate per guardrails.md G19.
**Cited by:** Story 8.1–8.5.

---

## F9 — Criterion bench harness for NFR-1
**Use:** The `poll_cost` bench and per-adapter micro-benches.

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_sysinfo_adapter(c: &mut Criterion) {
    let mut group = c.benchmark_group("poll_cost");
    group.sample_size(50);  // Min samples for stable p95
    group.bench_function("sysinfo", |b| {
        b.iter(|| {
            let provider = SysinfoAdapter::new();
            criterion::black_box(provider.read_all());
        });
    });
    group.finish();
}
criterion_group!(benches, bench_sysinfo_adapter);
criterion_main!(benches);
```
**Rule:** Sample size ≥ 50 to suppress noise. Bench output parsed by `benches/parse_threshold.rs` which fails the build if any group's mean CPU% > `T-1 (0.5%)`.
**Cited by:** Story 10.1, every adapter's optional self-bench.

---

## F10 — Panic-catch test for poller resilience
**Use:** Verify the poller survives a panicking provider.

```rust
#[tokio::test]
async fn poller_survives_provider_panic() {
    let mut bad = MockSensorProvider::new();
    bad.expect_read_all()
        .returning(|| panic!("boom"));
    bad.expect_descriptor().return_const(test_descriptor());

    let mut good = MockSensorProvider::new();
    good.expect_read_all().returning(|| vec![test_reading()]);
    good.expect_descriptor().return_const(test_descriptor());

    let (tx, mut rx) = broadcast::channel(8);
    Poller::new(vec![Arc::new(bad), Arc::new(good)]).run_once(&tx).await;

    // Good provider's readings MUST still arrive despite the bad one's panic.
    let received = rx.recv().await.expect("at least one message");
    assert!(received.iter().any(|r| r.kind == MetricKind::CpuFrequency));
}
```
**Rule:** The poller MUST `catch_unwind` per provider call. A panicking adapter MUST NOT poison the runtime.
**Cited by:** Story 7.2, guardrails.md G15.

---

## F11 — `unsafe` FFI test with SAFETY contract
**Use:** Every test exercising `unsafe` Win32 calls (`GetIfTable2`/`FreeMibTable`, `ShellExecuteW`, `DwmSetWindowAttribute`, `SetWindowDisplayAffinity`, `SHAppBarMessage`).

```rust
#[cfg(target_os = "windows")]
#[test]
fn getiftable2_returns_live_adapter() {
    // SAFETY: the API allocates a table on success; the pointer is checked and
    // released exactly once with the documented FreeMibTable deallocator.
    let mut table = std::ptr::null_mut();
    let r = unsafe { windows::Win32::NetworkManagement::IpHelper::GetIfTable2(&mut table) };
    assert_eq!(r, ERROR_SUCCESS);
    assert!(!table.is_null());
    unsafe { windows::Win32::NetworkManagement::IpHelper::FreeMibTable(table.cast()) };
}
```
**Rule:** Every `unsafe` block has a `// SAFETY:` comment justifying the invariant. Tests verify the documented contract. See guardrails.md G2.
**Cited by:** Story 3.5, Story 6.1–6.4.

---

## F12 — Event channel harness for tier/theme/monitor broadcasts
**Use:** Tests of components that subscribe to the `Event` channel (tier change, theme change, monitor change).

```rust
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    TierChanged(Tier),
    ThemeChanged(Theme),
    MonitorChanged(MonitorId),
    Shutdown,
}

#[tokio::test]
async fn tier_change_flips_status_pill() {
    let (tx, rx) = broadcast::channel::<Event>(8);
    let mut rx = rx;
    // ... wire GUI with rx ...
    tx.send(Event::TierChanged(Tier::Full)).unwrap();
    // GUI drains rx on next repaint; assert pill shows FULL
}
```
**Rule:** All non-sensor UI-affecting notifications flow through the `Event` channel (NOT the readings broadcast). Tests use this fixture to inject events without real OS state changes. See T-38 for coalescing semantics.
**Cited by:** Story 6.4, Story 7.4, Story 8.1, Story 8.2.

---

## F13 — Graceful shutdown harness with timeout hierarchy
**Use:** Tests of the shutdown sequence (poller cancel → accountant flush → OHM teardown → runtime drop).

```rust
use tokio_util::sync::CancellationToken;

#[tokio::test(start_paused = true)]
async fn shutdown_force_flushes_within_t19_budget() {
    let cancel = CancellationToken::new();
    let accountant = spawn_accountant(cancel.clone(), /* ... */);
    // ... accumulate some data without flushing ...
    let start = Instant::now();
    cancel.cancel();  // shutdown signal
    accountant.await.expect("accountant task panics");
    let elapsed = start.elapsed();
    // T-15 force-flush window
    assert!(elapsed.as_millis() < 500, "force-flush took {:?}", elapsed);
    // Assert DB has the flushed data
    assert_db_has_accumulated_data();
}
```
**Rule:** Shutdown tests use `tokio::time::pause()` + `start_paused = true` so wall-clock isn't consumed. The `Instant`-based assertions verify the T-39 hierarchy (500ms flush, 2000ms OHM teardown, 3000ms forced exit). See `nfr-thresholds.md` T-39.
**Cited by:** Story 7.5, Story 5.2.

---

## F14 — Regression harness triple-layer test
**Use:** Every story's tests include at least one assertion that exercises the FULL L0→L1→L2 chain to prove no regression in prior stories' layers. Used by the swarm to validate "definition of done" before opening a PR.

```rust
// L0 — pure unit (always runs)
#[test]
fn story_1_3_format_hz_ghz_unit() {
    assert_eq!(format_hz(3_840_000_000), "3.84 GHz");
}

// L1 — integration (Windows-only, depends on Story 3.x having merged)
#[cfg(target_os = "windows")]
#[test]
fn story_3_5_getiftable2_smoke_integration() {
    // Verifies that the adapter (Story 3.5) STILL works after this story's changes.
    let adapter = NetAdapter::new();
    let readings = adapter.read_all();
    assert!(!readings.is_empty(), "Story 3.5 regression: net adapter empty");
}

// L2 — UI snapshot (depends on Story 8.x having merged)
#[test]
fn story_8_3_metric_row_render_ui() {
    let mut harness = egui_kittest::Harness::new(|ui| {
        metric_row::render(ui, &test_reading(), &DisplayConfig::default());
    });
    insta::assert_snapshot!(harness);
}
```
**Rule:** Each story's PR MUST include at least one test at its declared Layer (per the Wiring block), AND at least one regression-tiebreaker test that re-runs a prior story's behavior at the same layer. The CI matrix (Story 11.2) enforces this by running ALL tests at ALL layers on every PR — there is no "only my crate" mode.
**Cited by:** every story via the `Layer:` field in its Wiring block; G25/G26/G27; `regression-harness.md` §2.
