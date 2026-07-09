//! Story 0.1 — Empty workspace member integration test.
//!
//! Per Story 0.1 TDD contract, Boundary test #3: "Empty workspace member:
//! remove src/lib.rs from one crate; cargo check MUST fail with a precise
//! diagnostic — no silent skip."
//!
//! This test asserts the positive contract on disk: every workspace member
//! has at least one source file (lib.rs for libraries, main.rs for
//! binaries). If a future PR deletes a source file, this catches it at
//! the workspace level before build.
//!
//! Cited:
//!   - Story 0.1 Boundary test #3

#[test]
fn every_workspace_member_has_at_least_one_source_file() {
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let workspace_root = &metadata.workspace_root;
    assert!(
        workspace_root.exists(),
        "workspace_root does not exist: {}",
        workspace_root.as_str()
    );

    let mut missing: Vec<String> = Vec::new();

    // Only check workspace members, not transitive deps (whose source files
    // may be absent from the cargo registry cache on fresh clones).
    let workspace_member_ids: std::collections::HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|id| id.repr.as_str())
        .collect();

    for pkg in &metadata.packages {
        if !workspace_member_ids.contains(pkg.id.repr.as_str()) {
            continue; // skip external deps
        }
        for target in &pkg.targets {
            let src_path = &target.src_path;
            if !src_path.as_std_path().exists() {
                missing.push(format!(
                    "crate '{}', target '{}': missing source file {}",
                    pkg.name,
                    target.name,
                    src_path.as_str()
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Story 0.1 contract violation: source files missing (Boundary test #3). \
         Restore them: {}",
        missing.join("; ")
    );
}
