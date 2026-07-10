//! Story 8.10 — First-Run Wizard (HITL — first impression).
//!
//! A modal egui panel collecting the v1 first-run settings on launch:
//!
//! 1. Docked edge (left/right/top/bottom) — default right.
//! 2. Target monitor — default primary (T-37).
//! 3. Billing-cycle start day — default 1 (T-26, T-37).
//! 4. Theme — default Dark (T-35, T-37).
//!
//! ## Trigger
//!
//! [`should_show`] returns true when `config.first_run_complete != true`. The
//! host (main.rs) gates the poller start on this flag (G24 — poller must NOT
//! start while the wizard is showing) and renders the wizard instead of the
//! live sidebar until the user completes or skips.
//!
//! ## Completion semantics
//!
//! - **Continue** → the wizard collects the chosen values, sets
//!   `first_run_complete = true`, and the host persists config.toml. Returns
//!   [`WizardAction::Continue`].
//! - **Skip** → defaults are applied (no user edits beyond the wizard defaults),
//!   `first_run_complete = true`, host persists. Returns [`WizardAction::Skip`].
//! - **Pending** → the wizard is still showing (user hasn't clicked either
//!   button). Returns [`WizardAction::Pending`].
//!
//! ## Boundary cases
//!
//! - Existing config with `first_run_complete = true` → wizard does NOT render
//!   (`should_show` returns false).
//! - Write fails (read-only FS) → the host surfaces an error + retry (the
//!   wizard itself is stateless; the host owns the IO). The wizard's contract
//!   is that `Continue`/`Skip` fire `on_complete` exactly once, which the host
//!   uses to trigger the persist + gate the poller.
//! - Window-X (close) → treated as Skip (the host's close handler calls Skip
//!   semantics — defaults + first_run_complete=true — so the wizard doesn't
//!   re-block on next launch).
//!
//! ## Cited
//!
//! - Story 8.10 TDD contract (F1: absent config → wizard renders; F8: complete
//!   → config written + first_run_complete=true; skip → defaults + flag;
//!   boundary: existing-complete → no wizard; poller gated G24).
//! - nfr-thresholds.md T-37 (first-run required fields), T-26 (cycle day 1–28).
//! - guardrails.md G24 (poller does not start while wizard shows), G11/G19
//!   (HITL — first impression).
//! - sidebar-domain::config::Config::first_run_complete (Story 1.5 — already a
//!   field, default false).

use eframe::egui::Ui;
use sidebar_domain::config::{Config, CycleStartDaySerde};

/// The action the wizard signals back to the host on a given frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WizardAction {
    /// The wizard is still showing (neither Continue nor Skip clicked this
    /// frame). The host should keep rendering the wizard and NOT start the
    /// poller.
    #[default]
    Pending,
    /// The user clicked "Continue". The host should persist config (with
    /// `first_run_complete = true`) and start the poller.
    Continue,
    /// The user clicked "Skip". The host should persist config with defaults
    /// applied + `first_run_complete = true` and start the poller.
    Skip,
}

/// The minimum cycle-start day (T-26 — Day must be in `[1, 28]`).
pub(crate) const MIN_CYCLE_DAY: u8 = 1;
/// The maximum cycle-start day (T-26 — Day must be in `[1, 28]`).
pub(crate) const MAX_CYCLE_DAY: u8 = 28;

/// Whether the wizard should show for the given config. Returns true when
/// `first_run_complete != true` (covers both "absent config" — which parses to
/// the default `first_run_complete = false` — and an explicit incomplete flag).
#[must_use]
pub fn should_show(config: &Config) -> bool {
    !config.first_run_complete
}

