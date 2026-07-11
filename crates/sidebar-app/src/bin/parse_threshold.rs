//! Story 0.2 — NFR-1 threshold parser for criterion bench output.
//!
//! Reads a criterion estimate JSON file (the `*.json` files criterion writes
//! under `target/criterion/<group>/<benchmark>/new/estimate.json`), extracts
//! the mean `typical` value (nanoseconds), subtracts the T-31 idle calibration,
//! and exits non-zero if any provider exceeds T-1 (0.5%) or the aggregate
//! exceeds T-2 (2.0%).
//!
//! Cited:
//!   - Story 0.2 TDD contract: "parse_threshold.rs unit-tested with synthetic
//!     criterion JSON at 0.3% (pass) and 0.6% (fail)"
//!   - nfr-thresholds.md T-1 (0.5% CPU budget per provider)
//!   - nfr-thresholds.md T-2 (2.0% CPU budget for the aggregate)
//!   - architecture.md §7.3 (perf gate)
//!
//! CLI: `parse_threshold <criterion-root> <tick-seconds>`
//!   - criterion-root: path to target/criterion/ (recursive scan for estimate.json)
//!   - tick-seconds: the poll interval used during the bench (default 10s per T-3)
//!   - criterion-root/calibration.txt: required T-31 marker emitted by poll_cost
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
    let tick_seconds = match parse_tick_seconds(&args[2]) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("parse_threshold error: {error}");
            return std::process::ExitCode::from(2);
        }
    };

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

fn parse_tick_seconds(value: &str) -> Result<f64, String> {
    let tick_seconds = value
        .parse::<f64>()
        .map_err(|error| format!("tick-seconds must be a number: {error}"))?;
    if !tick_seconds.is_finite() || tick_seconds <= 0.0 {
        return Err("tick-seconds must be finite and greater than zero".to_string());
    }
    Ok(tick_seconds)
}

#[cfg(test)]
mod tests {
    use super::parse_tick_seconds;

    #[test]
    fn invalid_tick_seconds_returns_error_without_panic() {
        let result = parse_tick_seconds("not-a-number");
        assert!(result.is_err());
    }

    #[test]
    fn non_positive_tick_seconds_returns_error() {
        assert!(parse_tick_seconds("0").is_err());
        assert!(parse_tick_seconds("-1").is_err());
    }

    #[test]
    fn positive_tick_seconds_parses() {
        assert_eq!(parse_tick_seconds("10.5"), Ok(10.5));
    }
}
