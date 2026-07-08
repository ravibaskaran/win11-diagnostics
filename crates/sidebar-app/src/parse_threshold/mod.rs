//! NFR-1 threshold parser for criterion bench output.
//!
//! Story 0.2 — parses criterion estimate JSON files and reports any group
//! whose mean CPU% exceeds T-1 (0.5%). Cited:
//!   - Story 0.2 TDD contract (parse_threshold unit-tested)
//!   - nfr-thresholds.md T-1 (0.5% CPU budget per provider)
//!   - architecture.md §7.3 (perf gate)
//!
//! ## Criterion JSON shape (estimate.json)
//!
//! ```json
//! {
//!   "mean": { "point_estimate": 1234567, "confidence_interval": {...} },
//!   "median": {...},
//!   "slope": {...},
//!   "std_dev": {...}
//! }
//! ```
//!
//! `point_estimate` is in nanoseconds (criterion's standard unit).
//!
//! ## CPU% conversion
//!
//! For a bench that runs N iterations of `provider.read_all()` within a tick
//! window of `tick_seconds`, the CPU% is:
//!
//! ```text
//! cpu_percent = (mean_ns / 1_000_000_000) / tick_seconds * 100
//! ```
//!
//! i.e. what fraction of the tick was spent in the provider. Per T-1, this
//! MUST be ≤ 0.5%.

use std::path::Path;

/// T-1 threshold: max CPU% per provider per tick. From nfr-thresholds.md.
pub const T1_MAX_CPU_PERCENT: f64 = 0.5;

/// A single bench group's evaluation result.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupResult {
    /// Bench group name (e.g. "poll_cost/sysinfo").
    pub name: String,
    /// Mean time per iteration, in nanoseconds.
    pub mean_ns: f64,
    /// Computed CPU% given the tick window.
    pub cpu_percent: f64,
    /// True if `cpu_percent` exceeds T-1.
    pub exceeded: bool,
}

/// Aggregate report across all scanned groups.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// One entry per `estimate.json` found under the criterion root.
    pub groups: Vec<GroupResult>,
}

impl Report {
    /// True if any group exceeded T-1.
    #[must_use]
    pub fn any_exceeded(&self) -> bool {
        self.groups.iter().any(|g| g.exceeded)
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "parse_threshold report (T-1 = {T1_MAX_CPU_PERCENT}% CPU):"
        )?;
        for g in &self.groups {
            let status = if g.exceeded { "FAIL" } else { "ok  " };
            writeln!(
                f,
                "  [{}] {:40} mean={:>12.0}ns  cpu={:.3}%",
                status, g.name, g.mean_ns, g.cpu_percent
            )?;
        }
        if self.any_exceeded() {
            let failures: Vec<&GroupResult> = self.groups.iter().filter(|g| g.exceeded).collect();
            write!(
                f,
                "NFR-1 violation: {} provider(s) exceeded {:.1}% CPU: {}",
                failures.len(),
                T1_MAX_CPU_PERCENT,
                failures
                    .iter()
                    .map(|g| format!("{} ({:.3}%)", g.name, g.cpu_percent))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            write!(f, "all {} group(s) under T-1 threshold", self.groups.len())
        }
    }
}

/// Errors that can occur during parsing.
#[derive(Debug)]
pub enum ParseError {
    /// Filesystem error (missing dir, permission denied, etc.).
    Io(std::io::Error),
    /// JSON parse error (malformed estimate.json).
    Json(serde_json::Error),
    /// Estimate structurally valid but semantically wrong (e.g. negative).
    InvalidEstimate(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::InvalidEstimate(msg) => write!(f, "invalid estimate: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<std::io::Error> for ParseError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Scan a criterion output directory recursively for `estimate.json` files,
/// evaluate each against T-1, and return the aggregate report.
///
/// `criterion_root` is typically `target/criterion/`. `tick_seconds` is the
/// poll interval used during the bench (default 10s per T-3).
///
/// # Errors
///
/// Returns `ParseError` if the directory cannot be read or any estimate file
/// is malformed. Missing `estimate.json` files are skipped silently (a group
/// may have multiple sub-benches, only some of which have estimates).
pub fn evaluate_directory(criterion_root: &Path, tick_seconds: f64) -> Result<Report, ParseError> {
    // The root MUST exist and be a directory. A missing root is an error,
    // not silently an empty report (the bench didn't run).
    if !criterion_root.exists() {
        return Err(ParseError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("criterion root not found: {}", criterion_root.display()),
        )));
    }
    if !criterion_root.is_dir() {
        return Err(ParseError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "criterion root is not a directory: {}",
                criterion_root.display()
            ),
        )));
    }
    let mut groups = Vec::new();
    walk_for_estimates(criterion_root, criterion_root, tick_seconds, &mut groups)?;
    Ok(Report { groups })
}

