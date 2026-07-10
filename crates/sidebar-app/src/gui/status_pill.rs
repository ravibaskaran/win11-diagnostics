//! Story 8.2 — Status Pill (PRD §5.3).
//!
//! A small colored pill in the sidebar header: **`BASIC`** (muted gray) or
//! **`FULL`** (accent green). Hovering shows the PRD §5.3 tooltip. Clicking
//! the pill in Basic mode invokes the launch-elevated callback (the **only**
//! UAC entry point per PRD §5.4 — HITL guardrail).
//!
//! ## Cited
//!
//! - PRD §5.3 (tooltip text verbatim) + §5.4 (privilege handling)
//! - Story 8.2 TDD contract (Happy Path #1-#2, Boundary #1-#2)
//! - architecture.md §4 + AD-8 (status pill click → `launch_elevated`)
//! - guardrails.md HITL — UAC trigger must be explicit user action only.
//!
//! ## RED phase
//!
//! `render` is a STUB that renders nothing — the tests below encode the
//! Story 8.2 contract and are expected to FAIL at this commit. The GREEN
//! commit implements the real pill.

use eframe::egui::Ui;
use sidebar_sensor::descriptor::ProviderTier;

/// Tooltip shown when the pill is in Basic mode (PRD §5.3, verbatim).
pub const TOOLTIP_BASIC: &str = "Basic mode. CPU temperature, fan speeds, \
     voltages, and non-NVIDIA GPU sensors require OpenHardwareMonitor with \
     administrator privileges. Click to learn how to enable Full mode.";

/// Tooltip shown when the pill is in Full mode (PRD §5.3, verbatim).
pub const TOOLTIP_FULL: &str = "Full mode. OpenHardwareMonitor is running. \
     All sensors active.";

/// Render the status pill.
///
/// - `tier` — the current runtime tier.
/// - `on_click_launch` — invoked when the user clicks the pill in Basic mode.
///   This is the **only** elevation entry point (PRD §5.4 / HITL). The Full
///   tier click is a no-op (the supervisor is already running).
///
/// STUB — renders nothing in RED phase.
pub fn render(_ui: &mut Ui, _tier: ProviderTier, _on_click_launch: &dyn Fn()) {
    // Intentionally empty: RED phase stub. GREEN commit adds the real pill
    // (button + tooltip + click handler).
}

/// Build the label string for a tier ("BASIC" / "FULL" / "BOTH").
///
/// Exposed `pub(crate)` so the snapshot renderer and tests can share the
/// canonical mapping without re-declaring it.
///
/// RED phase: unused in the lib build until the GREEN commit wires it into
/// `render`. The `dead_code` allow is removed at GREEN.
#[allow(dead_code)]
#[must_use]
pub(crate) fn tier_label(tier: ProviderTier) -> &'static str {
    match tier {
        ProviderTier::Basic => "BASIC",
        ProviderTier::Full => "FULL",
        ProviderTier::Both => "BOTH",
    }
}

/// Whether the pill click should trigger launch-elevated for the given tier.
/// Only `Basic` triggers it (Full is already running; `Both` is a provider
/// declaration, not a runtime mode — rendered as Basic-equivalent for click
/// purposes since the user may still want to upgrade).
///
/// RED phase: unused in the lib build until GREEN.
#[allow(dead_code)]
#[must_use]
pub(crate) fn click_triggers_launch(tier: ProviderTier) -> bool {
    matches!(tier, ProviderTier::Basic | ProviderTier::Both)
}

#[cfg(test)]
mod tests {
    //! Story 8.2 TDD contract tests (F8 egui_kittest).
    //!
    //! RED phase: every assertion is expected to FAIL — `render` is a no-op
    //! stub, so the kittest access tree contains nothing pill-related.

    use super::*;
    use eframe::egui;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Walk the kittest access tree and collect every node's text. Mirrors
    /// the Story 8.1 helper — labels and values both surface as text nodes.
    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: tier=Basic → pill renders "BASIC" =====

