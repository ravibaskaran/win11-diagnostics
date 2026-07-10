//! Story 8.9 — Metric Enable/Disable + Drag-Reorder UI.
//!
//! A settings sub-panel rendering the configured metric list with:
//!
//! - A drag handle (⠿) on each row — drag to reorder. Uses the egui 0.35
//!   native `Ui::dnd_drag_source` / `Ui::dnd_drop_zone` API + the
//!   `Response::dnd_set_drag_payload` / `dnd_release_payload` helpers (no
//!   `egui_dnd` crate — keeps the dep surface unchanged, G25 §3).
//! - A per-row checkbox — toggle whether the metric renders in the live view.
//!   Drives `[metrics] enabled` (Story 1.5).
//!
//! Persistence contract: every change (toggle OR reorder) invokes `on_change`
//! — the host debounces a `config.toml` write (PRD §5.5.8, same pattern as
//! the Story 8.5 settings panel).
//!
//! ## Ordering semantics
//!
//! `[metrics] order` is the canonical display sequence (Story 1.5). A metric
//! present in `order` but not `enabled` is rendered in this list with its
//! checkbox unchecked (so the user can re-enable it); a metric in `enabled`
//! but missing from `order` is appended at the end. The live sidebar view
//! (`render_sidebar` in `mod.rs`) consumes `order` filtered by `enabled`.
//!
//! ## Boundary cases
//!
//! - Empty `enabled` → "No metrics enabled" placeholder (the user is never
//!   left staring at a blank panel).
//! - Reorder persists across restart — verified by the config round-trip test
//!   (TOML serialize → deserialize preserves `order`).
//!
//! ## Cited
//!
//! - Story 8.9 TDD contract (disable CpuPower → row disappears; drag reorder
//!   updates order; boundary: all-disabled placeholder; config round-trip;
//!   metric in order-but-not-enabled ignored by the live view).
//! - nfr-thresholds.md T-21 (metric rendering).
//! - sidebar-domain::config::MetricsConfig (Story 1.5).

use eframe::egui::Ui;
use sidebar_domain::config::MetricsConfig;

/// Placeholder text shown when no metrics are enabled (Boundary: all-disabled).
pub const NO_METRICS_TEXT: &str = "No metrics enabled";

/// Render the metric enable/disable + reorder panel into `ui`, editing
/// `metrics` in place. The host passes `on_change: &dyn Fn()` which is
/// invoked whenever the user toggles a checkbox or completes a drag — the
/// host is responsible for persisting `config.toml` debounced (same pattern
/// as Story 8.5 settings panel).
///
/// `id_salt` disambiguates the egui::Id namespace when multiple metric lists
/// are rendered in the same frame (the F8 tests + the production settings
/// panel both render a list — distinct salts keep the DnD payloads isolated).
pub fn render(ui: &mut Ui, metrics: &mut MetricsConfig, id_salt: &str, on_change: &dyn Fn()) {
    // STUB (RED phase): renders nothing meaningful. The real implementation
    // (GREEN commit) wires the egui 0.35 DnD API + per-row checkboxes.
    // Deliberately NOT todo!()/unimplemented!() (clippy::todo = "deny").
    let _ = (metrics, id_salt, on_change);
    stub_render(ui);
}

/// Stub used only in the RED phase so the public render() signature compiles
/// without satisfying the TDD contract. Replaced wholesale in GREEN.
fn stub_render(_ui: &mut Ui) {}

// ===========================================================================
// Pure-fn helpers (extracted for testability — the live render path mutates
// `order` via these when a drag completes or a checkbox toggles).
// ===========================================================================

/// Move the metric at `from_idx` to `to_idx`, shifting the intervening
/// entries. No-op on out-of-range or equal indices (defensive).
#[allow(dead_code)] // Used by the GREEN render path + tests.
pub(crate) fn move_metric(order: &mut Vec<String>, from_idx: usize, to_idx: usize) {
    if from_idx == to_idx || from_idx >= order.len() || to_idx >= order.len() {
        return;
    }
    let item = order.remove(from_idx);
    order.insert(to_idx, item);
}

/// Return the metric names that are BOTH in `order` AND `enabled`, preserving
/// the `order` sequence. This is what the live sidebar view renders.
#[must_use]
#[allow(dead_code)] // Used by the GREEN render path + tests.
pub(crate) fn enabled_in_order(metrics: &MetricsConfig) -> Vec<String> {
    metrics
        .order
        .iter()
        .filter(|name| metrics.enabled.iter().any(|e| e == *name))
        .cloned()
        .collect()
}

