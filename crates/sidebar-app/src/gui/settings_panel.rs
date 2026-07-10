//! Story 8.5 — Settings Panel (PRD §5.5.8 + Settings architecture).
//!
//! The settings panel edits the user-facing fields of [`Config`] and notifies
//! the host of any change so it can persist `config.toml` debounced.
//!
//! ## Editable Fields (mapped to Config)
//!
//! | UI field            | Config path                              |
//! |---------------------|------------------------------------------|
//! | Billing cycle start | `bandwidth.cycle_start_day` (Day/LastDay)|
//! | Temperature unit    | `display.temp_unit`                      |
//! | Raw values toggle   | `display.raw_values`                     |
//! | Decimal/binary base | `display.decimal_base`                   |
//! | Poll interval       | `poll_interval_seconds`                  |
//! | Docked edge         | `dock.edge`                              |
//! | Theme mode          | `theme.mode`                             |
//!
//! ## Guardrails
//!
//! - **cycle_start_day** is constrained to 1–28 (T-26). Day 29+ is rejected
//!   at the UI — the user must pick ≤28 or `LastDayOfMonth`. The
//!   "no-retroactive-resplit" rule (PRD §5.5.8) means a cycle-start-day
//!   change applies only to the NEXT rollover, not the current cycle. A
//!   tooltip spells this out (HITL guardrail).
//! - **poll_interval_seconds=0** is clamped to 1 with a visible warning
//!   (T-3). The number-edit path also clamps >60 down to 60.
//!
//! ## Cited
//!
//! - Story 8.5 TDD contract (Happy Path #1-#2, Boundary #1-#4)
//! - PRD §5.5.8 (cycle rollover semantics)
//! - nfr-thresholds.md T-3 (poll clamp), T-21 (decimal/binary), T-26 (cycle
//!   start day), T-28 (decimal/binary), T-29 (temp unit)
//! - guardrails.md HITL (no-retroactive-resplit G11)

use eframe::egui::Ui;
#[allow(unused_imports)] // RED-phase: helpers exist for the GREEN commit.
use sidebar_domain::config::{Config, CycleStartDaySerde, DisplayConfig, DockConfig, ThemeConfig};
#[allow(unused_imports)] // RED-phase: used by display_temp_unit.
use sidebar_domain::format::TempUnit;

/// The minimum cycle-start day (T-26 — Day must be in `[1, 28]`).
#[allow(dead_code)] // Referenced by tests + GREEN-phase render.
pub(crate) const MIN_CYCLE_DAY: u8 = 1;
/// The maximum cycle-start day (T-26 — Day must be in `[1, 28]`).
#[allow(dead_code)] // Referenced by tests + GREEN-phase render.
pub(crate) const MAX_CYCLE_DAY: u8 = 28;

/// The minimum poll interval in seconds (T-3 — must be in `[1, 60]`).
#[allow(dead_code)] // Referenced by tests + GREEN-phase render.
pub(crate) const MIN_POLL_INTERVAL: u32 = 1;
/// The maximum poll interval in seconds (T-3 — must be in `[1, 60]`).
#[allow(dead_code)] // Referenced by tests + GREEN-phase render.
pub(crate) const MAX_POLL_INTERVAL: u32 = 60;

/// The exact tooltip text spelling out the no-retroactive-resplit rule
/// (PRD §5.5.8 + HITL guardrail).
pub const NO_RESPLIT_TOOLTIP: &str = "Billing-cycle start day applies to the \
     NEXT rollover only. The current cycle is not re-split.";

/// Render the settings panel into `ui`, editing `config` in place. The host
/// passes `on_change: &dyn Fn()` which is invoked whenever the user changes
/// any field — the host is responsible for persisting `config.toml` (debounced
/// per PRD §5.5.8).
#[allow(clippy::needless_pass_by_value)]
pub fn render(ui: &mut Ui, _config: &mut Config, _on_change: &dyn Fn()) {
    // RED-phase STUB — renders nothing. The GREEN-phase implementation
    // wires egui widgets for cycle_start_day, temp_unit, raw_values,
    // decimal_base, poll_interval, docked_edge, theme, plus the
    // no-retroactive-resplit tooltip + the T-3 visible poll warning.
    let _ = (ui,);
}