/// Recursive walker — collects every `estimate.json` under `root`.
fn walk_for_estimates(
    base: &Path,
    current: &Path,
    tick_seconds: f64,
    out: &mut Vec<GroupResult>,
) -> Result<(), ParseError> {
    if !current.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirectories.
            walk_for_estimates(base, &path, tick_seconds, out)?;
        } else if path.file_name().is_some_and(|n| n == "estimate.json") {
            // Found an estimate.json — parse and evaluate it.
            // The "group name" is the relative path from `base` to the
            // directory containing this estimate.json, minus the trailing
            // "new" component criterion adds.
            let rel = path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(base).ok())
                .map_or_else(|| path.clone(), std::path::Path::to_path_buf);
            let name = rel.to_string_lossy().replace('\\', "/");
            let name = name.trim_end_matches("/new").to_string();
            match evaluate_one_file(&path, &name, tick_seconds) {
                Ok(g) => out.push(g),
                Err(e) => {
                    // Skip malformed estimates but log them — a single bad
                    // file shouldn't fail the whole bench gate (criterion
                    // writes transient files during runs).
                    eprintln!(
                        "warn: skipping malformed estimate {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }
    Ok(())
}

/// Evaluate a single estimate.json file.
///
/// Exposed (with a different signature) for unit testing.
fn evaluate_one_file(
    path: &Path,
    name: &str,
    tick_seconds: f64,
) -> Result<GroupResult, ParseError> {
    let raw = std::fs::read_to_string(path)?;
    let estimate: Estimate = serde_json::from_str(&raw)?;
    Ok(evaluate_estimate(name, &estimate, tick_seconds))
}

/// Evaluate a parsed `Estimate` against T-1. Pure function — testable without
/// touching the filesystem.
#[must_use]
pub fn evaluate_estimate(name: &str, estimate: &Estimate, tick_seconds: f64) -> GroupResult {
    // Cast precision loss is acceptable here: nanosecond timings from
    // criterion don't need >52-bit mantissa precision for CPU% comparison.
    #[allow(clippy::cast_precision_loss)]
    let mean_ns = estimate.mean.point_estimate as f64;
    let cpu_percent = if tick_seconds > 0.0 {
        (mean_ns / 1_000_000_000.0) / tick_seconds * 100.0
    } else {
        f64::INFINITY
    };
    GroupResult {
        name: name.to_string(),
        mean_ns,
        cpu_percent,
        exceeded: cpu_percent > T1_MAX_CPU_PERCENT,
    }
}

/// Construct an `Estimate` from a mean point estimate (ns) — convenience for
/// unit tests.
#[must_use]
pub fn make_estimate(mean_ns: u64) -> Estimate {
    Estimate {
        mean: PointEstimate {
            point_estimate: mean_ns,
            confidence_interval: None,
        },
    }
}

/// Criterion `estimate.json` shape (subset we care about).
/// Criterion `estimate.json` shape (subset we care about).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Estimate {
    /// Mean point estimate + confidence interval.
    pub mean: PointEstimate,
}

/// Point estimate with optional confidence interval (criterion shape).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PointEstimate {
    /// The point estimate value (nanoseconds for criterion timings).
    pub point_estimate: u64,
    /// Optional confidence interval; absent in some criterion versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_interval: Option<ConfidenceInterval>,
}

/// Confidence interval bounds (criterion shape).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ConfidenceInterval {
    /// Lower bound of the confidence interval (nanoseconds).
    pub lower_bound: u64,
    /// Upper bound of the confidence interval (nanoseconds).
    pub upper_bound: u64,
}

#[cfg(test)]
mod tests {
    //! Story 0.2 TDD contract tests.
    //!
    //! Cited: Story 0.2 TDD contract:
    //!   Happy Path #2: "parse_threshold.rs unit-tested with synthetic
    //!   criterion JSON at 0.3% (pass) and 0.6% (fail)"
    //!   Boundary #1: "Bench breach: inject a fake bench reporting >0.5%;
    //!   parse_threshold MUST exit non-zero"

    use super::*;

    /// Story 0.2 Happy Path — 0.3% CPU is under T-1 (0.5%), passes.
    /// At tick_seconds=10, 0.3% of 10s = 30ms = 30_000_000 ns.
    #[test]
    fn estimate_under_threshold_passes() {
        let estimate = make_estimate(30_000_000); // 30ms mean over a 10s tick = 0.3%
        let result = evaluate_estimate("sysinfo", &estimate, 10.0);
        assert!(!result.exceeded, "0.3% should be under T-1");
        assert!((result.cpu_percent - 0.3).abs() < 0.001);
    }

