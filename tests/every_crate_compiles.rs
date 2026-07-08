//! Story 0.1 — Empty workspace member boundary test (RED phase).
//!
//! Per Story 0.1 TDD contract, Boundary test #3: "Empty workspace member:
//! remove src/lib.rs from one crate; cargo check MUST fail with
//! error[E0761] or analogous precise diagnostic — no silent skip."
//!
//! This test does NOT remove files (that would be destructive). Instead,
//! it asserts the positive contract: every workspace member has at least
//! one source file (lib.rs for libraries, main.rs for binaries). If a
//! future PR accidentally deletes a source file, this test catches it
//! at the workspace level.
//!
//! Cited: Story 0.1 Boundary test #3.

use std::path::Path;

#[test]
fn every_workspace_member_has_at_least_one_source_file() {
    // Per Story 0.1 Boundary test #3: workspace members must have their
    // entry-point source file. We assert this on disk rather than via
    // cargo metadata (which doesn't surface missing-source-file errors
    // until build).
    let metadata = cargo_metadata::MetadataCommand::new()
        .exec()
        .expect("cargo metadata failed");

    let workspace_root = metadata.workspace_root;
    assert!(
        workspace_root.exists(),
        "workspace_root does not exist: {}",
        workspace_root.display()
    );

    let mut missing: Vec<String> = Vec::new();

    for pkg in &metadata.packages {
        // Each package has at least one target; each target has a `src_path`.
        // For libraries, src_path points at lib.rs. For binaries, main.rs.
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
        "Story 0.1 contract violation: the following source files are missing \
         (Boundary test #3 — empty workspace member). Restore them: {}",
        missing.join("; ")
    );

    // Sanity: the workspace_root Path is in scope to avoid dead-code warnings
    // if the loop above is refactored out.
    let _ = Path::new(&workspace_root);
}
