//! Story 0.1 — Dependency conflict boundary test (RED phase).
//!
//! Per Story 0.1 TDD contract, Boundary test #2: "Dependency conflict:
//! introduce two crates pinning different major versions of tokio;
//! `cargo tree --duplicates` MUST list the conflict (CI gate). Fixture F6."
//!
//! This is the programmatic equivalent — it inspects `cargo metadata`'s
//! resolve graph and fails if any single dependency name resolves to more
//! than one major version. We exclude workspace members themselves.
//!
//! Cited:
//!   - Fixture F6 (idempotency harness pattern — re-runnable)
//!   - G3/G18 (no dependency conflicts allowed without architect review)
//!   - Story 0.1 Boundary test #2

#[test]
fn no_dependency_has_multiple_major_versions() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    // Collect workspace member IDs so we can skip them when scanning
    // the resolve graph.
    let member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    let resolve = metadata
        .resolve
        .as_ref()
        .expect("cargo metadata must include resolve graph (use --no-deps=false)");

    // Map: dep name -> set of major versions seen across the workspace.
    let mut dep_majors: std::collections::HashMap<String, std::collections::HashSet<u64>> =
        std::collections::HashMap::new();

    for node in &resolve.nodes {
        if member_ids.contains(node.id.repr.as_str()) {
            // Skip workspace members — we're scanning external deps.
            continue;
        }
        for dep in &node.deps {
            let dep_name = &dep.name;
            // Extract major version from the dep's package id (format: "name version (source)")
            // cargo_metadata exposes this via node.id.repr parsing OR via metadata.packages lookup.
            // Use the simpler approach: split on whitespace, parse the second token.
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
            conflicts.push(format!(
                "{}: major versions {:?}",
                name, sorted
            ));
        }
    }

    assert!(
        conflicts.is_empty(),
        "Story 0.1 dependency conflict: the following dependencies resolve to multiple major \
         versions in the workspace (G3/G18 violation). Resolve to a single version: {}",
        conflicts.join("; ")
    );
}
