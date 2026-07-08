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
fn workspace_has_exactly_one_application_binary_crate() {
    // Per architecture.md §4: sidebar-app is the sole *application* binary
    // crate; the other 10 are libraries. Utility binaries (like
    // parse_threshold, added in Story 0.2 for NFR-1 bench parsing) live as
    // additional [[bin]] targets UNDER sidebar-app, not as separate crates.
    //
    // Contract refinement (Story 0.2): the rule is "exactly 1 package whose
    // name is sidebar-app has a binary target," NOT "exactly 1 binary target
    // total." This keeps the workspace at 12 packages (G17 cap) while
    // allowing developer tooling binaries.
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    // Packages (not individual targets) that produce at least one binary.
    let bin_packages: Vec<&str> = metadata
        .packages
        .iter()
        .filter(|p| workspace_member_ids.contains(p.id.repr.as_str()))
        .filter(|p| {
            p.targets
                .iter()
                .any(|t| t.kind.contains(&cargo_metadata::TargetKind::Bin))
        })
        .map(|p| p.name.as_str())
        .collect();

    assert_eq!(
        bin_packages.len(),
        1,
        "Story 0.1/0.2 contract: expected exactly 1 package producing binaries, found {}: {:?}. \
         Only sidebar-app may produce binaries (utility bins like parse_threshold live \
         under sidebar-app/src/bin/, not as separate packages).",
        bin_packages.len(),
        bin_packages
    );
    assert_eq!(
        bin_packages[0], "sidebar-app",
        "The sole binary-producing package must be named 'sidebar-app'"
    );
}
