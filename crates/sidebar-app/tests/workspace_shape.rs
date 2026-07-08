//! Story 0.1 — Workspace shape integration test.
//!
//! Verifies the Cargo workspace skeleton matches architecture.md §4:
//! exactly 11 packages (10 library crates + 1 binary crate) with the
//! expected names, and exactly one binary crate (`sidebar-app`).
//!
//! Lives in `sidebar-app` (the top-level binary crate) because workspace-
//! shape contracts transcend any single library. Dev-deps `cargo_metadata`
//! + `semver` are declared in `crates/sidebar-app/Cargo.toml`.
//!
//! Cited contracts:
//!   - architecture.md §4 (crate layout)
//!   - Story 0.1 TDD contract, Happy Path test #1 + #2
//!   - G17 (generation cap: max 12 crates — we have 11)
//!   - Fixture F6 (idempotency — re-runnable)

/// The 12 crate names the workspace MUST contain (11 library crates + 1 binary).
/// Sourced from architecture.md §4 — domain, sensor, 6 adapters, persistence,
/// bandwidth, platform, app. Story 0.1's prose said "10 libs + 1 bin" but the
/// architecture's crate list (and this story's Technical Context) actually
/// names 11 libs + 1 bin = 12. The test follows the architecture.
/// Adding or removing a crate is a contract change requiring architect
/// sign-off (G19 — modifying the workspace member list).
pub const EXPECTED_PACKAGES: &[&str] = &[
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
    "sidebar-app",
];

/// Count is 12 (11 libs + 1 bin) per architecture.md §4.
/// G17 caps the workspace at 12; we are at the cap.
pub const EXPECTED_PACKAGE_COUNT: usize = 12;

#[test]
fn workspace_has_exactly_expected_package_count() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed — is the workspace root Cargo.toml present?");

    // metadata.packages includes workspace members AND all transitive deps.
    // Filter to workspace members only (those whose id is in workspace_members).
    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let workspace_packages: Vec<&cargo_metadata::Package> = metadata
        .packages
        .iter()
        .filter(|p| workspace_member_ids.contains(p.id.repr.as_str()))
        .collect();

    let actual_count = workspace_packages.len();
    assert_eq!(
        actual_count, EXPECTED_PACKAGE_COUNT,
        "Story 0.1 contract violation: workspace must contain exactly {EXPECTED_PACKAGE_COUNT} member packages, \
         found {actual_count}. See architecture.md §4. (G17 cap is 12.)"
    );
}

#[test]
fn workspace_contains_all_expected_crates_by_name() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let actual_names: std::collections::HashSet<String> = metadata
        .packages
        .iter()
        .filter(|p| workspace_member_ids.contains(p.id.repr.as_str()))
        .map(|p| p.name.clone())
        .collect();

    for expected in EXPECTED_PACKAGES {
        assert!(
            actual_names.contains(*expected),
            "Story 0.1 contract violation: expected crate '{expected}' not found in workspace. \
             Present crates: {actual_names:?}. See architecture.md §4."
        );
    }
}

#[test]
fn workspace_has_exactly_one_binary_crate() {
    // Per architecture.md §4: sidebar-app is the sole binary crate; the
    // other 10 are libraries. Catches accidental [[bin]] additions.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let bin_targets: Vec<&str> = metadata
        .packages
        .iter()
        .filter(|p| workspace_member_ids.contains(p.id.repr.as_str()))
        .flat_map(|p| {
            p.targets
                .iter()
                .filter(|t| t.kind.contains(&cargo_metadata::TargetKind::Bin))
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
