//! `NvmlBackend` trait + `RealNvmlBackend` (nvml-wrapper 0.12 adapter).
//!
//! This module isolates the concrete `nvml_wrapper::Nvml` API behind a trait so
//! the adapter ([`crate::NvmlAdapterGeneric`]) can be unit-tested with a mock.
//! The trait is intentionally small: one method, `refresh_and_snapshot`, that
//! queries the underlying source and returns a plain-data snapshot.
//!
//! ## Why a trait (not `dyn Nvml`)
//!
//! `nvml_wrapper::Nvml` is a concrete struct, not a trait. We cannot mock it
//! directly. Abstracting behind `NvmlBackend` lets `mockall` generate a mock
//! that returns canned [`NvmlSnapshot`]s — this is how the Story 3.2 TDD
//! contract's "mock NVML 42% util, 65°C" tests are satisfied without NVIDIA
//! hardware present.
//!
//! ## NVML-unavailable safety (critical)
//!
//! `Nvml::init()` returns `Err(NvmlError::DriverNotLoaded)` on machines without
//! an NVIDIA driver (e.g. the AMD-Ryzen-AI dev laptop LAPTOP-PLN56DNU).
//! `RealNvmlBackend::new()` MUST NOT panic on this: it eagerly attempts init
//! and stores `Option<Nvml>`. If init failed, `refresh_and_snapshot` returns
//! an empty [`NvmlSnapshot`] and emits a single `debug!` log — the adapter's
//! "NVML init failure → empty readings + flag, don't panic" contract (Story
//! 3.2 Unit Test #2). The init-failure log is emitted exactly once (in
//! `new()`), not on every tick, to avoid log spam.
//!
//! ## nvml-wrapper 0.12 API notes (verified against docs.rs 0.12.0)
//!
//! - `Nvml::init()` → `Result<Nvml, NvmlError>`. Failure modes include
//!   `DriverNotLoaded` (no NVIDIA driver), `LibraryNotFound` (no nvml.dll),
//!   `Unknown`. All non-fatal for us.
//! - `Nvml` is `Send + Sync` (per the struct docs), so it can live behind the
//!   adapter's `Mutex`.
//! - `nvml.device_count()` → `Result<u32, NvmlError>`.
//! - `nvml.device_by_index(i)` → `Result<Device<'_>, NvmlError>` — the
//!   `Device` borrows from `&Nvml`, so we cannot store the `Device`; we
//!   re-acquire per GPU per tick. This is cheap (NVML is a pointer lookup).
//! - `device.utilization_rates()` → `Result<Utilization, NvmlError>` where
//!   `Utilization { gpu: u32, memory: u32 }` — both are percentages 0–100.
//! - `device.temperature(TemperatureSensor::Gpu)` → `Result<u32, NvmlError>`
//!   (Celsius, unsigned). `TemperatureSensor` lives at
//!   `nvml_wrapper::enum_wrappers::device::TemperatureSensor`.
//!
//! ## T-13 timeout (per architecture AD-6 — NOT the adapter's job)
//!
//! NFR-thresholds T-13 says each NVML call should be wrapped in
//! `tokio::time::timeout(100ms, spawn_blocking(...))`. Per architecture AD-6,
//! the **poller** is responsible for the async runtime + `spawn_blocking` +
//! timeout wrapping; the adapter's `read_all` is synchronous (see
//! `SensorProvider::read_all` docs). This adapter therefore performs no
//! timeout enforcement — the poller enforces T-13 around the call. If a real
//! NVML call hangs, the poller's timeout fires and the adapter call is
//! dropped (returning nothing to the GUI for that tick), which matches the
//! T-13 failure action ("return empty readings, log").
//!
//! Cited: Story 3.2 Technical Context, architecture.md §7.2 + AD-6,
//! nfr-thresholds.md T-13 (upstream), T-20 (finite filter).

use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
use nvml_wrapper::Nvml;
use tracing::debug;

/// A plain-data snapshot of everything the NVML adapter needs from one refresh
/// cycle. Translation to `Reading`s happens in [`crate::readings_from_snapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NvmlSnapshot {
    /// Per-GPU readings, positionally indexed (0, 1, 2, ...).
    pub gpus: Vec<GpuSnapshot>,
}

/// Per-GPU snapshot. NVML reports utilization + temperature; v1 (Story 3.2)
/// emits exactly these two metrics. Memory util, power, fan, frequency are
/// later stories.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuSnapshot {
    /// GPU utilization % (0.0–100.0). `f64` for uniformity with the rest of
    /// the codebase (NVML returns u32 percent; we widen).
    pub utilization_pct: f64,
    /// GPU temperature (°C). NVML returns u32 Celsius; we widen to `f64`.
    pub temperature_c: f64,
}