/// Render the first-run wizard into `ui`, editing `config` in place for the
/// duration of the wizard. Returns the [`WizardAction`] the host should act on
/// this frame.
///
/// The host is responsible for:
/// 1. Calling [`should_show`] before rendering the wizard (and rendering the
///    live sidebar instead when it returns false).
/// 2. Persisting `config` (with `first_run_complete = true`) when the action
///    is Continue or Skip.
/// 3. NOT starting the poller while the action is Pending (G24).
///
/// ## Fields collected (T-37)
///
/// - Docked edge (left/right/top/bottom) — checkbox group, default right.
/// - Target monitor — text input, default "primary".
/// - Billing-cycle start day — slider 1–28 (T-26).
/// - Theme — checkbox group (Dark/Light/System), default Dark.
pub fn render_wizard(ui: &mut Ui, config: &mut Config) -> WizardAction {
    let mut action = WizardAction::Pending;

    ui.vertical_centered(|w| {
        w.heading("Welcome to sidebar");
        w.label(
            egui::RichText::new("Let's set up your first-run preferences.")
                .small()
                .weak(),
        );
        w.separator();

        // ---- Docked edge (T-37, T-36) ----
        w.label("Docked edge");
        w.horizontal(|row| {
            for &edge in &["Left", "Right", "Top", "Bottom"] {
                let mut selected = config.dock.edge == edge;
                if row.checkbox(&mut selected, edge).changed() && selected {
                    config.dock.edge = edge.to_string();
                }
            }
        });

        // ---- Target monitor (T-37, T-36) ----
        w.label("Target monitor");
        w.horizontal(|row| {
            let mut mon = config.dock.monitor_id.clone();
            let te = row.add(
                egui::TextEdit::singleline(&mut mon)
                    .desired_width(140.0)
                    .hint_text("primary"),
            );
            if te.changed() {
                config.dock.monitor_id = mon;
            }
            // Default affordance — clicking sets "primary".
            if row.button("Use primary").clicked() {
                config.dock.monitor_id = "primary".to_string();
            }
        });

        // ---- Billing-cycle start day (T-37, T-26: 1–28) ----
        w.label("Billing cycle start day");
        w.horizontal(|row| {
            let mut day_value: u8 = match &config.bandwidth.cycle_start_day {
                CycleStartDaySerde::Day(d) => *d,
                CycleStartDaySerde::LastDayOfMonth => MAX_CYCLE_DAY,
            };
            let slider =
                row.add(egui::Slider::new(&mut day_value, MIN_CYCLE_DAY..=MAX_CYCLE_DAY).text(""));
            if slider.changed() {
                config.bandwidth.cycle_start_day = CycleStartDaySerde::Day(day_value);
            }
            let mut last_day = matches!(
                config.bandwidth.cycle_start_day,
                CycleStartDaySerde::LastDayOfMonth
            );
            if row.checkbox(&mut last_day, "Last day").changed() {
                config.bandwidth.cycle_start_day = if last_day {
                    CycleStartDaySerde::LastDayOfMonth
                } else {
                    CycleStartDaySerde::Day(MIN_CYCLE_DAY)
                };
            }
        });

        // ---- Theme (T-37, T-35) ----
        w.label("Theme");
        w.horizontal(|row| {
            for &mode in &["Dark", "Light", "System"] {
                let mut selected = config.theme.mode == mode;
                if row.checkbox(&mut selected, mode).changed() && selected {
                    config.theme.mode = mode.to_string();
                }
            }
        });

        w.separator();

        // ---- Action buttons ----
        w.horizontal(|btns| {
            if btns.button("Skip").clicked() {
                // Restore defaults (T-37 skip semantics). We overwrite the
                // user-facing fields but preserve nothing else — the wizard
                // is the first thing the user sees, so "skip" = "I'll take the
                // defaults, get me in".
                *config = Config::default();
                config.first_run_complete = true;
                action = WizardAction::Skip;
            }
            if btns.button("Continue").clicked() {
                config.first_run_complete = true;
                action = WizardAction::Continue;
            }
        });
    });

    action
}

#[cfg(test)]
mod tests {
    //! Story 8.10 TDD contract tests.
    //!
    //! F8 (egui_kittest) for the wizard render + F1 pure-fn for the config
    //! write semantics. Tests that need to read config after a click use a
    //! shared `Arc<Mutex<Config>>` so the harness closure borrows the Arc
    //! clone (not the config directly) — same pattern as the settings_panel
    //! click tests.

    use super::*;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use sidebar_domain::config::{Config, CycleStartDaySerde};
    use std::sync::{Arc, Mutex};

    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== F1: should_show returns true for absent/incomplete config =====