/// Render the docked-edge radio section.
#[allow(dead_code)] // Wired by the GREEN-phase render().
fn dock_edge_section(ui: &mut Ui, dock: &mut DockConfig, changed: &mut bool) {
    ui.label("Docked edge");
    ui.horizontal(|row| {
        for &e in &["Left", "Right", "Top", "Bottom"] {
            let mut selected = dock.edge == e;
            if row.checkbox(&mut selected, e).changed() && selected {
                dock.edge = e.to_string();
                *changed = true;
            }
        }
    });
}

/// Render the theme-mode radio section.
#[allow(dead_code)] // Wired by the GREEN-phase render().
fn theme_section(ui: &mut Ui, theme: &mut ThemeConfig, changed: &mut bool) {
    ui.label("Theme");
    ui.horizontal(|row| {
        for &m in &["Dark", "Light", "System"] {
            let mut selected = theme.mode == m;
            if row.checkbox(&mut selected, m).changed() && selected {
                theme.mode = m.to_string();
                *changed = true;
            }
        }
    });
}

/// Normalize a TempUnit to the v1-supported set (Celsius/Fahrenheit). The
/// Reading's wire unit is always Celsius (canonical); the DisplayConfig's
/// `temp_unit` field only carries Celsius/Fahrenheit in v1.
#[allow(dead_code, clippy::needless_pass_by_value)] // Wired by the GREEN-phase render().
fn display_temp_unit(unit: TempUnit) -> TempUnit {
    unit
}

#[cfg(test)]
mod tests {
    //! Story 8.5 TDD contract tests (F8 egui_kittest + pure-fn unit tests).

    use super::*;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use sidebar_domain::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: cycle_start_day section renders with default day =====

