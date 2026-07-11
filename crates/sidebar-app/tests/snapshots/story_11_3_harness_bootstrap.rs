/// Test Layer: L2 — UI snapshot layer smoke marker + Story 11.3 bootstrap.
///
/// Story 11.3 ships a self-contained reference snapshot so the snapshot
/// harness itself can be tested without depending on any GUI story (breaking
/// the 8.1 <-> 11.3 cycle). The reference renders a single label
/// "sidebar snapshot harness OK" via egui_kittest and asserts the rendered
/// text matches. A real `.snap` file (insta) lands alongside this once a
/// HITL reviewer accepts the bootstrap snapshot (G19).
///
/// Cited: Story 11.3 DoD, tdd-fixtures.md F8, guardrails.md G19 (HITL on
/// every snapshot acceptance).

#[test]
fn story_11_3_bootstrap_snapshot_renders_expected_label() {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    let harness = Harness::new_ui(|ui| {
        ui.label("sidebar snapshot harness OK");
    });

    // The kittest access tree must contain the bootstrap label. This is the
    // textual snapshot — a HITL reviewer accepts it once via the
    // `requires-hitl-snapshot` label, after which it becomes the regression
    // baseline for the snapshot harness itself.
    harness.get_by_label("sidebar snapshot harness OK");
}