    #[test]
    fn basic_tier_renders_basic_pill() {
        let mut harness = Harness::new_ui(|ui| {
            render(ui, ProviderTier::Basic, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("BASIC"),
            "Basic tier must render a 'BASIC' pill (got: {labels})"
        );
    }

    // ===== Happy Path #2: click pill in Basic → invokes launch callback =====

    #[test]
    fn click_basic_pill_invokes_launch_callback() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_harness = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            // Clone the Arc each frame so the inner Fn can be re-created
            // (Harness::new_ui requires FnMut — invoked once per step).
            let c = counter_for_harness.clone();
            render(ui, ProviderTier::Basic, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        // Find the BASIC button and click it.
        harness.get_by_label("BASIC").click();
        harness.run();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "clicking the BASIC pill must invoke the launch-elevated callback exactly once"
        );
    }

    // ===== Boundary #1: tier=Full → "FULL" green, click no-op =====

    #[test]
    fn full_tier_renders_full_pill_and_click_is_noop() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_harness = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = counter_for_harness.clone();
            render(ui, ProviderTier::Full, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("FULL"),
            "Full tier must render a 'FULL' pill (got: {labels})"
        );
        // Clicking the Full pill must NOT invoke launch (already running).
        harness.get_by_label("FULL").click();
        harness.run();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "clicking the FULL pill must be a no-op (launch callback not invoked)"
        );
    }

    // ===== Boundary #2: tooltip text matches PRD §5.3 verbatim =====

    #[test]
    fn basic_tooltip_matches_prd_verbatim() {
        // The static constant IS the verbatim PRD text; assert against the
        // exact literal to lock it in. The GREEN commit wires this into
        // ui.on_hover_text(...). We also verify the kittest access tree
        // exposes the tooltip after a hover (tooltips appear as label nodes
        // via egui's on_hover_text → accesskit description).
        let expected = "Basic mode. CPU temperature, fan speeds, voltages, \
             and non-NVIDIA GPU sensors require OpenHardwareMonitor with \
             administrator privileges. Click to learn how to enable Full mode.";
        assert_eq!(TOOLTIP_BASIC, expected);

        let mut harness = Harness::new_ui(|ui| {
            render(ui, ProviderTier::Basic, &|| {});
        });
        harness.run();
        // Hover the BASIC pill to trigger the tooltip.
        let pill = harness.get_by_label("BASIC");
        pill.hover();
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(TOOLTIP_BASIC),
            "hovering the BASIC pill must surface the PRD §5.3 tooltip verbatim \
             (got: {labels})"
        );
    }

    #[test]
    fn full_tooltip_matches_prd_verbatim() {
        let expected = "Full mode. OpenHardwareMonitor is running. \
             All sensors active.";
        assert_eq!(TOOLTIP_FULL, expected);

        let mut harness = Harness::new_ui(|ui| {
            render(ui, ProviderTier::Full, &|| {});
        });
        harness.run();
        let pill = harness.get_by_label("FULL");
        pill.hover();
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(TOOLTIP_FULL),
            "hovering the FULL pill must surface the PRD §5.3 tooltip verbatim \
             (got: {labels})"
        );
    }

    /// Sanity: tier_label maps each variant. (This one passes in RED too —
    /// the helper is real — but it locks the contract.)
    #[test]
    fn tier_label_maps_each_variant() {
        assert_eq!(tier_label(ProviderTier::Basic), "BASIC");
        assert_eq!(tier_label(ProviderTier::Full), "FULL");
        assert_eq!(tier_label(ProviderTier::Both), "BOTH");
    }

    /// Sanity: only Basic/Both trigger launch on click.
    #[test]
    fn click_triggers_launch_only_for_basic_and_both() {
        assert!(click_triggers_launch(ProviderTier::Basic));
        assert!(click_triggers_launch(ProviderTier::Both));
        assert!(!click_triggers_launch(ProviderTier::Full));
    }

    /// Compile-time anchor: the egui import is used in GREEN; this suppresses
    /// the unused-import warning in RED without `#[allow]` noise.
    #[test]
    fn egui_import_anchor() {
        let _ = egui::Vec2::ZERO;
    }
}
