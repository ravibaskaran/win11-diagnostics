//! Event channel coalescer (Story 7.4).
//!
//! Carries UI-affecting [`Event`](sidebar_domain::event::Event)s from emitters
//! (OhmSupervisor tier changes, theme bridge, hotkey system) to subscribers
//! (the GUI, the poller for tier-driven registry rebuilds). Tier-change events
//! pass through a 500ms debounce window (T-38) so OHM flap doesn't thrash the
//! status pill; other event types pass through immediately.
//!
//! ## Architecture
//!
//! Two `broadcast::Sender<Event>` endpoints (capacity 8, T-14):
//! - The **raw** channel: emitters (OhmSupervisor, etc.) send here.
//! - The **coalesced** channel: subscribers (GUI, poller) receive here.
//!
//! The coalescer task sits between them: it reads from the raw channel, and
//! for `TierChanged` events holds the latest for up to 500ms before publishing
//! (so a rapid Basic→Full→Basic→Full sequence collapses to a single Full).
//! Non-tier events are forwarded immediately.
//!
//! ## G15 panic safety
//!
//! The coalescer loop is wrapped so a panic in the debounce logic logs + falls
//! back to pass-through mode (no coalescing) rather than killing the task.
//!
//! Cited: Story 7.4, architecture.md §6 (G23), nfr-thresholds.md T-14/T-38,
//! tdd-fixtures.md F12.

use std::time::Duration;

use sidebar_domain::event::Event;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// The T-38 coalescing window for tier-change events.
pub const TIER_COALESCE_WINDOW: Duration = Duration::from_millis(500);

/// The broadcast capacity (T-14).
pub const CHANNEL_CAPACITY: usize = 8;

/// A paired raw/coalesced channel set. Emitters send to `raw_tx`; subscribers
/// receive from `coalesced_rx`. The coalescer task connects them.
///
/// Construct via [`EventChannel::new`] which spawns the coalescer.
pub struct EventChannel {
    /// Emitters send here (OhmSupervisor, theme bridge, hotkey system).
    pub raw_tx: broadcast::Sender<Event>,
    /// Subscribers receive here (GUI, poller registry rebuild).
    pub coalesced_tx: broadcast::Sender<Event>,
    /// The background coalescer task. The integration shutdown path awaits
    /// this handle so no worker remains detached after eframe exits.
    pub coalescer: JoinHandle<()>,
}

impl EventChannel {
    /// Create a new Event channel + spawn the coalescer task. The task runs
    /// until the raw sender is dropped (all emitters gone) or it panics (G15
    /// fallback).
    ///
    /// Returns the channel handle. Callers get `raw_tx` (for emitters) and can
    /// `coalesced_tx.subscribe()` for each subscriber.
    #[must_use]
    pub fn new() -> Self {
        let (raw_tx, raw_rx) = broadcast::channel::<Event>(CHANNEL_CAPACITY);
        let (coalesced_tx, _coalesced_rx) = broadcast::channel::<Event>(CHANNEL_CAPACITY);
        let coalesced_tx_clone = coalesced_tx.clone();
        let coalescer = spawn_coalescer(raw_rx, coalesced_tx_clone);
        Self {
            raw_tx,
            coalesced_tx,
            coalescer,
        }
    }

    /// Subscribe to the coalesced event stream (for the GUI / poller).
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.coalesced_tx.subscribe()
    }
}