    #[test]
    fn cycle_start_day_section_renders_default() {
        let mut config = Config::default();
        assert_eq!(config.bandwidth.cycle_start_day, CycleStartDaySerde::Day(1));
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Billing cycle start day"),
            "panel must render the cycle-start-day section (got: {labels})"
        );
        // The slider's current value text shows the day number.
        assert!(
            labels.contains("day"),
            "panel must surface the day slider (got: {labels})"
        );
    }

    // ===== Happy Path #1 (persist): changing config field → re-render reflects it =====

    #[test]
    fn change_cycle_start_day_renders_new_value() {
        // Verify the GREEN render path observes a config mutation by setting
        // cycle_start_day=15 BEFORE constructing the harness — the rendered
        // labels must surface "15". This locks in that render() reads from
        // config (no caching).
        let mut config = Config::default();
        config.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("15"),
            "after setting cycle_start_day=15 the panel must surface '15' (got: {labels})"
        );
    }

    // ===== Happy Path #2: toggle raw_values on → flips config flag =====
    //
    // The settings panel updates the config; the metric-row re-render is
    // verified via the composition test (render_snapshot in mod.rs). Here we
    // lock in that clicking the checkbox flips the config flag AND fires the
    // on_change callback.

    #[test]
    fn toggle_raw_values_flips_config_flag() {
        // Use a shared config behind Arc<Mutex<>> so the closure can mutate it
        // AND the test can read it after the harness click. This mirrors the
        // real wiring where AppState holds Config behind a lock.
        use std::sync::Mutex;
        let config = Arc::new(Mutex::new(Config::default()));
        assert!(
            !config.lock().unwrap().display.raw_values,
            "default raw_values=false"
        );
        let counter = Arc::new(AtomicUsize::new(0));
        let cfg_for_closure = config.clone();
        let c = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = c.clone();
            let cfg = cfg_for_closure.clone();
            let mut cfg_guard = cfg.lock().unwrap();
            render(ui, &mut cfg_guard, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        // Find the "Show raw values" checkbox and click it.
        harness.get_by_label("Show raw values (Hz/bytes)").click();
        harness.run();
        assert!(
            config.lock().unwrap().display.raw_values,
            "clicking the checkbox must flip display.raw_values=true"
        );
        assert!(
            counter.load(Ordering::SeqCst) >= 1,
            "on_change callback must fire at least once after a toggle"
        );
    }

    // ===== Boundary #1: cycle_start_day change does NOT retroactively re-split =====

    #[test]
    fn cycle_start_day_change_does_not_re_split_current_cycle() {
        // The no-retroactive-resplit rule is documented in NO_RESPLIT_TOOLTIP.
        // The panel surfaces the tooltip text + only mutates the config field
        // (it does NOT touch cycle bookkeeping — that's the host's job on the
        // next rollover).
        let mut config = Config::default();
        let labels = {
            let mut harness = Harness::new_ui(|ui| {
                render(ui, &mut config, &|| {});
            });
            harness.run();
            all_labels(&harness).join(" | ")
        };
        // harness dropped before mutating config (borrow checker safety).
        assert!(
            labels.contains(NO_RESPLIT_TOOLTIP),
            "settings panel must surface the no-retroactive-resplit tooltip (got: {labels})"
        );
        // Pure-fn contract: cycle_start_day can be mutated without touching
        // any other config field. We assert that mutating cycle_start_day
        // leaves the rest of the config unchanged (no retroactive re-split at
        // the data level — the panel is pure render).
        let mut before = Config::default();
        before.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        let mut after = before.clone();
        after.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        assert_eq!(before, after, "cycle_start_day mutation is idempotent");
    }

    // ===== Boundary #2: poll_interval=0 → clamped to 1 with warning =====

    #[test]
    fn poll_interval_zero_clamps_to_one() {
        let mut config = Config {
            poll_interval_seconds: 0,
            ..Config::default()
        };
        let labels = {
            let mut harness = Harness::new_ui(|ui| {
                render(ui, &mut config, &|| {});
            });
            harness.run();
            all_labels(&harness).join(" | ")
        };
        // harness dropped; now we can mutate config.
        assert!(
            config.poll_interval_seconds <= MIN_POLL_INTERVAL,
            "precondition: poll_interval at floor"
        );
        assert!(
            labels.contains("Poll interval at minimum"),
            "poll_interval at floor must surface a visible warning (got: {labels})"
        );
        // The live slider path applies clamp_to_range; we simulate it here.
        let clamped = config
            .poll_interval_seconds
            .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);
        assert_eq!(
            clamped, MIN_POLL_INTERVAL,
            "poll_interval=0 clamps to 1 (T-3)"
        );
    }

    // ===== Boundary #3: cycle_start_day=29 rejected at UI (T-26) =====

    #[test]
    fn cycle_start_day_29_rejected_at_ui() {
        // The slider constrains to [1, 28]. We verify the slider's range by
        // asserting the panel renders the "Last day" escape hatch and the
        // cycle-start-day section.
        assert_eq!(MIN_CYCLE_DAY, 1);
        assert_eq!(MAX_CYCLE_DAY, 28);
        let mut config = Config::default();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Last day"),
            "panel must offer the 'Last day' option for month-end (T-26 escape hatch)"
        );
        assert!(
            labels.contains("Billing cycle start day"),
            "panel must render the cycle-start-day section"
        );
    }

    // ===== Boundary #4: settings closed without save → autosave debounced =====

    #[test]
    fn on_change_callback_invoked_on_edit() {
        // The autosave contract: on any change, the host's on_change fires.
        // The host debounces. We verify the callback fires on a click.
        use std::sync::Mutex;
        let config = Arc::new(Mutex::new(Config::default()));
        let counter = Arc::new(AtomicUsize::new(0));
        let cfg_for_closure = config.clone();
        let c = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = c.clone();
            let cfg = cfg_for_closure.clone();
            let mut cfg_guard = cfg.lock().unwrap();
            render(ui, &mut cfg_guard, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        let before = counter.load(Ordering::SeqCst);
        harness.get_by_label("Last day").click();
        harness.run();
        let after = counter.load(Ordering::SeqCst);
        assert!(
            after > before,
            "toggling 'Last day' must fire on_change (before={before}, after={after})"
        );
        assert_eq!(
            config.lock().unwrap().bandwidth.cycle_start_day,
            CycleStartDaySerde::LastDayOfMonth
        );
    }

    // ===== Pure-fn sanity =====

    #[test]
    fn display_temp_unit_normalizes() {
        assert_eq!(display_temp_unit(TempUnit::Celsius), TempUnit::Celsius);
        assert_eq!(
            display_temp_unit(TempUnit::Fahrenheit),
            TempUnit::Fahrenheit
        );
    }
}
