//! Story 10.1 — provider CPU-cost bench (T-1/T-2/F9).

#![allow(missing_docs)]

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use sidebar_app::provider_registry::build_registry;
use sidebar_sensor::classifier::ActiveTier;
use sysinfo::System;

fn measure_idle_cpu_percent() -> f64 {
    let mut system = System::new();
    system.refresh_cpu_usage();
    std::thread::sleep(Duration::from_mins(1));
    system.refresh_cpu_usage();
    let idle = f64::from(system.global_cpu_usage());
    assert!(
        idle.is_finite() && (0.0..=100.0).contains(&idle),
        "T-31 idle CPU calibration unavailable or invalid: {idle}"
    );
    idle
}

fn poll_cost(c: &mut Criterion) {
    let providers = build_registry(ActiveTier::Basic);
    let path = std::path::Path::new("target/criterion/calibration.txt");
    if let Err(error) = std::fs::remove_file(path) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "cannot clear stale T-31 calibration: {error}"
        );
    }
    // T-31 requires a real 60-second idle baseline. Never emit a fabricated
    // marker: a failed measurement aborts the bench and the parser fails
    // closed when calibration.txt is absent.
    let calibration = measure_idle_cpu_percent();
    let mut metadata = format!("calibration_idle_cpu_percent={calibration:.3}\n");
    for provider in &providers {
        let _ = writeln!(
            metadata,
            "expected_group=poll_cost/provider/{}",
            provider.descriptor().name
        );
    }
    metadata.push_str("expected_group=poll_cost/aggregate\n");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("cannot create T-31 calibration directory: {error}"));
    }
    std::fs::write(path, &metadata)
        .unwrap_or_else(|error| panic!("cannot persist T-31 calibration: {error}"));
    print!("{metadata}");
    let mut group = c.benchmark_group("poll_cost");
    for provider in &providers {
        let name = provider.descriptor().name;
        group.bench_function(BenchmarkId::new("provider", name), |b| {
            b.iter(|| black_box(provider.read_all()));
        });
    }
    group.bench_function("aggregate", |b| {
        b.iter(|| {
            let readings: Vec<_> = providers.iter().flat_map(|p| p.read_all()).collect();
            black_box(readings);
        });
    });
    group.finish();
}

criterion_group!(benches, poll_cost);
criterion_main!(benches);
