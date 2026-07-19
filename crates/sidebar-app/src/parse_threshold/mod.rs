//! NFR-1 threshold parser for criterion bench output.
//!
//! Story 0.2 — parses criterion estimate JSON files and reports any group
//! whose mean CPU% exceeds T-1 (0.5%) or T-2 (2.0% aggregate). Cited:
//!   - Story 0.2 TDD contract (parse_threshold unit-tested)
//!   - nfr-thresholds.md T-1 (0.5% CPU budget per provider)
//!   - nfr-thresholds.md T-2 (2.0% CPU budget for the aggregate)
//!   - architecture.md §7.3 (perf gate)
//!
//! ## Criterion JSON shape (estimate.json)
//!
//! ```json
//! {
//!   "mean": { "point_estimate": 1234567.0, "confidence_interval": {
//!     "lower_bound": 1234560.0, "upper_bound": 1234570.0
//!   } },
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
//! The parser subtracts the calibrated idle baseline from this measured value;
//! provider groups MUST be ≤ T-1 (0.5%) and the aggregate group MUST be ≤ T-2
//! (2.0%).

use std::path::Path;

/// T-1 threshold: max CPU% per provider per tick. From nfr-thresholds.md.
pub const T1_MAX_CPU_PERCENT: f64 = 0.5;
/// T-2 threshold: max aggregate CPU% per tick.
pub const T2_MAX_CPU_PERCENT: f64 = 2.0;

/// A single bench group's evaluation result.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupResult {
    /// Bench group name (e.g. "poll_cost/sysinfo").
    pub name: String,
    /// Mean time per iteration, in nanoseconds.
    pub mean_ns: f64,
    /// Computed CPU% given the tick window.
    pub cpu_percent: f64,
    /// True if `cpu_percent` exceeds the applicable T-1 or T-2 threshold.
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
            "parse_threshold report (T-1 = {T1_MAX_CPU_PERCENT}% provider, T-2 = {T2_MAX_CPU_PERCENT}% aggregate):"
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
                "NFR-1 violation: {} group(s) exceeded threshold: {}",
                failures.len(),
                failures
                    .iter()
                    .map(|g| format!("{} ({:.3}%)", g.name, g.cpu_percent))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            write!(
                f,
                "all {} group(s) under T-1/T-2 thresholds",
                self.groups.len()
            )
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

/// Calibration metadata emitted by the poll-cost benchmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    /// Idle host CPU percentage measured before Criterion starts.
    pub idle_cpu_percent: f64,
    /// Criterion groups expected for a complete run.
    pub expected_groups: Vec<String>,
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
/// Returns `ParseError` if the directory cannot be read, no estimate files are
/// found, or any estimate file is malformed. The gate fails closed so a
/// partial or empty Criterion run cannot pass as a clean benchmark.
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
    let calibration_path = criterion_root.join("calibration.txt");
    let calibration = parse_calibration(&std::fs::read_to_string(&calibration_path)?)?;
    let mut groups = Vec::new();
    walk_for_estimates(
        criterion_root,
        criterion_root,
        tick_seconds,
        calibration.idle_cpu_percent,
        &mut groups,
    )?;
    if groups.is_empty() {
        return Err(ParseError::InvalidEstimate(format!(
            "no valid estimate.json files found under {}",
            criterion_root.display()
        )));
    }
    let actual: std::collections::HashSet<&str> = groups.iter().map(|g| g.name.as_str()).collect();
    let missing: Vec<&str> = calibration
        .expected_groups
        .iter()
        .map(String::as_str)
        .filter(|name| !actual.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(ParseError::InvalidEstimate(format!(
            "partial Criterion report; missing expected group(s): {}",
            missing.join(", ")
        )));
    }
    Ok(Report { groups })
}

/// Recursive walker — collects every `estimate.json` under `root`.
fn walk_for_estimates(
    base: &Path,
    current: &Path,
    tick_seconds: f64,
    calibration_percent: f64,
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
            walk_for_estimates(base, &path, tick_seconds, calibration_percent, out)?;
        } else if path.file_name().is_some_and(|n| n == "estimates.json") {
            // Found an estimate.json — parse and evaluate it.
            // The "group name" is the relative path from `base` to the
            // directory containing this estimate.json, minus the trailing
            // "new" component criterion adds.
            let rel = path
                .parent()
                .and_then(|p| p.strip_prefix(base).ok())
                .map_or_else(|| path.clone(), std::path::Path::to_path_buf);
            let name = rel.to_string_lossy().replace('\\', "/");
            let name = name.trim_end_matches("/new").to_string();
            out.push(evaluate_one_file(
                &path,
                &name,
                tick_seconds,
                calibration_percent,
            )?);
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
    calibration_percent: f64,
) -> Result<GroupResult, ParseError> {
    let raw = std::fs::read_to_string(path)?;
    let estimate: Estimate = serde_json::from_str(&raw)?;
    Ok(evaluate_estimate_with_calibration(
        name,
        &estimate,
        tick_seconds,
        calibration_percent,
    ))
}