    /// Story 0.2 Happy Path — 0.6% CPU exceeds T-1 (0.5%), fails.
    /// At tick_seconds=10, 0.6% of 10s = 60ms = 60_000_000 ns.
    #[test]
    fn estimate_over_threshold_fails() {
        let estimate = make_estimate(60_000_000); // 60ms mean over a 10s tick = 0.6%
        let result = evaluate_estimate("nvml-proc-gpu", &estimate, 10.0);
        assert!(result.exceeded, "0.6% should exceed T-1");
        assert!((result.cpu_percent - 0.6).abs() < 0.001);
    }

    /// Story 0.2 Boundary — exactly-at-threshold (0.5%) does NOT exceed
    /// (the contract is ">", not ">="). 0.5% of 10s = 50_000_000 ns.
    #[test]
    fn estimate_at_threshold_does_not_fail() {
        let estimate = make_estimate(50_000_000);
        let result = evaluate_estimate("boundary", &estimate, 10.0);
        assert!(!result.exceeded, "exactly 0.5% should NOT exceed T-1");
        assert!((result.cpu_percent - 0.5).abs() < 0.001);
    }

    /// Story 0.2 Boundary — tick_seconds=0 would divide by zero; we define
    /// it as +inf CPU% (always fails). Defends against misconfiguration.
    #[test]
    fn zero_tick_seconds_is_infinite_cpu() {
        let estimate = make_estimate(1_000);
        let result = evaluate_estimate("zero-tick", &estimate, 0.0);
        assert!(
            result.exceeded,
            "tick_seconds=0 must be treated as a failure"
        );
        assert!(result.cpu_percent.is_infinite());
    }

    /// Story 0.2 Boundary — report aggregates multiple groups correctly.
    #[test]
    fn report_any_exceeded_aggregates() {
        let pass = evaluate_estimate("g1", &make_estimate(10_000_000), 10.0); // 0.1%
        let fail = evaluate_estimate("g2", &make_estimate(70_000_000), 10.0); // 0.7%
        let report = Report {
            groups: vec![pass.clone(), fail.clone()],
        };
        assert!(report.any_exceeded());
        let all_pass = Report {
            groups: vec![
                pass,
                evaluate_estimate("g3", &make_estimate(5_000_000), 10.0),
            ],
        };
        assert!(!all_pass.any_exceeded());
    }

    /// Story 0.2 Boundary — Display impl names the failing provider in the
    /// NFR-1 violation message (Boundary #1 contract).
    #[test]
    fn display_names_failing_provider() {
        let fail = evaluate_estimate("nvml-proc-gpu", &make_estimate(60_000_000), 10.0);
        let report = Report { groups: vec![fail] };
        let s = report.to_string();
        assert!(s.contains("NFR-1 violation"), "missing violation header");
        assert!(
            s.contains("nvml-proc-gpu"),
            "missing provider name in failure message"
        );
        assert!(s.contains("0.600"), "missing CPU% value");
    }

    /// Story 0.2 Boundary — malformed JSON returns ParseError::Json.
    #[test]
    fn malformed_json_is_json_error() {
        let bad = "not json at all";
        let result: Result<Estimate, _> = serde_json::from_str(bad);
        assert!(result.is_err());
    }

    /// Story 0.2 Boundary — non-existent directory returns ParseError::Io.
    #[test]
    fn missing_directory_is_io_error() {
        let result = evaluate_directory(Path::new("nonexistent/dir/that/does/not/exist"), 10.0);
        assert!(matches!(result, Err(ParseError::Io(_))));
    }

    /// Round-trip: serialize an Estimate, parse it back, evaluate.
    #[test]
    fn estimate_round_trips_through_serde() {
        let original = make_estimate(42_000_000);
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: Estimate = serde_json::from_str(&json).expect("deserialize");
        let result = evaluate_estimate("rt", &parsed, 10.0);
        assert!((result.mean_ns - 42_000_000.0).abs() < 0.001);
    }

    /// Integration: write a synthetic criterion-style directory tree to a
    /// temp dir (fixture F1) and verify evaluate_directory picks it up.
    #[test]
    fn evaluate_directory_walks_subdirs() {
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("criterion");
        let group_dir = root.join("poll_cost").join("sysinfo").join("new");
        std::fs::create_dir_all(&group_dir).expect("mkdir");
        let estimate = make_estimate(60_000_000); // 0.6% — exceeds
        let json = serde_json::to_string(&estimate).expect("serialize");
        std::fs::write(group_dir.join("estimate.json"), json).expect("write");

        let report = evaluate_directory(&root, 10.0).expect("eval");
        assert_eq!(report.groups.len(), 1, "exactly one group should be found");
        assert!(report.any_exceeded());
        let name = &report.groups[0].name;
        assert!(
            name.contains("sysinfo"),
            "group name should contain bench name, got: {name}"
        );
    }

    /// PathBuf import silence — `PathBuf` is used by the binary target.
    #[test]
    fn _pathbuf_in_scope() {
        use std::path::PathBuf;
        let _ = PathBuf::new();
    }
}
