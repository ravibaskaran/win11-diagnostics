//! Story 0.1 — Workspace shape contract (RED phase).
//!
//! Verifies the Cargo workspace skeleton matches architecture.md §4:
//! exactly 11 packages (10 library crates + 1 binary crate) with the
//! expected names. Cited contracts:
//!   - architecture.md §4 (crate layout)
//!   - Story 0.1 TDD contract, Happy Path test #2
//!   - G17 (generation cap: max 12 crates — we have 11)
//!   - Fixture F6 (idempotency — this test is re-runnable)
//!
//! This test belongs to NO crate (it lives at the workspace root in `tests/`)
//! and is compiled as part of the workspace's implicit root test target once
//! Story 0.1's GREEN phase lands the root `Cargo.toml`. Until then, this file
//! documents the contract and fails to compile — the RED state.

/// The 11 crate names the workspace MUST contain, in canonical order.
/// Sourced from architecture.md §4 and Story 0.1 Technical Context.
/// Adding or removing a crate is a contract change requiring architect
/// sign-off (G19).
pub const EXPECTED_PACKAGES: &[&str] = &[
    // 10 library crates
    "sidebar-domain",
    "sidebar-sensor",
    "sidebar-adapter-sysinfo",
    "sidebar-adapter-nvml",
    "sidebar-adapter-battery",
    "sidebar-adapter-ohm",
    "sidebar-adapter-pdh",
    "sidebar-adapter-net",
    "sidebar-persistence",
    "sidebar-bandwidth",
    "sidebar-platform",
    // 1 binary crate
    "sidebar-app",
];

/// Count is 11 (10 libs + 1 bin) per Story 0.1 Happy Path test #2.
/// G17 caps the workspace at 12 crates; we use 11.
pub const EXPECTED_PACKAGE_COUNT: usize = 11;

#[test]
fn workspace_has_exactly_expected_package_count() {
    // Per Story 0.1 TDD contract: MetadataCommand returns exactly 11 packages.
    // Threshold: G17 generation cap is 12; we assert exactly 11.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed — is the workspace root Cargo.toml present?");

    let actual_count = metadata.packages.len();
    assert_eq!(
        actual_count, EXPECTED_PACKAGE_COUNT,
        "Story 0.1 contract violation: workspace must contain exactly {} packages, found {}. \
         See architecture.md §4. (G17 cap is 12.)",
        EXPECTED_PACKAGE_COUNT, actual_count
    );
}

#[test]
fn workspace_contains_all_expected_crates_by_name() {
    // Per Story 0.1 TDD contract + architecture.md §4: every expected crate
    // name must be present. Catches typos, missing crates, or renames.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let actual_names: std::collections::HashSet<String> = metadata
        .packages
        .iter()
        .map(|p| p.name.clone())
        .collect();

    for expected in EXPECTED_PACKAGES {
        assert!(
            actual_names.contains(*expected),
            "Story 0.1 contract violation: expected crate '{}' not found in workspace. \
             Present crates: {:?}. See architecture.md §4.",
            expected,
            actual_names
        );
    }
}

#[test]
fn workspace_has_exactly_one_binary_crate() {
    // Per architecture.md §4: sidebar-app is the sole binary crate; the other
    // 10 are libraries. Verifies the [lib]/[[bin]] split is correct.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let bin_targets: Vec<&str> = metadata
        .packages
        .iter()
        .flat_map(|p| {
            p.targets
                .iter()
                .filter(|t| t.kind.iter().any(|k| k == "bin"))
                .map(|_| p.name.as_str())
        })
        .collect();

    assert_eq!(
        bin_targets.len(),
        1,
        "Story 0.1 contract violation: expected exactly 1 binary crate, found {}. \
         sidebar-app is the sole binary per architecture.md §4.",
        bin_targets.len()
    );
    assert_eq!(
        bin_targets[0], "sidebar-app",
        "The single binary crate must be named 'sidebar-app'"
    );
}
