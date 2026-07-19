//! `sidebar-app` library facade.
//!
//! Story 0.2 adds the `parse_threshold` module here (rather than in a new
//! crate) so it can be unit-tested without inflating the workspace package
//! count past the G17 cap of 12. sidebar-app is now a mixed lib+bin crate
//! (Cargo supports this natively).
//!
//! Future stories add GUI/poller wiring modules here.

pub mod event_channel;
pub mod gui;
pub mod i18n;
pub mod nfr;
pub mod parse_threshold;
pub mod poller;
pub mod provider_registry;
pub mod shutdown;
pub mod tier_probe;
