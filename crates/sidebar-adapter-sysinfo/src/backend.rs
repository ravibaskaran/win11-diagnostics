//! `SysinfoBackend` trait + `RealSysinfoBackend` (sysinfo 0.39.x adapter).
//!
//! This module isolates the concrete `sysinfo::System` API behind a trait so
//! the adapter ([`crate::SysinfoAdapterGeneric`]) can be unit-tested with a
//! mock. The trait is intentionally small: one method, `refresh_and_snapshot`,
//! that refreshes the underlying source and returns a plain-data snapshot.
//!
//! ## Why a trait (not `dyn sysinfo::System`)
//!
//! `sysinfo::System` is a concrete struct, not a trait. We cannot mock it
//! directly. Abstracting behind `SysinfoBackend` lets `mockall` generate a
//! `MockSysinfoBackend` (well, `MockFakeBackend` in our tests) that returns
//! canned [`SysinfoSnapshot`]s — this is how the Story 3.1 TDD contract's
//! "mock sysinfo 8 cores" tests are satisfied.
//!
//! ## sysinfo 0.39.x API notes
//!
//! - `System::new()` constructs an empty system; the first `refresh_*` call
//!   populates it.
//! - `refresh_cpu()` MUST be called twice on the very first poll to get a
//!   non-zero CPU% (sysinfo measures CPU usage as a delta between two
//!   samples). For unit-test fidelity we don't try to be clever here — the
//!   production caller accepts that the first tick's CPU% may be 0.0.
//! - `refresh_memory()` is single-shot (no delta).
//! - `refresh_processes(ProcessesToUpdate::All)` populates the process map.
//! - `cpus()` → `&[Cpu]` with `.cpu_usage()` (f32) and `.frequency()` (u64 Hz).
//! - `Disks::new_with_refreshed_list()` returns an owned `Disks` iterable.
//!
//! Cited: Story 3.1 Technical Context, architecture.md §7.2.

use sysinfo::{
    CpuRefreshKind, Disk, Disks, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System,
};

/// A plain-data snapshot of everything the sysinfo adapter needs from one
/// refresh cycle. Translation to `Reading`s happens in
/// [`crate::readings_from_snapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SysinfoSnapshot {
    /// Per-core CPU utilization (%) + frequency (Hz). Position = core index.
    pub cpus: Vec<CpuSnapshot>,
    /// RAM used (bytes).
    pub memory_used_bytes: u64,
    /// RAM total (bytes).
    pub memory_total_bytes: u64,
    /// Per-drive snapshots.
    pub disks: Vec<DiskSnapshot>,
    /// Per-process snapshots (already filtered to top-N upstream of the
    /// adapter — v1 emits whatever sysinfo gives us; top-N selection is a
    /// GUI concern per Story 1.6).
    pub processes: Vec<ProcessSnapshot>,
    /// System uptime (seconds).
    pub uptime_seconds: u64,
}

/// Per-core CPU reading pair.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuSnapshot {
    /// Utilization % (0.0–100.0).
    pub cpu_usage: f64,
    /// Frequency (Hz).
    pub frequency: f64,
}

/// Per-drive capacity snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskSnapshot {
    /// Mount point (e.g. `"C:\\"`). Used as the SensorId instance.
    pub mount_point: String,
    /// Used bytes.
    pub used_bytes: u64,
    /// Total bytes.
    pub total_bytes: u64,
}

/// Per-process snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProcessSnapshot {
    /// OS PID.
    pub pid: u32,
    /// CPU % (sysinfo's float).
    pub cpu_percent: f64,
    /// Resident memory (bytes).
    pub memory_bytes: u64,
}

/// Abstraction over the sysinfo data source. The production impl wraps a
/// real `sysinfo::System`; tests substitute a mock.
///
/// Implementations need NOT be `Send + Sync` themselves — the adapter wraps
/// the backend in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait SysinfoBackend {
    /// Refresh the underlying source and return a plain-data snapshot.
    fn refresh_and_snapshot(&mut self) -> SysinfoSnapshot;
}

/// Production backend wrapping a real `sysinfo::System` + `Disks`.
pub struct RealSysinfoBackend {
    sys: System,
    disks: Disks,
}

impl RealSysinfoBackend {
    /// Construct a backend with empty initial state. The first
    /// `refresh_and_snapshot` call populates CPU/memory/processes/disk.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            disks: Disks::new(),
        }
    }
}

impl Default for RealSysinfoBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SysinfoBackend for RealSysinfoBackend {
    fn refresh_and_snapshot(&mut self) -> SysinfoSnapshot {
        // Refresh CPU, memory, and processes in one call. sysinfo 0.39
        // exposes `refresh_specifics` on System for batched refresh.
        self.sys.refresh_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage().with_frequency())
                .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram())
                .with_processes(
                    ProcessRefreshKind::nothing()
                        .with_cpu()
                        .with_memory()
                        .with_user(sysinfo::UpdateKind::Never),
                ),
        );
        // sysinfo 0.39 takes `ProcessesToUpdate::All` for a full refresh.
        // `refresh_specifics` above already does processes; the explicit
        // call below keeps the count limit explicit (0 = all).
        self.sys.refresh_processes(ProcessesToUpdate::All, true);

        // Refresh the disk list (handles hot-plugged USB drives per Boundary #3).
        self.disks.refresh(true);

        let cpus: Vec<CpuSnapshot> = self
            .sys
            .cpus()
            .iter()
            .map(|c| CpuSnapshot {
                cpu_usage: f64::from(c.cpu_usage()),
                // u64 Hz → f64: 2^53 Hz ≈ 9 PB-Hz (nonsensical upper bound);
                // no precision concern for any real CPU frequency.
                frequency: bytes_to_f64(c.frequency()),
            })
            .collect();

        let memory_used_bytes = self.sys.used_memory();
        let memory_total_bytes = self.sys.total_memory();

        let disks: Vec<DiskSnapshot> = self.disks.list().iter().map(disk_to_snapshot).collect();

        let processes: Vec<ProcessSnapshot> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcessSnapshot {
                pid: pid.as_u32(),
                cpu_percent: f64::from(p.cpu_usage()),
                memory_bytes: p.memory(),
            })
            .collect();

        let uptime_seconds = System::uptime();

        SysinfoSnapshot {
            cpus,
            memory_used_bytes,
            memory_total_bytes,
            disks,
            processes,
            uptime_seconds,
        }
    }
}

/// Translate a `sysinfo::Disk` into our plain-data [`DiskSnapshot`].
fn disk_to_snapshot(d: &Disk) -> DiskSnapshot {
    let mount_point = d
        .mount_point()
        .to_str()
        .map(str::to_owned)
        .unwrap_or_default();
    DiskSnapshot {
        mount_point,
        used_bytes: d.total_space().saturating_sub(d.available_space()),
        total_bytes: d.total_space(),
    }
}

/// Convert a `u64` to `f64` for the snapshot fields. `f64` exactly
/// represents integers up to 2^53, so this is loss-free for any realistic
/// byte/Hz/second count. See `lib::bytes_to_f64` for the full rationale.
#[inline]
#[allow(clippy::cast_precision_loss)]
fn bytes_to_f64(v: u64) -> f64 {
    v as f64
}