    #[test]
    fn should_show_true_when_first_run_incomplete() {
        let config = Config::default();
        assert!(!config.first_run_complete, "default config is incomplete");
        assert!(
            should_show(&config),
            "wizard must show when first_run_complete=false"
        );
    }

    #[test]
    fn should_show_false_when_first_run_complete() {
        let config = Config {
            first_run_complete: true,
            ..Config::default()
        };
        assert!(
            !should_show(&config),
            "wizard must NOT show when first_run_complete=true"
        );
    }

    // ===== F8: wizard renders the four required fields (T-37) =====

    #[test]
    fn wizard_renders_all_required_fields() {
        let mut config = Config::default();
        let mut harness = Harness::new_ui(|ui| {
            render_wizard(ui, &mut config);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Docked edge") || labels.contains("docked edge"),
            "wizard must collect docked edge (T-37) (got: {labels})"
        );
        assert!(
            labels.contains("Monitor") || labels.contains("monitor"),
            "wizard must collect target monitor (T-37) (got: {labels})"
        );
        assert!(
            labels.contains("Billing cycle") || labels.contains("cycle"),
            "wizard must collect billing-cycle start day (T-37/T-26) (got: {labels})"
        );
        assert!(
            labels.contains("Theme") || labels.contains("theme"),
            "wizard must collect theme (T-37) (got: {labels})"
        );
        assert!(
            labels.contains("Continue"),
            "wizard must offer a Continue button (got: {labels})"
        );
        assert!(
            labels.contains("Skip"),
            "wizard must offer a Skip button (got: {labels})"
        );
    }

    // ===== F8: Continue → first_run_complete=true, config preserved =====

    #[test]
    fn continue_sets_first_run_complete_true() {
        let config = Arc::new(Mutex::new(Config::default()));
        config.lock().unwrap().dock.edge = "Left".to_string();
        config.lock().unwrap().theme.mode = "Light".to_string();
        config.lock().unwrap().bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);

        let c = config.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let mut guard = c.lock().unwrap();
            render_wizard(ui, &mut guard);
        });
        harness.run();
        harness.get_by_label("Continue").click();
        harness.run();
        let after = config.lock().unwrap();
        assert!(
            after.first_run_complete,
            "Continue must set first_run_complete=true (got: {:?})",
            *after
        );
        // User-edited values must survive (not overwritten by defaults).
        assert_eq!(after.dock.edge, "Left");
        assert_eq!(after.theme.mode, "Light");
    }

    // ===== F8: Skip → defaults applied + first_run_complete=true =====

    #[test]
    fn skip_applies_defaults_and_sets_first_run_complete() {
        let config = Arc::new(Mutex::new(Config::default()));
        let c = config.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let mut guard = c.lock().unwrap();
            render_wizard(ui, &mut guard);
        });
        harness.run();
        harness.get_by_label("Skip").click();
        harness.run();
        let after = config.lock().unwrap();
        assert!(
            after.first_run_complete,
            "Skip must set first_run_complete=true (got: {:?})",
            *after
        );
        // Defaults must be present (T-37).
        assert_eq!(
            after.dock.edge, "Right",
            "skip → docked edge defaults to Right"
        );
        assert_eq!(
            after.dock.monitor_id, "primary",
            "skip → monitor defaults to primary"
        );
        assert_eq!(
            after.bandwidth.cycle_start_day,
            CycleStartDaySerde::Day(1),
            "skip → cycle start day defaults to Day(1)"
        );
        assert_eq!(after.theme.mode, "Dark", "skip → theme defaults to Dark");
    }

    // ===== Boundary: Continue returns Continue action (not Pending) =====
    //
    // kittest's `run()` loops multiple internal steps until the UI settles
    // (see egui_kittest `_try_run`). The `clicked()` flag is true on exactly
    // one step (the release frame), then false again. So we record the PEAK
    // action across steps — once Continue/Skip is observed, it wins over later
    // Pending steps (the production host reads the action per-frame and acts
    // immediately on Continue/Skip, so the ephemeral flag is correct in prod).

    fn record_action(slot: &Arc<Mutex<WizardAction>>, act: WizardAction) {
        // Pending never overwrites a Continue/Skip already recorded.
        let mut current = slot.lock().unwrap();
        if *current == WizardAction::Pending {
            *current = act;
        }
    }

    #[test]
    fn continue_returns_continue_action() {
        let config = Arc::new(Mutex::new(Config::default()));
        let action = Arc::new(Mutex::new(WizardAction::Pending));
        let c = config.clone();
        let a = action.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let mut guard = c.lock().unwrap();
            let act = render_wizard(ui, &mut guard);
            record_action(&a, act);
        });
        harness.run();
        harness.get_by_label("Continue").click();
        harness.run();
        assert_eq!(
            *action.lock().unwrap(),
            WizardAction::Continue,
            "Continue click must return WizardAction::Continue"
        );
    }

    // ===== Boundary: Skip returns Skip action =====

    #[test]
    fn skip_returns_skip_action() {
        let config = Arc::new(Mutex::new(Config::default()));
        let action = Arc::new(Mutex::new(WizardAction::Pending));
        let c = config.clone();
        let a = action.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let mut guard = c.lock().unwrap();
            let act = render_wizard(ui, &mut guard);
            record_action(&a, act);
        });
        harness.run();
        harness.get_by_label("Skip").click();
        harness.run();
        assert_eq!(
            *action.lock().unwrap(),
            WizardAction::Skip,
            "Skip click must return WizardAction::Skip"
        );
    }

    // ===== Boundary: pre-click → Pending =====

    #[test]
    fn pre_click_returns_pending() {
        let config = Arc::new(Mutex::new(Config::default()));
        // Direct assignment (not record_action): with no click, every render
        // returns Pending, so the final value is Pending regardless.
        let action = Arc::new(Mutex::new(WizardAction::Pending));
        let c = config.clone();
        let a = action.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let mut guard = c.lock().unwrap();
            let act = render_wizard(ui, &mut guard);
            *a.lock().unwrap() = act;
        });
        harness.run();
        assert_eq!(
            *action.lock().unwrap(),
            WizardAction::Pending,
            "wizard with no click must return Pending"
        );
    }

    // ===== Boundary: cycle_start_day slider constrains to [1, 28] (T-26) =====

    #[test]
    fn wizard_cycle_day_constrained_to_1_28() {
        let mut config = Config::default();
        let mut harness = Harness::new_ui(|ui| {
            render_wizard(ui, &mut config);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("cycle") || labels.contains("Billing"),
            "wizard must surface the billing-cycle section (T-26) (got: {labels})"
        );
        // The shared constraint bounds are the same as settings_panel.
        assert_eq!(MIN_CYCLE_DAY, 1);
        assert_eq!(MAX_CYCLE_DAY, 28);
    }

    // ===== F1: complete → config.toml round-trip preserves first_run_complete =====

    #[test]
    fn first_run_complete_persists_through_toml_round_trip() {
        let config = Config {
            first_run_complete: true,
            dock: sidebar_domain::config::DockConfig {
                edge: "Left".to_string(),
                ..sidebar_domain::config::DockConfig::default()
            },
            ..Config::default()
        };
        let toml_str = config.to_toml_string().expect("toml serialize");
        let parsed = Config::from_toml_str(&toml_str).expect("toml parse");
        assert!(
            parsed.first_run_complete,
            "first_run_complete=true must survive TOML round-trip"
        );
        assert_eq!(
            parsed.dock.edge, "Left",
            "dock.edge must survive round-trip"
        );
        // And should_show must now return false.
        assert!(
            !should_show(&parsed),
            "after persisting first_run_complete=true, should_show must be false"
        );
    }

    // ===== Boundary: complete config file parses → wizard NOT shown =====

    #[test]
    fn existing_complete_config_skips_wizard() {
        let toml_str = "first_run_complete = true\npoll_interval_seconds = 5";
        let config = Config::from_toml_str(toml_str).expect("parse");
        assert!(config.first_run_complete);
        assert!(
            !should_show(&config),
            "existing complete config must skip the wizard"
        );
    }
}