/// Evaluate a parsed `Estimate` against T-1. Pure function — testable without
/// touching the filesystem.
#[must_use]
pub fn evaluate_estimate(name: &str, estimate: &Estimate, tick_seconds: f64) -> GroupResult {
    evaluate_estimate_with_calibration(name, estimate, tick_seconds, 0.0)
}

/// Evaluate a parsed `Estimate` after subtracting the measured idle CPU.
#[must_use]
pub fn evaluate_estimate_with_calibration(
    name: &str,
    estimate: &Estimate,
    tick_seconds: f64,
    calibration_percent: f64,
) -> GroupResult {
    let mean_ns = estimate.mean.point_estimate;
    let raw_cpu_percent = if tick_seconds > 0.0 {
        (mean_ns / 1_000_000_000.0) / tick_seconds * 100.0
    } else {
        f64::INFINITY
    };
    let cpu_percent = raw_cpu_percent - calibration_percent;
    let threshold = if name == "aggregate" || name.ends_with("/aggregate") {
        T2_MAX_CPU_PERCENT
    } else {
        T1_MAX_CPU_PERCENT
    };
    GroupResult {
        name: name.to_string(),
        mean_ns,
        cpu_percent,
        exceeded: cpu_percent > threshold,
    }
}

/// Parse the calibration marker written by the poll-cost benchmark.
pub fn parse_calibration(contents: &str) -> Result<Calibration, ParseError> {
    let mut idle_cpu_percent = None;
    let mut expected_groups = Vec::new();
    for line in contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = line.strip_prefix("calibration_idle_cpu_percent=") {
            let value = value.parse::<f64>().map_err(|_| {
                ParseError::InvalidEstimate(format!("invalid calibration value: {value}"))
            })?;
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err(ParseError::InvalidEstimate(format!(
                    "calibration must be finite and between 0 and 100: {value}"
                )));
            }
            idle_cpu_percent = Some(value);
        } else if let Some(group) = line.strip_prefix("expected_group=") {
            if group.is_empty() || expected_groups.iter().any(|g| g == group) {
                return Err(ParseError::InvalidEstimate(
                    "calibration expected_group is empty or duplicated".to_string(),
                ));
            }
            expected_groups.push(group.to_string());
        }
    }
    let idle_cpu_percent = idle_cpu_percent.ok_or_else(|| {
        ParseError::InvalidEstimate(
            "calibration_idle_cpu_percent field is missing from calibration.txt".to_string(),
        )
    })?;
    let has_provider = expected_groups
        .iter()
        .any(|group| group.starts_with("poll_cost/provider/"));
    let has_aggregate = expected_groups
        .iter()
        .any(|group| group.ends_with("/aggregate"));
    if !has_provider || !has_aggregate {
        return Err(ParseError::InvalidEstimate(
            "calibration must list expected provider groups and poll_cost/aggregate".to_string(),
        ));
    }
    Ok(Calibration {
        idle_cpu_percent,
        expected_groups,
    })
}

