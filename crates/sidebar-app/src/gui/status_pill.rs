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

use eframe::egui::{self, Color32, Ui};
use sidebar_sensor::descriptor::ProviderTier;

/// Tooltip shown when the pill is in Basic mode (PRD §5.3, verbatim).
pub const TOOLTIP_BASIC: &str = "Basic mode. CPU temperature, fan speeds, \
     voltages, and non-NVIDIA GPU sensors require LibreHardwareMonitor with \
     administrator privileges. Click to learn how to enable Full mode.";

/// Tooltip shown when the pill is in Full mode (PRD §5.3, verbatim).
pub const TOOLTIP_FULL: &str = "Full mode. LibreHardwareMonitor is running. \
     All sensors active.";

/// Muted gray fill for the BASIC pill (PRD §5.3 — "muted gray"). A neutral
/// mid-gray that reads as inactive in both light and dark egui themes.
const BASIC_FILL: Color32 = Color32::from_rgb(120, 120, 120);

/// Accent green fill for the FULL pill (PRD §5.3 — "accent green"). A vivid
/// green that reads as "active / OK" across themes.
const FULL_FILL: Color32 = Color32::from_rgb(46, 160, 67);

/// Render the status pill.
///
/// - `tier` — the current runtime tier.
/// - `on_click_launch` — invoked when the user clicks the pill in Basic mode.
///   This is the **only** elevation entry point (PRD §5.4 / HITL). The Full
///   tier click is a no-op (the supervisor is already running).
///
/// Design:
/// - The pill is an `egui::Button` with a colored fill + the tier label.
/// - The tooltip is wired via `response.on_hover_text(...)` so it surfaces in
///   the accesskit tree (kittest F8 contract).
/// - On click, if `click_triggers_launch(tier)` returns true, we invoke the
///   callback. HITL: this is the user's explicit action — no auto-elevation.
pub fn render(ui: &mut Ui, tier: ProviderTier, on_click_launch: &dyn Fn()) {
    let label = tier_label(tier);
    let fill = pill_fill(tier);
    let tooltip = pill_tooltip(tier);

    // v1.0 UI/UX (audit MJ-Z4) — debounce: store the last-click instant
    // in egui memory so repeated clicks within 5s don't fire multiple
    // launch_elevated calls (which queue multiple UAC prompts).
    let debounce_id = egui::Id::new("status_pill_last_click");
    let now = ui.ctx().input(|i| i.time);
    let last_click: f64 = ui.ctx().data(|d| d.get_temp(debounce_id)).unwrap_or(-999.0);
    let cooldown = 1.5; // seconds — blocks UAC double-queue but doesn't stall post-success

    let button_label = if now - last_click < cooldown && click_triggers_launch(tier) {
        "LAUNCHING…"
    } else {
        label
    };
    let button_fill = if now - last_click < cooldown && click_triggers_launch(tier) {
        egui::Color32::from_rgb(180, 140, 0) // muted amber while launching
    } else {
        fill
    };

    let button = egui::Button::new(button_label)
        .fill(button_fill)
        .corner_radius(8);
    let response = ui.add(button);
    let response = response.on_hover_text(tooltip);

    // HITL (PRD §5.4): only an explicit user click triggers launch_elevated.
    // Full tier click is a no-op (supervisor already running). Both is a
    // provider declaration, not a runtime mode — treat as Basic-clickable.
    // v1.0: debounce so clicks within the cooldown window are ignored.
    if response.clicked() && click_triggers_launch(tier) && now - last_click >= cooldown {
        ui.ctx().data_mut(|d| d.insert_temp(debounce_id, now));
        tracing::info!(
            target = "sidebar.app.status_pill",
            tier = ?tier,
            "user clicked status pill — invoking launch_elevated (HITL explicit action)"
        );
        on_click_launch();
        // Request repaint so the pill returns to normal after cooldown.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1500));
    }
}

/// Build the label string for a tier ("BASIC" / "FULL" / "BOTH").
///
/// Exposed `pub(crate)` so the snapshot renderer and tests can share the
/// canonical mapping without re-declaring it.
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
#[must_use]
pub(crate) fn click_triggers_launch(tier: ProviderTier) -> bool {
    matches!(tier, ProviderTier::Basic | ProviderTier::Both)
}

/// Pick the pill fill color per tier (PRD §5.3 — Basic gray, Full green).
/// Both is a provider declaration surfaced in the header as a neutral pill.
#[must_use]
pub(crate) fn pill_fill(tier: ProviderTier) -> Color32 {
    match tier {
        ProviderTier::Basic | ProviderTier::Both => BASIC_FILL,
        ProviderTier::Full => FULL_FILL,
    }
}

/// Pick the tooltip text per tier (PRD §5.3 verbatim).
#[must_use]
pub(crate) fn pill_tooltip(tier: ProviderTier) -> &'static str {
    match tier {
        // Both is a provider declaration; show the Basic tooltip so the user
        // still has the "click to enable Full" affordance.
        ProviderTier::Basic | ProviderTier::Both => TOOLTIP_BASIC,
        ProviderTier::Full => TOOLTIP_FULL,
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.2 TDD contract tests (F8 egui_kittest).
    //!
    //! RED phase: every assertion is expected to FAIL — `render` is a no-op
    //! stub, so the kittest access tree contains nothing pill-related.

    use super::*;
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
             and non-NVIDIA GPU sensors require LibreHardwareMonitor with \
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
        let expected = "Full mode. LibreHardwareMonitor is running. \
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

    /// Sanity: pill_fill picks the PRD §5.3 colors.
    #[test]
    fn pill_fill_picks_prd_colors() {
        assert_eq!(pill_fill(ProviderTier::Basic), BASIC_FILL);
        assert_eq!(pill_fill(ProviderTier::Full), FULL_FILL);
        // Both reuses the muted gray (provider declaration, not a mode).
        assert_eq!(pill_fill(ProviderTier::Both), BASIC_FILL);
    }

    /// Sanity: pill_tooltip picks the PRD §5.3 verbatim text per tier.
    #[test]
    fn pill_tooltip_picks_prd_text() {
        assert_eq!(pill_tooltip(ProviderTier::Basic), TOOLTIP_BASIC);
        assert_eq!(pill_tooltip(ProviderTier::Full), TOOLTIP_FULL);
        assert_eq!(pill_tooltip(ProviderTier::Both), TOOLTIP_BASIC);
    }
}
