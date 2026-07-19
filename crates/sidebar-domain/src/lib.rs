//! `sidebar-domain` — Pure domain types and logic.
//!
//! The domain layer holds the canonical `Reading`, `SensorId`, `MetricKind`,
//! `Unit` types and pure functions (formatting, billing, alert, aggregation
//! hooks via `aggregate`/`smooth` were removed in v1.0 ponytail pass 2 —
//! dead code with zero production callers). It has ZERO OS dependencies and
//! ZERO I/O — that's the contract that makes strict TDD feasible for ~80%
//! of the codebase (architecture.md AD-4).
//!
//! Story 0.6 also places the shared `Error` enum here (rather than in a
//! separate crate) to preserve the G17 12-package workspace cap.

pub mod alert;
pub mod billing;
pub mod config;
pub mod error;
pub mod event;
pub mod format;
pub mod graph;
pub mod reading;
pub mod reposition;
