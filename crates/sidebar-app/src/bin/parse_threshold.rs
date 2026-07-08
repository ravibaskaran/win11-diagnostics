//! Story 0.2 — NFR-1 threshold parser for criterion bench output.
//!
//! Reads a criterion estimate JSON file (the `*.json` files criterion writes
//! under `target/criterion/<group>/<benchmark>/new/estimate.json`), extracts
//! the mean `typical` value (nanoseconds), converts to a CPU% given the
//! per-iteration unit cost, and exits non-zero if any group exceeds the T-1
//! threshold (0.5% CPU average).
//!
//! Cited:
//!   - Story 0.2 TDD contract: "parse_threshold.rs unit-tested with synthetic
//!     criterion JSON at 0.3% (pass) and 0.6% (fail)"
//!   - nfr-thresholds.md T-1 (0.5% CPU budget per provider)
//!   - architecture.md §7.3 (perf gate)
//!
//! CLI: `parse_threshold <criterion-root> <tick-seconds>`
//!   - criterion-root: path to target/criterion/ (recursive scan for estimate.json)
//!   - tick-seconds: the poll interval used during the bench (default 10s per T-3)
//!
//! Exits 0 if all groups under threshold, 1 if any exceeds.

// This file contains the `main` for the binary. The parse logic lives in
// `lib` form via the `parse_threshold` module below so it can be unit-tested.
// To enable that, sidebar-app must expose this as a library too — see the
// accompanying `src/lib.rs` (added in this same commit; sidebar-app becomes a
// mixed lib+bin crate, which Cargo supports natively without inflating the
// workspace package count).

use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} <criterion-root> <tick-seconds>", args[0]);
        return std::process::ExitCode::from(2);
    }
    let root = PathBuf::from(&args[1]);
    let tick_seconds: f64 = args[2]
        .parse()
        .unwrap_or_else(|e| panic!("tick-seconds must be a number: {e}"));

    match sidebar_app::parse_threshold::evaluate_directory(&root, tick_seconds) {
        Ok(report) => {
            println!("{report}");
            if report.any_exceeded() {
                std::process::ExitCode::from(1)
            } else {
                std::process::ExitCode::from(0)
            }
        }
        Err(e) => {
            eprintln!("parse_threshold error: {e}");
            std::process::ExitCode::from(2)
        }
    }
}