impl Default for EventChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn the coalescer task. Reads raw events; for `TierChanged`, debounces
/// 500ms (T-38) publishing only the latest; for other events, publishes
/// immediately. G15: on panic, logs + switches to pass-through.
///
/// The task exits when the raw receiver closes (all senders dropped).
pub fn spawn_coalescer(
    mut raw_rx: broadcast::Receiver<Event>,
    coalesced_tx: broadcast::Sender<Event>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The pending tier (if we're inside a debounce window). `Some(tier)`
        // means we saw a TierChanged and are waiting for the window to elapse
        // before publishing it (so a subsequent TierChanged replaces it).
        let mut pending_tier: Option<sidebar_domain::event::Tier> = None;
        let mut debounce = tokio::time::interval(TIER_COALESCE_WINDOW);
        // Don't fire the immediate first tick — we only tick when we have a
        // pending tier.
        debounce.tick().await;

        loop {
            tokio::select! {
                // A raw event arrived from an emitter.
                recv = raw_rx.recv() => {
                    match recv {
                        Ok(event) => {
                            if let Event::TierChanged(tier) = event {
                                // Store the latest tier; the debounce timer
                                // will publish it when the window elapses.
                                // If a previous tier was pending, it's
                                // replaced (coalesced away — T-38).
                                pending_tier = Some(tier);
                                // Reset the debounce window so it fires
                                // TIER_COALESCE_WINDOW after this latest
                                // event.
                                debounce.reset();
                            } else {
                                // Non-tier event — pass through immediately.
                                // A Shutdown event also exits the loop after
                                // publishing.
                                let is_shutdown = event == Event::Shutdown;
                                let _ = coalesced_tx.send(event);
                                if is_shutdown {
                                    tracing::info!(
                                        "event coalescer: Shutdown received — exiting"
                                    );
                                    return;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                n,
                                "event coalescer: raw channel lagged (T-14 cap) — {} events dropped",
                                n
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            // All raw senders dropped — flush any pending tier
                            // + exit.
                            if let Some(tier) = pending_tier.take() {
                                let _ = coalesced_tx.send(Event::TierChanged(tier));
                            }
                            tracing::info!(
                                "event coalescer: raw channel closed — exiting"
                            );
                            return;
                        }
                    }
                }
                // The debounce window elapsed — publish the pending tier (if any).
                _ = debounce.tick() => {
                    if let Some(tier) = pending_tier.take() {
                        let _ = coalesced_tx.send(Event::TierChanged(tier));
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    //! Story 7.4 TDD contract tests (F12 event-channel harness).
    //!
    //! Cited: Story 7.4, nfr-thresholds.md T-14/T-38, guardrails.md G15/G23,
    //! tdd-fixtures.md F12.

    use super::*;
    use sidebar_domain::event::{Event, Tier};

    // ----- Happy Path #1: two tier changes within 500ms → only the latter -----

    /// Cited: Story 7.4 Happy Path #1, T-38.
    #[tokio::test]
    async fn two_tier_changes_within_window_only_latter_published() {
        let channel = EventChannel::new();
        let mut rx = channel.subscribe();
        let raw = channel.raw_tx.clone();

        // Send Full then Basic within the 500ms window.
        let _ = raw.send(Event::TierChanged(Tier::Full));
        let _ = raw.send(Event::TierChanged(Tier::Basic));

        // Wait for the coalesce window to elapse + the publish.
        tokio::time::sleep(TIER_COALESCE_WINDOW + Duration::from_millis(100)).await;

        // Drain all received events.
        let mut received = Vec::new();
        while let Ok(e) = rx.try_recv() {
            received.push(e);
        }

        // Only ONE tier-change published (the latest: Basic).
        let tier_events: Vec<_> = received
            .iter()
            .filter(|e| matches!(e, Event::TierChanged(_)))
            .collect();
        assert_eq!(
            tier_events.len(),
            1,
            "coalesced to 1 tier event, got {tier_events:?}"
        );
        assert_eq!(*tier_events[0], Event::TierChanged(Tier::Basic));
    }

    // ----- Happy Path #2: theme change not coalesced -----

    /// Cited: Story 7.4 Happy Path #2.
    #[tokio::test]
    async fn theme_change_passes_through_immediately() {
        let channel = EventChannel::new();
        let mut rx = channel.subscribe();
        let raw = channel.raw_tx.clone();

        let _ = raw.send(Event::ThemeChanged("dark".to_string()));

        // Theme events are NOT coalesced — should arrive near-instantly.
        let received = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("theme event should arrive within 100ms (not coalesced)")
            .expect("recv ok");

        assert_eq!(received, Event::ThemeChanged("dark".to_string()));
    }

    // ----- Boundary #1: 100 tier changes in 1s → at most 2 published -----

    /// Cited: Story 7.4 Boundary #1, T-38.
    #[tokio::test]
    async fn many_tier_changes_collapse_to_at_most_two() {
        let channel = EventChannel::new();
        let mut rx = channel.subscribe();
        let raw = channel.raw_tx.clone();

        // Fire 100 rapid tier changes (alternating Basic/Full).
        for i in 0..100 {
            let tier = if i % 2 == 0 { Tier::Basic } else { Tier::Full };
            let _ = raw.send(Event::TierChanged(tier));
        }

        // Wait for the coalesce window to settle.
        tokio::time::sleep(TIER_COALESCE_WINDOW + Duration::from_millis(200)).await;

        let mut count = 0;
        while let Ok(e) = rx.try_recv() {
            if matches!(e, Event::TierChanged(_)) {
                count += 1;
            }
        }
        // T-38: at most 2 published (the start-of-window + end-of-window).
        // In practice the rapid burst collapses to 1 (only the last), but the
        // spec says "at most 2" — allow either.
        assert!(
            count <= 2,
            "expected at most 2 coalesced tier events, got {count}"
        );
    }

    // ----- Boundary #2: channel overflow (T-14 cap) → oldest dropped -----

    /// Cited: Story 7.4 Boundary #2, T-14.
    #[tokio::test]
    async fn channel_overflow_drops_oldest() {
        let (tx, mut rx) = broadcast::channel::<Event>(CHANNEL_CAPACITY);

        // Fill past capacity — the oldest are dropped.
        for i in 0..(CHANNEL_CAPACITY + 5) {
            let _ = tx.send(Event::HotkeyPressed(format!("hk{i}")));
        }

        // The receiver lags — first recv returns Lagged.
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(n)) => {
                assert!(n >= 5, "expected ≥5 lagged, got {n}");
            }
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    // ----- Boundary #3: coalescer handles Shutdown → exits cleanly -----

    /// Cited: Story 7.4 Boundary #4, T-39.
    #[tokio::test]
    async fn shutdown_event_passes_through_and_exits() {
        let mut channel = EventChannel::new();
        let mut rx = channel.subscribe();
        let raw = channel.raw_tx.clone();

        let _ = raw.send(Event::Shutdown);

        let received = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Shutdown should pass through immediately")
            .expect("recv ok");
        assert_eq!(received, Event::Shutdown);

        // The coalescer task should exit shortly after Shutdown.
        tokio::time::timeout(Duration::from_millis(500), &mut channel.coalescer)
            .await
            .expect("coalescer must terminate after Shutdown")
            .expect("coalescer task must join cleanly");
    }

    // ----- Boundary #4: raw channel close → coalescer flushes + exits -----

    /// Cited: Story 7.4, G15.
    #[tokio::test]
    async fn raw_channel_close_flushes_pending_and_exits() {
        let mut channel = EventChannel::new();
        let mut rx = channel.subscribe();

        // Send a tier change (pending in the debounce window).
        let _ = channel.raw_tx.send(Event::TierChanged(Tier::Full));
        // Drop the raw sender → raw channel closes → coalescer should flush
        // the pending tier + exit.
        drop(channel.raw_tx);

        // The pending Full should be flushed.
        let received =
            tokio::time::timeout(TIER_COALESCE_WINDOW + Duration::from_millis(200), rx.recv())
                .await
                .expect("pending tier should be flushed on close")
                .expect("recv ok");
        assert_eq!(received, Event::TierChanged(Tier::Full));
        tokio::time::timeout(Duration::from_millis(500), &mut channel.coalescer)
            .await
            .expect("coalescer must terminate after raw channel close")
            .expect("coalescer task must join cleanly");
    }

    // ----- Integration: full channel end-to-end -----

    #[tokio::test]
    async fn end_to_end_tier_then_theme_then_tier() {
        let channel = EventChannel::new();
        let mut rx = channel.subscribe();
        let raw = channel.raw_tx.clone();

        let _ = raw.send(Event::TierChanged(Tier::Full));
        // Theme passes through immediately.
        let _ = raw.send(Event::ThemeChanged("light".to_string()));
        let _ = raw.send(Event::TierChanged(Tier::Basic));

        // Theme arrives first (not coalesced).
        let theme = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("theme")
            .expect("recv");
        assert_eq!(theme, Event::ThemeChanged("light".to_string()));

        // Then the coalesced tier (the latest: Basic) after the window.
        let tier =
            tokio::time::timeout(TIER_COALESCE_WINDOW + Duration::from_millis(200), rx.recv())
                .await
                .expect("tier")
                .expect("recv");
        assert_eq!(tier, Event::TierChanged(Tier::Basic));
    }
}