/// Toggle a metric's membership in `enabled`. Returns true if the metric
/// is now enabled (i.e. it was just added).
#[allow(dead_code)] // Used by the GREEN render path + tests.
pub(crate) fn toggle_enabled(metrics: &mut MetricsConfig, name: &str) -> bool {
    if let Some(pos) = metrics.enabled.iter().position(|e| e == name) {
        metrics.enabled.remove(pos);
        false
    } else {
        metrics.enabled.push(name.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.9 TDD contract tests.
    //!
    //! F8 (egui_kittest) for the render contract + pure-fn unit tests for the
    //! order-mutation helpers (toggle + reorder).

    use super::*;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    fn metrics_with(enabled: &[&str], order: &[&str]) -> MetricsConfig {
        MetricsConfig {
            enabled: enabled.iter().map(|s| (*s).to_string()).collect(),
            order: order.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    // ===== Happy Path #1: each enabled metric renders a row with its name =====

    #[test]
    fn enabled_metrics_render_as_rows() {
        let mut metrics = metrics_with(
            &["CpuUtilization", "CpuTemperature", "CpuPower"],
            &["CpuUtilization", "CpuTemperature", "CpuPower"],
        );
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut metrics, "test", &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("CpuPower"),
            "panel must render 'CpuPower' row (got: {labels})"
        );
        assert!(
            labels.contains("CpuUtilization"),
            "panel must render 'CpuUtilization' row (got: {labels})"
        );
    }

    // ===== Happy Path #2: disable CpuPower → row is unchecked (still listed) =====

    #[test]
    fn disable_metric_unchecks_row() {
        // We simulate a disable by pre-mutating enabled to exclude CpuPower.
        // The panel renders every metric in `order` with a checkbox reflecting
        // its enabled state.
        let mut metrics = metrics_with(
            &["CpuUtilization", "CpuTemperature"],
            &["CpuUtilization", "CpuTemperature", "CpuPower"],
        );
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut metrics, "test", &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("CpuPower"),
            "disabled metric must still appear as an unchecked row (got: {labels})"
        );
    }

    // ===== Happy Path #3: clicking a checkbox updates `enabled` + fires on_change =====

    #[test]
    fn click_checkbox_toggles_enabled_and_fires_on_change() {
        let metrics = Arc::new(Mutex::new(metrics_with(
            &["CpuUtilization"],
            &["CpuUtilization", "CpuPower"],
        )));
        let counter = Arc::new(AtomicUsize::new(0));
        let m = metrics.clone();
        let c = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = c.clone();
            let m = m.clone();
            let mut guard = m.lock().unwrap();
            render(ui, &mut guard, "test", &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        // Find the CpuPower checkbox row and click it.
        harness.get_by_label("CpuPower").click();
        harness.run();
        let after = metrics.lock().unwrap();
        assert!(
            after.enabled.contains(&"CpuPower".to_string()),
            "clicking CpuPower checkbox must add it to enabled (got: {:?})",
            after.enabled
        );
        assert!(
            counter.load(Ordering::SeqCst) >= 1,
            "on_change must fire at least once after toggle"
        );
    }

    // ===== Boundary #1: all metrics disabled → "No metrics enabled" placeholder =====

    #[test]
    fn all_disabled_shows_placeholder() {
        let mut metrics = metrics_with(&[], &["CpuUtilization", "CpuPower"]);
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut metrics, "test", &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(NO_METRICS_TEXT),
            "all metrics disabled must render the placeholder (got: {labels})"
        );
    }

    // ===== Boundary #2: metric in order but not enabled → ignored by live view =====

    #[test]
    fn order_filtered_by_enabled_for_live_view() {
        let metrics = metrics_with(
            &["CpuUtilization"],
            &["CpuUtilization", "CpuPower", "CpuTemperature"],
        );
        let live = enabled_in_order(&metrics);
        assert_eq!(
            live,
            &["CpuUtilization".to_string()],
            "live view must show only enabled metrics in order (got: {live:?})"
        );
    }

    // ===== Boundary #3: reorder updates `order` (config round-trip) =====

    #[test]
    fn reorder_persists_through_config_round_trip() {
        let mut metrics = metrics_with(
            &["CpuUtilization", "CpuFrequency", "CpuTemperature"],
            &["CpuUtilization", "CpuFrequency", "CpuTemperature"],
        );
        // Simulate a reorder: CpuFrequency moves above CpuUtilization.
        move_metric(&mut metrics.order, 1, 0);
        // Serialize to TOML + parse back — order must survive.
        let toml_str = {
            let cfg = sidebar_domain::config::Config {
                metrics: metrics.clone(),
                ..sidebar_domain::config::Config::default()
            };
            cfg.to_toml_string().expect("toml serialize")
        };
        let parsed = sidebar_domain::config::Config::from_toml_str(&toml_str).expect("toml parse");
        assert_eq!(
            parsed.metrics.order,
            vec![
                "CpuFrequency".to_string(),
                "CpuUtilization".to_string(),
                "CpuTemperature".to_string(),
            ],
            "reordered order must survive a TOML round-trip"
        );
    }

    // ===== Pure-fn helper tests =====

    #[test]
    fn move_metric_noop_on_equal_indices() {
        let mut order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        move_metric(&mut order, 1, 1);
        assert_eq!(
            order,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn move_metric_moves_forward() {
        let mut order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        move_metric(&mut order, 0, 2);
        assert_eq!(
            order,
            vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
    }

    #[test]
    fn move_metric_moves_backward() {
        let mut order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        move_metric(&mut order, 2, 0);
        assert_eq!(
            order,
            vec!["c".to_string(), "a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn toggle_enabled_adds_then_removes() {
        let mut metrics = metrics_with(&["a"], &["a", "b"]);
        assert!(toggle_enabled(&mut metrics, "b"), "first toggle adds");
        assert!(metrics.enabled.contains(&"b".to_string()));
        assert!(!toggle_enabled(&mut metrics, "b"), "second toggle removes");
        assert!(!metrics.enabled.contains(&"b".to_string()));
    }

    #[test]
    fn enabled_in_order_preserves_order_sequence() {
        let metrics = metrics_with(&["c", "a"], &["a", "b", "c"]);
        // Only enabled entries, in the order-sequence (a, c — not c, a).
        assert_eq!(
            enabled_in_order(&metrics),
            vec!["a".to_string(), "c".to_string()]
        );
    }
}
