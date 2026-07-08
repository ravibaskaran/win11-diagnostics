//! Story 0.1 — Dependency conflict integration test.
//!
//! Per Story 0.1 TDD contract, Boundary test #2: "Dependency conflict:
//! introduce two crates pinning different major versions of tokio;
//! cargo tree --duplicates MUST list the conflict."
//!
//! This is the programmatic equivalent — scans cargo metadata's resolve
//! graph and fails if any single dependency name resolves to more than
//! one major version.
//!
//! Cited:
//!   - Story 0.1 Boundary test #2
//!   - G3/G18 (no dependency conflicts without architect review)
//!   - Fixture F6 (idempotency harness — re-runnable)

#[test]
fn no_dependency_has_multiple_major_versions() {
    // Story 0.1 GREEN has zero runtime deps, so this test trivially passes.
    // It earns its keep once stories 1.x + 3.x start landing deps —
    // if two crates pin tokio 0.5 + tokio 1.x, this fails.

    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let resolve = metadata
        .resolve
        .as_ref()
        .expect("cargo metadata must include resolve graph");

    // Map: dep name -> set of major versions seen across the workspace.
    let mut dep_majors: std::collections::HashMap<String, std::collections::HashSet<u64>> =
        std::collections::HashMap::new();

    for node in &resolve.nodes {
        if member_ids.contains(node.id.repr.as_str()) {
            // Scan only external deps, not workspace members.
            continue;
        }
        for dep in &node.deps {
            let dep_name = &dep.name;
            let parts: Vec<&str> = dep.pkg.repr.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(v) = semver::Version::parse(parts[1]) {
                    dep_majors
                        .entry(dep_name.clone())
                        .or_default()
                        .insert(v.major);
                }
            }
        }
    }

    let mut conflicts: Vec<String> = Vec::new();
    for (name, majors) in &dep_majors {
        if majors.len() > 1 {
            let mut sorted: Vec<u64> = majors.iter().copied().collect();
            sorted.sort_unstable();
            conflicts.push(format!("{name}: major versions {sorted:?}"));
        }
    }

    assert!(
        conflicts.is_empty(),
        "Story 0.1 dependency conflict: the following dependencies resolve to multiple major \
         versions in the workspace (G3/G18 violation): {}",
        conflicts.join("; ")
    );
}