/// Abstraction over the NVML data source. The production impl wraps a real
/// `nvml_wrapper::Nvml`; tests substitute a mock.
///
/// Implementations need NOT be `Send + Sync` themselves — the adapter wraps
/// the backend in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait NvmlBackend {
    /// Refresh the underlying source and return a plain-data snapshot.
    ///
    /// On permanent NVML unavailability (no driver, no library), this returns
    /// an empty [`NvmlSnapshot`] — it MUST NOT panic.
    fn refresh_and_snapshot(&mut self) -> NvmlSnapshot;
}

/// Production backend wrapping a real `nvml_wrapper::Nvml`.
///
/// Holds `Option<Nvml>`: `Some` if `Nvml::init()` succeeded in [`new`], `None`
/// otherwise. When `None`, every `refresh_and_snapshot` returns an empty
/// snapshot — this is the NVML-unavailable contract.
///
/// [`new`]: Self::new
pub struct RealNvmlBackend {
    /// `Some(nvml)` if init succeeded; `None` if init failed (no NVIDIA
    /// driver / library on this machine). Subsequent calls do not retry init
    /// — the absence is a process-lifetime property on consumer hardware.
    nvml: Option<Nvml>,
}

impl RealNvmlBackend {
    /// Construct a backend, eagerly attempting `Nvml::init()`.
    ///
    /// On init failure (no NVIDIA driver, no nvml.dll, etc.) this logs a
    /// single `debug!` and stores `None` — it does NOT panic. Every
    /// `refresh_and_snapshot` thereafter returns empty readings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nvml: Self::try_init(),
        }
    }

    /// Attempt NVML init exactly once, logging the outcome. Kept separate so
    /// tests and `new()` share the exact log wording.
    fn try_init() -> Option<Nvml> {
        match Nvml::init() {
            Ok(nvml) => Some(nvml),
            Err(e) => {
                // Story 3.2 Unit Test #2 contract: init failure → debug!,
                // no panic. We log once at construction (not per-tick) to
                // avoid log spam. The adapter surface still emits an empty
                // snapshot every tick — that's the "empty readings" half.
                debug!(error = %e, "NVML unavailable; adapter will emit empty readings");
                None
            }
        }
    }
}

impl Default for RealNvmlBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NvmlBackend for RealNvmlBackend {
    fn refresh_and_snapshot(&mut self) -> NvmlSnapshot {
        // If init failed at construction, return empty every tick. NVML
        // unavailability is a process-lifetime property on machines without
        // NVIDIA hardware — retrying per-tick wastes cycles and spams logs.
        let Some(nvml) = self.nvml.as_ref() else {
            return NvmlSnapshot::default();
        };

        // device_count can fail (e.g. driver lost between init and now). Treat
        // any failure as "zero GPUs this tick" — partial readings from prior
        // ticks are not carried forward (gauges, not counters — see
        // architecture §5.2).
        let count = match nvml.device_count() {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "nvml.device_count failed; emitting empty snapshot");
                return NvmlSnapshot::default();
            }
        };

        let mut gpus = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
        for index in 0..count {
            // Per-device errors are isolated: a failing device doesn't drop
            // readings from healthy ones (Boundary #4: NVML error mid-poll →
            // partial readings, logged). We push a default snapshot (both
            // fields 0.0) on failure and let the T-20 finite filter in
            // `readings_from_snapshot` drop the zero-valued entries — except
            // 0.0 IS finite, so we instead OMIT the snapshot entirely for a
            // failed device by using `Option` semantics inline.
            match nvml.device_by_index(index) {
                Ok(device) => {
                    let utilization_pct = match device.utilization_rates() {
                        Ok(u) => f64::from(u.gpu),
                        Err(e) => {
                            debug!(index, error = %e, "utilization_rates failed; skipping util");
                            // Use NaN sentinel so the finite filter drops it.
                            f64::NAN
                        }
                    };
                    let temperature_c = match device.temperature(TemperatureSensor::Gpu) {
                        Ok(t) => f64::from(t),
                        Err(e) => {
                            debug!(index, error = %e, "temperature failed; skipping temp");
                            f64::NAN
                        }
                    };
                    // Only record the GPU if at least one of the two metrics
                    // succeeded; a fully-failed device contributes nothing.
                    if utilization_pct.is_finite() || temperature_c.is_finite() {
                        // Replace any NaN with a value the finite filter will
                        // drop. We keep the partial-success metric and lose
                        // the failed one by leaving NaN in place — the
                        // finite filter in `readings_from_snapshot` skips
                        // NaN entries per-metric, not per-GPU.
                        gpus.push(GpuSnapshot {
                            utilization_pct,
                            temperature_c,
                        });
                    }
                }
                Err(e) => {
                    debug!(index, error = %e, "device_by_index failed; skipping GPU");
                }
            }
        }

        NvmlSnapshot { gpus }
    }
}