/// Construct an `Estimate` from a mean point estimate (ns) — convenience for
/// unit tests.
#[must_use]
pub fn make_estimate(mean_ns: u64) -> Estimate {
    Estimate {
        mean: PointEstimate {
            #[allow(clippy::cast_precision_loss)]
            point_estimate: mean_ns as f64,
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
    pub point_estimate: f64,
    /// Optional confidence interval; absent in some criterion versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_interval: Option<ConfidenceInterval>,
}

/// Confidence interval bounds (criterion shape).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ConfidenceInterval {
    /// Lower bound of the confidence interval (nanoseconds).
    pub lower_bound: f64,
    /// Upper bound of the confidence interval (nanoseconds).
    pub upper_bound: f64,
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

    #[test]
    fn empty_criterion_directory_fails_closed() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = tmp.path().join("criterion");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(
            root.join("calibration.txt"),
            "calibration_idle_cpu_percent=0\nexpected_group=poll_cost/provider/sysinfo\nexpected_group=poll_cost/aggregate\n",
        )
        .expect("calibration");
        let result = evaluate_directory(&root, 10.0);
        assert!(matches!(result, Err(ParseError::InvalidEstimate(_))));
    }

    #[test]
    fn malformed_estimate_fails_closed() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let estimate = tmp.path().join("criterion").join("poll_cost").join("new");
        std::fs::create_dir_all(&estimate).expect("mkdir");
        std::fs::write(
            tmp.path().join("criterion").join("calibration.txt"),
            "calibration_idle_cpu_percent=0\nexpected_group=poll_cost/provider/sysinfo\nexpected_group=poll_cost/aggregate\n",
        )
        .expect("calibration");
        std::fs::write(estimate.join("estimates.json"), "not json").expect("write");
        let result = evaluate_directory(tmp.path().join("criterion").as_path(), 10.0);
        assert!(matches!(result, Err(ParseError::Json(_))));
    }

    #[test]
    fn calibration_parser_reads_idle_field_and_expected_groups() {
        let calibration = parse_calibration(
            "calibration_idle_cpu_percent=1.250\nexpected_group=poll_cost/provider/sysinfo\nexpected_group=poll_cost/aggregate\n",
        )
        .expect("valid calibration");
        assert!((calibration.idle_cpu_percent - 1.25).abs() < f64::EPSILON);
        assert_eq!(calibration.expected_groups.len(), 2);
    }

    #[test]
    fn calibration_missing_idle_field_fails_closed() {
        let result = parse_calibration("expected_group=poll_cost/aggregate\n");
        assert!(matches!(result, Err(ParseError::InvalidEstimate(_))));
    }

    #[test]
    fn aggregate_uses_t2_threshold() {
        let pass = evaluate_estimate("poll_cost/aggregate", &make_estimate(190_000_000), 10.0);
        let fail = evaluate_estimate("poll_cost/aggregate", &make_estimate(210_000_000), 10.0);
        assert!(!pass.exceeded, "1.9% should be under T-2");
        assert!(fail.exceeded, "2.1% should exceed T-2");
    }

    #[test]
    fn calibration_is_subtracted_from_cpu_percent() {
        let result = evaluate_estimate_with_calibration(
            "poll_cost/provider/sysinfo",
            &make_estimate(80_000_000),
            10.0,
            0.25,
        );
        assert!((result.cpu_percent - 0.55).abs() < 0.001);
        assert!(result.exceeded);
    }

    #[test]
    fn partial_criterion_report_fails_closed() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = tmp.path().join("criterion");
        let provider_dir = root
            .join("poll_cost")
            .join("provider")
            .join("sysinfo")
            .join("new");
        std::fs::create_dir_all(&provider_dir).expect("mkdir provider");
        std::fs::write(
            provider_dir.join("estimates.json"),
            serde_json::to_string(&make_estimate(10_000_000)).expect("estimate"),
        )
        .expect("write estimate");
        std::fs::write(
            root.join("calibration.txt"),
            "calibration_idle_cpu_percent=0\nexpected_group=poll_cost/provider/sysinfo\nexpected_group=poll_cost/provider/net\nexpected_group=poll_cost/aggregate\n",
        )
        .expect("calibration");
        let result = evaluate_directory(&root, 10.0);
        assert!(
            matches!(result, Err(ParseError::InvalidEstimate(message)) if message.contains("partial Criterion report"))
        );
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

    #[test]
    fn real_criterion_float_estimate_parses() {
        let json = r#"{
            "mean": {
                "point_estimate": 123.45,
                "confidence_interval": {
                    "confidence_level": 0.95,
                    "lower_bound": 120.10,
                    "upper_bound": 126.80
                }
            }
        }"#;
        let estimate: Estimate = serde_json::from_str(json).expect("real Criterion shape");
        assert!((estimate.mean.point_estimate - 123.45).abs() < 1e-9);
        let interval = estimate
            .mean
            .confidence_interval
            .as_ref()
            .expect("confidence interval");
        assert!((interval.lower_bound - 120.10).abs() < 1e-9);
        assert!((interval.upper_bound - 126.80).abs() < 1e-9);
        let result = evaluate_estimate("poll_cost/provider/sysinfo", &estimate, 10.0);
        assert!((result.mean_ns - 123.45).abs() < 1e-9);
    }

    #[test]
    fn integer_criterion_estimate_still_parses() {
        let json = r#"{"mean":{"point_estimate":1234567}}"#;
        let estimate: Estimate = serde_json::from_str(json).expect("integer Criterion shape");
        assert!((estimate.mean.point_estimate - 1_234_567.0).abs() < f64::EPSILON);
    }

    /// Integration: write a synthetic criterion-style directory tree to a
    /// temp dir (fixture F1) and verify evaluate_directory picks it up.
    #[test]
    fn evaluate_directory_walks_subdirs() {
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("criterion");
        let group_dir = root
            .join("poll_cost")
            .join("provider")
            .join("sysinfo")
            .join("new");
        std::fs::create_dir_all(&group_dir).expect("mkdir");
        std::fs::write(
            root.join("calibration.txt"),
            "calibration_idle_cpu_percent=0\nexpected_group=poll_cost/provider/sysinfo\nexpected_group=poll_cost/aggregate\n",
        )
        .expect("calibration");
        let estimate = make_estimate(60_000_000); // 0.6% — exceeds
        let json = serde_json::to_string(&estimate).expect("serialize");
        std::fs::write(group_dir.join("estimates.json"), json).expect("write");
        let aggregate_dir = root.join("poll_cost").join("aggregate").join("new");
        std::fs::create_dir_all(&aggregate_dir).expect("mkdir aggregate");
        std::fs::write(
            aggregate_dir.join("estimates.json"),
            serde_json::to_string(&make_estimate(100_000_000)).expect("serialize aggregate"),
        )
        .expect("write aggregate");

        let report = evaluate_directory(&root, 10.0).expect("eval");
        assert_eq!(
            report.groups.len(),
            2,
            "provider and aggregate groups should be found"
        );
        assert!(report.any_exceeded());
        let name = &report
            .groups
            .iter()
            .find(|group| group.name.contains("sysinfo"))
            .expect("provider group")
            .name;
        assert!(
            name.contains("sysinfo"),
            "group name should contain bench name, got: {name}"
        );
    }
}
