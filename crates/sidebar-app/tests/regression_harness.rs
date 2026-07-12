//! Test Layer: L1 — regression-harness metadata and CI wiring contracts.
//!
//! This is intentionally a small integration test.  It checks only the
//! scaffold owned by Story 11.1; the full matrix and coverage/report jobs are
//! deferred to Stories 11.2–11.4.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("sidebar-app must live under the workspace crates directory")
        .to_path_buf()
}

fn read_workspace_file(path: &str) -> String {
    let full_path = workspace_root().join(path);
    let raw = fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", full_path.display()));
    // Normalize CRLF -> LF so substring assertions survive git's
    // core.autocrlf=true checkout normalization on Windows.
    raw.replace("\r\n", "\n")
}

#[test]
fn ci_declares_independent_layer_jobs() {
    let ci = read_workspace_file(".github/workflows/ci.yml");

    for job in ["unit:", "integration:", "bench:"] {
        assert!(ci.contains(job), "CI is missing the {job} job");
    }
    assert!(ci.contains("name: \"cargo test --workspace --lib (L0"));
    assert!(ci.contains("name: \"cargo test --workspace --tests (L1"));
    assert!(ci.contains("name: \"cargo bench (L3"));
    assert!(ci.contains("integration:\n    name:"));
    assert!(ci.contains("runs-on: windows-latest"));
}

#[test]
fn regression_harness_documents_all_layers_and_budgets() {
    let harness = read_workspace_file("docs/backlog/regression-harness.md");

    for layer in ["L0", "L1", "L2", "L3", "L4"] {
        assert!(harness.contains(layer), "harness is missing {layer}");
    }
    for budget in ["60 s total", "30 s total", "600 s total"] {
        assert!(
            harness.contains(budget),
            "harness is missing budget {budget}"
        );
    }
    assert!(harness.contains("Every test in the workspace declares exactly one layer"));
}

#[test]
fn scaffold_has_one_executable_marker_per_layer() {
    let markers = [
        ("crates/sidebar-domain/src/format.rs", "Test Layer: L0"),
        (
            "crates/sidebar-app/tests/regression_harness.rs",
            "Test Layer: L1",
        ),
        (
            "crates/sidebar-app/tests/snapshots/layer_smoke.rs",
            "Test Layer: L2",
        ),
        (
            "crates/sidebar-app/benches/layer_smoke.rs",
            "Test Layer: L3",
        ),
        ("verify/layer-smoke.ps1", "L4 is manual"),
    ];
    for (path, marker) in markers {
        assert!(
            read_workspace_file(path).contains(marker),
            "{path} is missing layer marker {marker}"
        );
    }
}

#[test]
fn smoke_runner_gates_windows_only_layers() {
    let smoke = read_workspace_file("verify/layer-smoke.ps1");
    assert!(smoke.contains("layer-gating:"));
    assert!(smoke.contains("@('L1', 'L3')"));
}

/// Story 11.2 — the CI workflow MUST run `cargo-llvm-cov` (T-43) to produce
/// a coverage report so the coverage-delta gate (T-42) can detect regressions.
/// This is a structural assertion on ci.yml; the actual coverage computation
/// runs in the coverage CI job.
#[test]
fn ci_yaml_runs_cargo_llvm_cov_for_coverage() {
    let ci = read_workspace_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("llvm-cov") || ci.contains("llvm_cov"),
        "ci.yml must invoke cargo-llvm-cov (T-43) for coverage. snippet:\n{ci}"
    );
}

/// Story 11.2 — the CI workflow MUST upload a `regression-report.md` artifact
/// per PR so reviewers can see the per-layer pass/fail + coverage delta. This
/// is the documented 8-point DoD contract.
#[test]
fn ci_yaml_uploads_regression_report_artifact() {
    let ci = read_workspace_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("regression-report"),
        "ci.yml must upload a regression-report artifact (Story 11.2 DoD). snippet:\n{ci}"
    );
}

/// Story 11.2 / T-42 — the CI regression job MUST enforce the absolute
/// coverage floors (the delta-vs-main comparison is HITL-gated, but the
/// absolute floor gate is enforceable from the lcov we already generate).
/// Floors per nfr-thresholds.md T-42:
///   domain / sensor ≥ 80% ; adapter / platform ≥ 60% ; app ≥ 40%.
#[test]
fn ci_yaml_enforces_t42_coverage_floors() {
    let ci = read_workspace_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("T-42 coverage floor gate"),
        "ci.yml regression job must include the T-42 absolute coverage-floor gate step. snippet:\n{ci}"
    );
    // Each floor category must be named in the awk floor map so a crate
    // cannot silently slip past the gate.
    for floor in [
        "sidebar-domain",
        "sidebar-sensor",
        "sidebar-adapter",
        "sidebar-platform",
        "sidebar-app",
    ] {
        assert!(
            ci.contains(floor),
            "ci.yml T-42 gate must name the {floor} crate in its floor map. snippet:\n{ci}"
        );
    }
    assert!(ci.contains("return 80"), "domain/sensor floor must be 80%");
    assert!(
        ci.contains("return 60"),
        "adapter/platform floor must be 60%"
    );
    assert!(ci.contains("return 40"), "app floor must be 40%");
}
