//! Story 10.2 — smoke-checklist structural tests.
//!
//! Asserts verify/smoke-checklist.md and verify/smoke-checklist.ps1 exist,
//! are well-formed, and cover the 18 documented items. The actual smoke
//! runs on a Win11 host (L4); these tests are the L1 contract that the
//! checklist + script stay in sync with the documented item count.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("sidebar-app must live under crates/")
        .to_path_buf()
}

#[test]
fn smoke_checklist_md_exists_and_lists_18_items() {
    let path = workspace_root().join("verify/smoke-checklist.md");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // Count table rows starting with `| <digit>` — the 18 items.
    let item_rows = raw
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('|')
                && trimmed[1..]
                    .trim_start()
                    .starts_with(|c: char| c.is_ascii_digit())
        })
        .count();
    assert!(
        item_rows >= 18,
        "smoke-checklist.md must list >= 18 items; found {item_rows}"
    );
}

#[test]
fn smoke_checklist_ps1_exists_and_parses() {
    let path = workspace_root().join("verify/smoke-checklist.ps1");
    assert!(path.exists(), "{} must exist", path.display());
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // The script must invoke at least the core automatable items.
    assert!(
        raw.contains("Invoke-SmokeItem"),
        "script must define Invoke-SmokeItem"
    );
    assert!(
        raw.contains("nfr_cold_start"),
        "script must run item 1 (cold-start)"
    );
    assert!(
        raw.contains("nfr_sqlite_rss"),
        "script must run item 5 (SQLite RSS)"
    );
    assert!(
        raw.contains("restart_mid_cycle"),
        "script must run item 16 (R11 persistence)"
    );
}

#[test]
fn smoke_checklist_marks_manual_items_separately() {
    let path = workspace_root().join("verify/smoke-checklist.md");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // The checklist must distinguish automatable vs manual items.
    assert!(raw.contains("Automatable"));
    assert!(raw.contains("manual"));
    // UAC + OBS + multi-monitor items must be marked manual.
    assert!(raw.to_lowercase().contains("uac"));
    assert!(raw.to_lowercase().contains("obs"));
}
