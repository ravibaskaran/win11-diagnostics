//! `Event` — UI-affecting notifications carried on the Event broadcast channel
//! (Story 7.4).
//!
//! The Event channel is SEPARATE from the readings broadcast
//! (`broadcast::Sender<Vec<Reading>>`, Story 7.2). It carries discrete
//! notifications that the GUI/poller/accountant must react to: tier changes
//! (OHM launched/crashed), theme changes (system dark/light flipped), monitor
//! changes (dock target unplugged), hotkey presses, and shutdown.
//!
//! ## Tier-change coalescing (T-38)
//!
//! `Event::TierChanged` events pass through a 500ms coalescer (Story 7.4's
//! `event_channel::spawn_coalescer`) so OHM flap doesn't thrash the status
//! pill. Only the latest tier within the window is published to subscribers.
//! Other event types pass through immediately (no coalescing).
//!
//! Cited: Story 7.4, architecture.md §6 (G23 Event channel discipline),
//! nfr-thresholds.md T-14 (cap 8) / T-38 (500ms coalesce).

/// The runtime tier — a local mirror of `sidebar_sensor::ProviderTier` so the
/// pure-domain layer (AD-4) does NOT depend on sidebar-sensor. The app layer
/// maps between this and `ProviderTier` at the channel boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Basic mode — no admin, no LHM.
    Basic,
    /// Full mode — LHM subprocess running.
    Full,
}

/// UI-affecting events carried on the Event broadcast channel.
///
/// Variant ordering is NOT significant — subscribers match exhaustively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The runtime tier changed (OHM launched → Full, or OHM crashed → Basic).
    /// Coalesced 500ms by the event_channel (T-38).
    TierChanged(Tier),
    /// The system theme changed (dark/light). Not coalesced.
    /// The string is the theme mode: "dark", "light", or "system".
    ThemeChanged(String),
    /// The dock-target monitor changed (unplugged/replugged). Not coalesced.
    /// The string is the monitor device ID.
    MonitorChanged(String),
    /// A global hotkey was pressed. Not coalesced.
    /// The string is the hotkey action name (e.g. "click_through").
    HotkeyPressed(String),
    /// Shutdown requested. Subscribers should drain within their T-39 phase.
    /// Published when Ctrl+C / WM_CLOSE / any component requests shutdown.
    Shutdown,
}

impl Event {
    /// Returns `true` if this event is a `TierChanged` (the coalesced variant).
    /// Used by the coalescer to decide whether to debounce or pass through.
    #[must_use]
    pub fn is_tier_change(&self) -> bool {
        matches!(self, Self::TierChanged(_))
    }
}

#[cfg(test)]
mod tests {
    //! Story 7.4 domain-layer tests for the Event enum + Tier type.

    use super::*;

    /// Cited: Story 7.4.
    #[test]
    fn tier_basic_full_equality() {
        assert_ne!(Tier::Basic, Tier::Full);
        assert_eq!(Tier::Basic, Tier::Basic);
        assert_eq!(Tier::Full, Tier::Full);
    }

    /// Cited: Story 7.4.
    #[test]
    fn event_is_tier_change_discriminates() {
        assert!(Event::TierChanged(Tier::Full).is_tier_change());
        assert!(Event::TierChanged(Tier::Basic).is_tier_change());
        assert!(!Event::ThemeChanged("dark".to_string()).is_tier_change());
        assert!(!Event::Shutdown.is_tier_change());
    }

    /// Cited: Story 7.4.
    #[test]
    fn event_equality_and_clone() {
        let e1 = Event::TierChanged(Tier::Full);
        let e2 = e1.clone();
        assert_eq!(e1, e2);
    }
}
