//! About dialog (Story 13.4).
//!
//! A small `egui::Window` triggered by the "ⓘ" button in the sidebar header
//! (next to the gear). Shows the app version, LHM credit + license link,
//! privacy-policy link, GitHub issues link, and the LHM one-time-click
//! Full-mode instructions (Path A, approved 2026-07-13).
//!
//! ## Cited
//! Story 13.4 TDD contract. guardrails.md G28 (non-technical-user
//! hardening). nfr-thresholds.md T-37 (first-run wizard analog). Fixture
//! F8 (egui_kittest).

use eframe::egui::Ui;

/// The LHM one-time-click instructions shown in the About dialog. Cited:
/// Story 13.4, G28, Epic 13 Path A decision.
pub const LHM_ONE_TIME_CLICK_INSTRUCTIONS: &str = "Click the BASIC pill in the \
    sidebar and accept the Windows UAC prompt. If sensor readings (CPU \
    temperature, fan speeds, voltages) do not appear within ~10 seconds, \
    find the LibreHardwareMonitor icon in the system tray (bottom-right), \
    right-click it → View → Web Server. This is a one-time setup; LHM \
    remembers the setting for future launches. Then click the sidebar \
    status pill again.";

/// The GitHub issues URL. Cited: Story 13.4.
pub const GITHUB_ISSUES_URL: &str = "https://github.com/ravibaskaran/win11-diagnostics/issues";

/// The privacy-policy path (relative to the repo root; also linked from
/// README + SECURITY + the code-signing policy). Cited: Story 13.4.
pub const PRIVACY_POLICY_PATH: &str = "docs/privacy-policy.md";

/// Render the About dialog into `ui`. `open` controls visibility; the
/// dialog sets it to `false` when the user clicks Close. Cited: Story
/// 13.4, G28, F8.
pub fn render_about(ui: &mut Ui, open: &mut bool) {
    if !*open {
        return;
    }
    // egui::Window::open borrows `open` mutably (to set it false when the
    // user clicks the X). The closure below does NOT touch `open` — the
    // Window's built-in X button is the only close affordance, so there's
    // no second mutable borrow.
    egui::Window::new("About sidebar")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ui.ctx(), |ui| {
            ui.vertical(|ui| {
                ui.heading(format!("sidebar v{}", env!("CARGO_PKG_VERSION")));
                ui.label(
                    egui::RichText::new("Glanceable system health, calmly.")
                        .small()
                        .weak(),
                );
                ui.separator();

                ui.heading("What this is");
                ui.label(
                    "A lightweight desktop sidebar for Windows 11 that shows \
                     live hardware telemetry (CPU, GPU, RAM, disk, network) \
                     and monthly bandwidth tracking. No telemetry, no \
                     account, no cloud — all readings stay on your machine.",
                );

                ui.separator();
                ui.heading("Full mode (temperature, fans, voltages)");
                ui.label(LHM_ONE_TIME_CLICK_INSTRUCTIONS);

                ui.separator();
                ui.heading("Credits");
                ui.label(format!(
                    "Bundled LibreHardwareMonitor (MPL-2.0) — see {}",
                    "resources/LibreHardwareMonitor.LICENSE.txt"
                ));
                ui.label("Inspired by SidebarDiagnostics (the original C# app).");

                ui.separator();
                ui.heading("Links");
                ui.label(format!("Privacy policy: {PRIVACY_POLICY_PATH}"));
                ui.label(format!("Report a bug: {GITHUB_ISSUES_URL}"));
            });
        });
}

#[cfg(test)]
mod tests {
    //! Story 13.4 TDD contract tests for the About dialog. Cited: F8.

    use super::*;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;

    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    /// Cited: Story 13.4, F8. The About dialog MUST surface the app version,
    /// LHM credit, privacy-policy link, GitHub issues link, and the
    /// Full-mode one-time-click instructions.
    #[test]
    fn about_dialog_renders_version_lhm_credit_privacy_link() {
        let mut open = true;
        let mut harness = Harness::new_ui(|ui| {
            render_about(ui, &mut open);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "About MUST surface the app version (got: {labels})"
        );
        assert!(
            labels.contains("LibreHardwareMonitor"),
            "About MUST credit LibreHardwareMonitor (got: {labels})"
        );
        assert!(
            labels.contains("Privacy policy"),
            "About MUST link the privacy policy (got: {labels})"
        );
        assert!(
            labels.contains("github.com"),
            "About MUST link GitHub issues (got: {labels})"
        );
    }

    /// Cited: Story 13.4, F8, G28. The Full-mode instructions MUST contain
    /// the literal phrase "View → Web Server" (the Path A one-time-click
    /// documentation).
    #[test]
    fn about_dialog_full_mode_instructions_contain_view_web_server() {
        let mut open = true;
        let mut harness = Harness::new_ui(|ui| {
            render_about(ui, &mut open);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("View") && labels.contains("Web Server"),
            "About MUST document the LHM one-time-click (View → Web Server) (got: {labels})"
        );
    }

    /// Cited: Story 13.4, F8. When `open` is false, the dialog MUST NOT
    /// render any content.
    #[test]
    fn about_dialog_does_not_render_when_closed() {
        let mut open = false;
        let mut harness = Harness::new_ui(|ui| {
            render_about(ui, &mut open);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            !labels.contains("About sidebar"),
            "About dialog MUST NOT render when open=false (got: {labels})"
        );
    }
}
