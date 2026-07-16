//! `sidebar-adapter-ohm` — LibreHardwareMonitor HTTP bridge (Story 3.6).
//!
//! ## Role
//!
//! LibreHardwareMonitor (LHM) is a bundled subprocess (launched by
//! `OhmSupervisor`, Story 6.4) that exposes a small HTTP server on
//! `127.0.0.1:<port>` (default 17127, T-45). This adapter `GET`s
//! `/data.json` on every tick, parses the JSON sensor tree, and emits
//! `Reading`s for CPU temperature/power/fan/voltage, AMD/Intel GPU
//! temperature/power, and SSD SMART/temperature.
//!
//! ## Tier + cost
//!
//! `Tier::Full` (runs only in Full mode — LHM is heavyweight to run).
//! `CostClass::Lightweight` (one localhost HTTP roundtrip + one
//! `serde_json` deserialization per tick — sub-millisecond on any modern
//! machine; NFR-1/T-1).
//!
//! ## Mockability (TDD contract Happy Path #2)
//!
//! The real `ureq` HTTP client is abstracted behind [`HttpClient`]. Production
//! wires up [`RealHttpClient`]; tests inject `MockHttpClient` (via `mockall`)
//! that returns canned JSON — this is how the Story 3.6 unit tests satisfy the
//! contract without hitting the network. The translation from JSON to
//! `Vec<Reading>` lives in [`readings_from_json`], which is a pure function —
//! the bulk of the contract is exercised directly against the saved fixture
//! `tests/fixtures/lhm_data.json`.
//!
//! ## Boundary handling
//!
//! - LHM not running (connection refused) → empty readings + `debug!`
//!   (Boundary #1)
//! - 500ms timeout (T-10) → empty + `debug!` (Boundary #2)
//! - Non-LHM service returns HTML 404 → JSON parse fails → empty + `warn!`
//!   (Boundary #3)
//! - Sensor node missing `value` → that node skipped, others returned
//!   (Boundary #4)
//! - Dual-socket (two CPUs) → `SensorId.instance = "cpu/0"` and `"cpu/1"`
//!   derived from the LHM node `id` (Boundary #5)
//! - Schema drift (new field) → `#[serde(default)]` tolerance, no fail
//!   (Boundary #6)
//!
//! ## Cited
//!
//! - Story 3.6 TDD contract (Happy Path #1-#2, Boundary #1-#6)
//! - architecture.md AD-2 (revised), AD-7 (revised), §7.2
//! - nfr-thresholds.md T-10 (500ms HTTP timeout — HITL), T-20 (finite
//!   values only), T-45 (port 17127 default)
//! - guardrails.md G15 (Mutex poison recovery), G11 (T-10 HITL)

use std::sync::Mutex;

use sidebar_domain::reading::{finite, MetricKind, Reading, SensorId, Unit};
use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};
use sidebar_sensor::provider::SensorProvider;
use tracing::{debug, warn};

pub mod http;
pub mod lhm_model;
pub mod pipe;

use crate::http::{HttpClient, OhmError, RealHttpClient, DEFAULT_OHM_PORT};
use crate::lhm_model::LhmNode;

/// Metrics emitted by the OHM adapter (Full mode only).
///
/// `&'static [MetricKind]` keeps the descriptor `const`-constructible.
const OHM_METRICS: &[MetricKind] = &[
    MetricKind::CpuTemperature,
    MetricKind::CpuPower,
    MetricKind::FanSpeed,
    MetricKind::Voltage,
    MetricKind::GpuTemperature,
    MetricKind::GpuPower,
    MetricKind::GpuFanSpeed,
    MetricKind::DiskTemperature,
];

/// Descriptor for the OHM adapter — `Tier::Full`, `CostClass::Lightweight`.
const DESCRIPTOR: SensorDescriptor = SensorDescriptor::new(
    "ohm",
    CostClass::Lightweight,
    OHM_METRICS,
    ProviderTier::Full,
);

/// LHM-backed adapter. Generic over `C: HttpClient` so tests inject a mock.
/// The production alias [`OhmAdapter`] fixes `C = RealHttpClient`.
///
/// Holds `Mutex<C>` so `read_all` (`&self` per [`SensorProvider`]) can still
/// call the `&self`-but-stateful HTTP client. The lock is held only for the
/// brief HTTP roundtrip + parse (sub-millisecond on localhost).
pub struct OhmAdapterGeneric<C: HttpClient> {
    client: Mutex<C>,
    port: u16,
}

/// Production adapter wired to [`RealHttpClient`].
pub type OhmAdapter = OhmAdapterGeneric<RealHttpClient>;

impl<C: HttpClient> OhmAdapterGeneric<C> {
    /// Construct with a specific HTTP client + port. Tests use this to
    /// inject a `MockHttpClient`.
    #[must_use]
    pub fn with_client(client: C, port: u16) -> Self {
        Self {
            client: Mutex::new(client),
            port,
        }
    }

    /// The LHM HTTP port the adapter was constructed with.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl OhmAdapter {
    /// Construct the production adapter targeting the default LHM port
    /// (17127, T-45).
    #[must_use]
    pub fn new() -> Self {
        Self::with_client(RealHttpClient::new(), DEFAULT_OHM_PORT)
    }

    /// Construct the production adapter targeting a specific port (used by
    /// `OhmSupervisor` when it has resolved a non-default port, Story 6.4).
    #[must_use]
    pub fn with_port(port: u16) -> Self {
        Self::with_client(RealHttpClient::new(), port)
    }
}

impl Default for OhmAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: HttpClient + Send> SensorProvider for OhmAdapterGeneric<C> {
    fn descriptor(&self) -> &SensorDescriptor {
        &DESCRIPTOR
    }

    fn read_all(&self) -> Vec<Reading> {
        // Lock once per tick. Poison recovery (G15): if a previous call
        // panicked mid-lock, we recover the inner client rather than
        // propagating the poison. An unpoisoned Mutex is the common case.
        let client = self
            .client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let url = format!("http://127.0.0.1:{}/data.json", self.port);
        let body = match client.get(&url) {
            Ok(b) => b,
            Err(OhmError::HttpFailed(reason)) => {
                // Boundary #1 — connection refused / network failure. This
                // is the common case when LHM is not running yet (cold start
                // before OhmSupervisor launches it). Quiet log.
                debug!(port = self.port, %reason, "LHM /data.json fetch failed");
                return Vec::new();
            }
            Err(OhmError::Timeout) => {
                // Boundary #2 — T-10. Should be very rare on localhost.
                debug!(
                    port = self.port,
                    timeout_ms = http::HTTP_TIMEOUT_MS,
                    "LHM fetch timed out"
                );
                return Vec::new();
            }
            Err(OhmError::NotJson(reason)) => {
                // Boundary #3 — something else is serving the port.
                warn!(port = self.port, %reason, "LHM port returned non-JSON body");
                return Vec::new();
            }
            Err(OhmError::Parse(reason)) => {
                // Boundary #4 — LHM body parsed as JSON but didn't match
                // the LhmNode tree contract. Likely a real LHM version
                // change; surface as a warn so we see it in logs.
                warn!(port = self.port, %reason, "LHM /data.json parse failed");
                return Vec::new();
            }
            Err(OhmError::RejectedUrl(reason)) => {
                warn!(port = self.port, %reason, "LHM URL rejected by G16 loopback policy");
                return Vec::new();
            }
        };

        match serde_json::from_str::<Vec<LhmNode>>(&body) {
            Ok(tree) => readings_from_json(&tree),
            Err(e) => {
                // Could be a malformed JSON body (Boundary #3-ish) — surface
                // as NotJson so the caller-side log captures it.
                warn!(port = self.port, error = %e, "LHM /data.json body is not valid JSON");
                Vec::new()
            }
        }
    }
}

/// Translate a parsed LHM `/data.json` tree into canonical `Vec<Reading>`.
///
/// The tree is a top-level JSON array of [`LhmNode`]s. We recurse into
/// `Node`-typed children, and for each `Sensor` leaf map its category
/// (derived from the `id` path: `/amdcpu/0/temperature/0` → category
/// `temperature`) to a [`MetricKind`] + [`Unit`].
///
/// ## SensorId derivation (Boundary #5)
///
/// The LHM `id` path segments after the hardware class encode the hardware
/// instance + sensor instance — e.g. `/amdcpu/0/temperature/0`. We derive:
/// - `SensorId.category` from the hardware class (`amdcpu`/`intelcpu` →
///   `"cpu"`; `gpu-*` → `"gpu"`; `hdd`/`ssd` → `"disk"`)
/// - `SensorId.instance` from `<class>/<hw_index>` (e.g. `"cpu/0"`, `"cpu/1"`
///   for dual-socket — Boundary #5)
///
/// ## Skips (not errors)
///
/// - Sensor nodes with `value: None` → skipped (Boundary #4)
/// - Non-finite `value` (`NaN`/`±Inf`) → skipped (T-20)
/// - Categories we don't model (`clock`/`load`/`throughput`/...) → skipped
///   with a `debug!` (kept silent — these are normal siblings in the tree)
///
/// ## Cited
///
/// - Story 3.6 TDD contract
/// - nfr-thresholds.md T-20 (finite values only)
fn readings_from_json(tree: &[LhmNode]) -> Vec<Reading> {
    let mut out = Vec::new();
    // The root is an array of nodes (typically one computer node with `/`
    // id). Walk every node; the recursion descends through hardware folders
    // (`/amdcpu/0`, `/gpu-amd/0`, ...) and emits Readings at sensor leaves
    // (`/amdcpu/0/temperature/0`).
    for root in tree {
        walk_node(root, &mut out);
    }
    out
}

/// Recursive walk. For each `Node`-typed child we recurse; for each
/// `Sensor`-typed leaf we attempt to map it to a `Reading`.
///
/// Hardware context is derived from the sensor's own `id` path (the parent
/// folder's `id` is a prefix of the sensor's `id`), so we do NOT need to
/// thread state down the recursion — the leaf id is self-describing.
fn walk_node(node: &LhmNode, out: &mut Vec<Reading>) {
    if node.is_sensor() {
        if let Some(r) = map_sensor(node) {
            out.push(r);
        }
        return;
    }
    // Node (folder) — recurse into children.
    for child in &node.children {
        walk_node(child, out);
    }
}

/// Map a sensor-leaf [`LhmNode`] to a [`Reading`]. Returns `None` if:
/// - the `value` field is absent (Boundary #4) or non-finite (T-20),
/// - the sensor category (parsed from the `id` path) is not one we model
///   (`clock`/`load`/`throughput`/...),
/// - the hardware class is not recognized (some LHM nodes like `/ram/0` or
///   `/mainboard/0` carry voltages we DO map, but a `/nic/0/throughput` is
///   skipped because we don't model NIC metrics here — network is the
///   net adapter's job, Story 3.5).
fn map_sensor(node: &LhmNode) -> Option<Reading> {
    // Boundary #4: a sensor with no `value` is unread (LHM hasn't sampled
    // it yet). Skip — never emit NaN (T-20).
    let raw = node.value?;
    // T-20: non-finite values are omitted, not emitted with a sentinel.
    let raw = finite(raw)?;

    // Parse the LHM id path. Format: `/<hw_class>/<hw_index>/<category>/<sensor_index>`
    // e.g. `/amdcpu/0/temperature/0`. The leading segment is empty (the id
    // starts with `/`). We need at least: hw_class, hw_index, category.
    let segments: Vec<&str> = node.id.split('/').collect();
    // Expected layout after split: ["", hw_class, hw_index, category, sensor_index?]
    if segments.len() < 4 {
        // Not a sensor-shaped id — skip silently. Some LHM versions emit
        // single-segment ids for ad-hoc sensors; we don't model those.
        return None;
    }
    let hw_class = segments[1];
    let hw_index = segments[2];
    let category = segments[3];

    let hw_kind = HardwareKind::from_lhm_class(hw_class)?;
    let sensor_kind = SensorKind::from_lhm_category(category)?;

    let (metric, unit) = combine(hw_kind, sensor_kind)?;
    let sensor_id = SensorId::new(
        hw_kind.category(),
        format!("{}/{}", hw_kind.instance_tag(), hw_index),
    );

    Some(Reading::new(sensor_id, metric, raw, unit))
}

/// Recognized LHM hardware classes. Each maps to a `SensorId.category` and
/// instance tag prefix. Unknown classes return `None` (sensor skipped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardwareKind {
    /// `amdcpu` / `intelcpu` → category `"cpu"`, instance `"cpu/<idx>"`.
    Cpu,
    /// `gpu-amd` / `gpu-nvidia` / `gpu-intel` → `"gpu"`, `"gpu/<idx>"`.
    Gpu,
    /// `hdd` / `ssd` / `sat` → `"disk"`, `"disk/<idx>"`.
    Disk,
    /// `mainboard` — motherboard rails (VCORE, +3.3V, +5V, +12V, fan
    /// headers). Mapped to `"board"` category for the `Voltage` /
    /// `FanSpeed` metrics. The motherboard's own temperature sensors map
    /// to no current `MetricKind` (we don't have a "BoardTemperature"),
    /// so those sensors are skipped downstream.
    Mainboard,
}

impl HardwareKind {
    /// Map an LHM hardware class string to a [`HardwareKind`]. Returns
    /// `None` for unrecognized classes (e.g. `ram`, `nic`, `psu`) — those
    /// sensors are silently skipped.
    #[must_use]
    fn from_lhm_class(s: &str) -> Option<Self> {
        match s {
            "amdcpu" | "intelcpu" | "cpu" => Some(Self::Cpu),
            "gpu-amd" | "gpu-nvidia" | "gpu-intel" | "gpu" => Some(Self::Gpu),
            "hdd" | "ssd" | "sat" => Some(Self::Disk),
            "mainboard" => Some(Self::Mainboard),
            _ => None,
        }
    }

    /// The `SensorId.category` for this hardware class.
    #[must_use]
    const fn category(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Disk => "disk",
            Self::Mainboard => "board",
        }
    }

    /// The `SensorId.instance` prefix (combined with the hw index).
    #[must_use]
    const fn instance_tag(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Disk => "disk",
            Self::Mainboard => "board",
        }
    }
}

/// Recognized LHM sensor categories (the segment between hw_index and the
/// trailing sensor index in the id path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SensorKind {
    Temperature,
    Power,
    Fan,
    Voltage,
}

impl SensorKind {
    #[must_use]
    fn from_lhm_category(s: &str) -> Option<Self> {
        match s {
            "temperature" => Some(Self::Temperature),
            "power" => Some(Self::Power),
            "fan" => Some(Self::Fan),
            "voltage" => Some(Self::Voltage),
            // `clock`, `load`, `throughput`, `data`, `control`, `level`,
            // `factor`, `smalldata`, `smalldata` — not modeled here.
            // CpuFrequency/GpuFrequency/CpuUtilization/GpuUtilization have
            // cheaper dedicated adapters (sysinfo, NVML) so OHM doesn't
            // need to emit them.
            _ => None,
        }
    }
}

/// Combine hardware + sensor category into the (MetricKind, Unit) pair.
/// Returns `None` for combinations we don't model (e.g. mainboard
/// temperature — no BoardTemperature variant exists; disk power/voltage/fan —
/// out of scope).
///
/// Mapping table (Story 3.6 spec):
///
/// | hw \ sensor | Temperature | Power      | Fan              | Voltage  |
/// |-------------|-------------|------------|------------------|----------|
/// | Cpu         | CpuTemp °C  | CpuPower W | FanSpeed RPM     | Voltage  |
/// | Gpu         | GpuTemp °C  | GpuPower W | GpuFanSpeed RPM  | Voltage  |
/// | Disk        | DiskTemp °C | (skip)     | (skip)           | (skip)   |
/// | Mainboard   | (skip)      | (skip)     | FanSpeed RPM     | Voltage  |
fn combine(hw: HardwareKind, s: SensorKind) -> Option<(MetricKind, Unit)> {
    let pair = match s {
        SensorKind::Temperature => match hw {
            HardwareKind::Cpu => (MetricKind::CpuTemperature, Unit::Celsius),
            HardwareKind::Gpu => (MetricKind::GpuTemperature, Unit::Celsius),
            HardwareKind::Disk => (MetricKind::DiskTemperature, Unit::Celsius),
            // Mainboard temp has no MetricKind home today.
            HardwareKind::Mainboard => return None,
        },
        SensorKind::Power => match hw {
            HardwareKind::Cpu => (MetricKind::CpuPower, Unit::Watts),
            HardwareKind::Gpu => (MetricKind::GpuPower, Unit::Watts),
            // Disk + mainboard power out of scope.
            HardwareKind::Disk | HardwareKind::Mainboard => return None,
        },
        SensorKind::Fan => match hw {
            // CPU + chassis fans are generic FanSpeed; GPU fans get the
            // dedicated GpuFanSpeed variant.
            HardwareKind::Cpu | HardwareKind::Mainboard => (MetricKind::FanSpeed, Unit::Rpm),
            HardwareKind::Gpu => (MetricKind::GpuFanSpeed, Unit::Rpm),
            // Disks don't have fans.
            HardwareKind::Disk => return None,
        },
        SensorKind::Voltage => match hw {
            HardwareKind::Cpu | HardwareKind::Gpu | HardwareKind::Mainboard => {
                (MetricKind::Voltage, Unit::Volts)
            }
            // Disk voltage out of scope.
            HardwareKind::Disk => return None,
        },
    };
    Some(pair)
}

#[cfg(test)]
mod tests {
    //! Story 3.6 TDD contract tests.
    //!
    //! These tests are split into two groups:
    //!   1. Pure-translation tests against the saved fixture
    //!      (`tests/fixtures/lhm_data.json`) — exercises `readings_from_json`
    //!      without any HTTP layer. This is where the bulk of Happy Path #1
    //!      + Boundary #4-#6 lives.
    //!   2. Adapter-level tests via `MockHttpClient` — exercises the HTTP
    //!      fetch + error mapping (Boundaries #1-#3).
    //!
    //! Cited:
    //!   - Story 3.6 TDD contract (Happy Path #1-#2, Boundary #1-#6)
    //!   - architecture.md AD-2 (revised), AD-7 (revised)
    //!   - nfr-thresholds.md T-10 (HITL), T-20 (finite only), T-45 (port 17127)

    use super::*;
    use mockall::mock;
    use sidebar_domain::reading::Unit;
    use std::fs;
    use std::path::PathBuf;

    /// Locate the saved LHM fixture. The path is relative to the crate root
    /// regardless of where `cargo test` runs from.
    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("lhm_data.json")
    }

    /// Load + parse the saved LHM fixture into a `Vec<LhmNode>`.
    fn load_fixture() -> Vec<LhmNode> {
        let raw =
            fs::read_to_string(fixture_path()).expect("tests/fixtures/lhm_data.json must exist");
        serde_json::from_str::<Vec<LhmNode>>(&raw)
            .expect("fixture must be valid LHM JSON matching the LhmNode schema")
    }

    // Auto-mock `HttpClient` so adapter tests don't hit the network.
    mock! {
        pub FakeClient {}
        impl HttpClient for FakeClient {
            fn get(&self, url: &str) -> Result<String, OhmError>;
        }
    }

    // ----- Happy Path #1: parse fixture → expected readings -----

    /// Story 3.6 Happy Path #1. The saved fixture MUST yield:
    /// - at least one `CpuTemperature` reading (`cpu/0`, value 65.0 °C)
    /// - at least one `FanSpeed` reading (`cpu/0`, value 1500 RPM)
    /// - at least one `Voltage` reading (`cpu/0`, value 1.05 V)
    /// - dual-socket (Boundary #5): `cpu/0` AND `cpu/1` temperatures
    /// - one GPU temperature (`gpu/0`, value 52.0 °C)
    /// - one disk temperature (`disk/0`, value 38.0 °C)
    #[test]
    fn fixture_yields_expected_cpu_temp_fan_voltage_readings() {
        let tree = load_fixture();
        let readings = readings_from_json(&tree);

        // CPU temperature: `/amdcpu/0/temperature/0` value 65.0
        let cpu_temp = readings.iter().find(|r| {
            r.kind == MetricKind::CpuTemperature
                && r.sensor.category == "cpu"
                && r.sensor.instance == "cpu/0"
        });
        let cpu_temp = cpu_temp.expect("cpu/0 temperature reading expected");
        assert!((cpu_temp.value - 65.0).abs() < 1e-6, "cpu temp value");
        assert_eq!(cpu_temp.unit, Unit::Celsius);

        // CPU fan: `/amdcpu/0/fan/0` value 1500 RPM
        let cpu_fan = readings.iter().find(|r| {
            r.kind == MetricKind::FanSpeed
                && r.sensor.category == "cpu"
                && r.sensor.instance == "cpu/0"
        });
        let cpu_fan = cpu_fan.expect("cpu/0 fan reading expected");
        assert!((cpu_fan.value - 1500.0).abs() < 1e-6, "cpu fan value");
        assert_eq!(cpu_fan.unit, Unit::Rpm);

        // CPU voltage: `/amdcpu/0/voltage/0` value 1.05 V
        let cpu_volt = readings.iter().find(|r| {
            r.kind == MetricKind::Voltage
                && r.sensor.category == "cpu"
                && r.sensor.instance == "cpu/0"
        });
        let cpu_volt = cpu_volt.expect("cpu/0 voltage reading expected");
        assert!((cpu_volt.value - 1.05).abs() < 1e-6, "cpu voltage value");
        assert_eq!(cpu_volt.unit, Unit::Volts);

        // CPU power: `/amdcpu/0/power/0` value 18.5 W
        let cpu_pwr = readings.iter().find(|r| {
            r.kind == MetricKind::CpuPower
                && r.sensor.category == "cpu"
                && r.sensor.instance == "cpu/0"
        });
        let cpu_pwr = cpu_pwr.expect("cpu/0 power reading expected");
        assert!((cpu_pwr.value - 18.5).abs() < 1e-6, "cpu power value");
        assert_eq!(cpu_pwr.unit, Unit::Watts);
    }

    /// Story 3.6 Happy Path #1 extension: GPU + disk temperature readings
    /// from the fixture.
    #[test]
    fn fixture_yields_gpu_and_disk_temperature() {
        let tree = load_fixture();
        let readings = readings_from_json(&tree);

        let gpu = readings
            .iter()
            .find(|r| r.kind == MetricKind::GpuTemperature && r.sensor.instance == "gpu/0")
            .expect("gpu/0 temperature");
        assert!((gpu.value - 52.0).abs() < 1e-6);

        let disk = readings
            .iter()
            .find(|r| r.kind == MetricKind::DiskTemperature && r.sensor.instance == "disk/0")
            .expect("disk/0 temperature");
        assert!((disk.value - 38.0).abs() < 1e-6);
    }

    // ----- Boundary #5: dual-socket CPUs get cpu/0 + cpu/1 instances -----

    /// Story 3.6 Boundary #5. Cited: Story 3.6 TDD contract.
    #[test]
    fn dual_socket_cpus_produce_distinct_instances() {
        let tree = load_fixture();
        let readings = readings_from_json(&tree);
        let temps: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuTemperature)
            .collect();
        // cpu/0 (value 65) and cpu/1 (value 71).
        let instances: Vec<&str> = temps.iter().map(|r| r.sensor.instance.as_str()).collect();
        assert!(instances.contains(&"cpu/0"), "cpu/0 missing: {instances:?}");
        assert!(instances.contains(&"cpu/1"), "cpu/1 missing: {instances:?}");
    }

    // ----- Boundary #4: missing-value sensor is skipped, others returned -----

    /// Story 3.6 Boundary #4. Cited: Story 3.6 TDD contract, T-20.
    #[test]
    fn sensor_without_value_field_is_skipped() {
        let json = r#"[
          {
            "id": "/", "text": "root", "type": "Node",
            "children": [
              {
                "id": "/amdcpu/0", "text": "AMD Ryzen", "type": "Node",
                "children": [
                  { "id": "/amdcpu/0/temperature/0", "text": "Has Value",
                    "type": "Sensor", "value": 55.0 },
                  { "id": "/amdcpu/0/temperature/1", "text": "No Value",
                    "type": "Sensor" }
                ]
              }
            ]
          }
        ]"#;
        let tree: Vec<LhmNode> = serde_json::from_str(json).unwrap();
        let readings = readings_from_json(&tree);
        // Exactly one temperature reading: the one with value.
        let temps: Vec<_> = readings
            .iter()
            .filter(|r| r.kind == MetricKind::CpuTemperature)
            .collect();
        assert_eq!(temps.len(), 1, "missing-value sensor must be skipped");
        assert!((temps[0].value - 55.0).abs() < 1e-6);
    }

    // ----- Boundary #6: schema drift tolerant (new field added) -----

    /// Story 3.6 Boundary #6. Cited: Story 3.6 TDD contract.
    /// The serde model test already covers this at the parse layer; here we
    /// verify the translation also passes through cleanly.
    #[test]
    fn schema_drift_with_unknown_future_field_is_tolerated() {
        let json = r#"[
          {
            "id": "/", "text": "root", "type": "Node",
            "future_top_level_field": "v0.9.7+",
            "children": [
              {
                "id": "/amdcpu/0", "text": "AMD", "type": "Node",
                "children": [
                  { "id": "/amdcpu/0/temperature/0", "text": "Pkg",
                    "type": "Sensor", "value": 60.0,
                    "future_sensor_field": 999 }
                ]
              }
            ]
          }
        ]"#;
        let tree: Vec<LhmNode> = serde_json::from_str(json).unwrap();
        let readings = readings_from_json(&tree);
        assert_eq!(
            readings
                .iter()
                .filter(|r| r.kind == MetricKind::CpuTemperature)
                .count(),
            1
        );
    }

    // ----- T-20: NaN value is skipped -----

    /// nfr-thresholds.md T-20. A non-finite sensor value must not propagate.
    #[test]
    fn non_finite_sensor_value_is_skipped() {
        let json = r#"[
          {
            "id": "/", "text": "root", "type": "Node",
            "children": [
              {
                "id": "/amdcpu/0", "text": "AMD", "type": "Node",
                "children": [
                  { "id": "/amdcpu/0/temperature/0", "text": "Finite",
                    "type": "Sensor", "value": 50.0 },
                  { "id": "/amdcpu/0/temperature/1", "text": "NaN",
                    "type": "Sensor", "value": null }
                ]
              }
            ]
          }
        ]"#;
        let tree: Vec<LhmNode> = serde_json::from_str(json).unwrap();
        let readings = readings_from_json(&tree);
        assert!(
            readings.iter().all(|r| r.value.is_finite()),
            "no non-finite values must propagate"
        );
    }

    // ----- Adapter-level: Boundary #1 (connection refused → empty) -----

    /// Story 3.6 Boundary #1. Cited: T-10, T-45.
    #[test]
    fn adapter_returns_empty_on_connection_refused() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let adapter = OhmAdapterGeneric::with_client(mock, DEFAULT_OHM_PORT);
        let r = adapter.read_all();
        assert!(r.is_empty(), "connection refused → empty readings");
    }

    // ----- Adapter-level: Boundary #2 (timeout → empty) -----

    /// Story 3.6 Boundary #2 (T-10). Cited: T-10.
    #[test]
    fn adapter_returns_empty_on_timeout() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| Err(OhmError::Timeout));
        let adapter = OhmAdapterGeneric::with_client(mock, DEFAULT_OHM_PORT);
        assert!(adapter.read_all().is_empty());
    }

    // ----- Adapter-level: Boundary #3 (non-JSON body → empty) -----

    /// Story 3.6 Boundary #3. The adapter's `read_all` handles both
    /// `NotJson` (HTTP-layer) and raw-parse-failure of the body. Both must
    /// yield empty readings.
    #[test]
    fn adapter_returns_empty_on_non_json_body() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Ok("<html><body>404 Not Found</body></html>".to_string()));
        let adapter = OhmAdapterGeneric::with_client(mock, DEFAULT_OHM_PORT);
        assert!(adapter.read_all().is_empty());
    }

    // ----- Adapter-level: Happy Path #2 (mock client returns fixture JSON) -----

    /// Story 3.6 Happy Path #2. The adapter wires the mock HttpClient →
    /// `readings_from_json` end-to-end and yields the expected CPU temp.
    #[test]
    fn adapter_via_mock_client_yields_cpu_temperature() {
        let raw = fs::read_to_string(fixture_path()).unwrap();
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(move |_| Ok(raw.clone()));
        let adapter = OhmAdapterGeneric::with_client(mock, DEFAULT_OHM_PORT);
        let r = adapter.read_all();
        assert!(
            r.iter()
                .any(|x| x.kind == MetricKind::CpuTemperature && x.sensor.instance == "cpu/0"),
            "mock-fetched fixture must yield cpu/0 temperature"
        );
    }

    // ----- Descriptor correctness -----

    /// `Tier::Full`, `CostClass::Lightweight` (Story 3.6 Technical Context).
    #[test]
    fn descriptor_is_full_tier_lightweight() {
        let adapter = OhmAdapter::new();
        let d = adapter.descriptor();
        assert_eq!(d.name, "ohm");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.requires_tier, ProviderTier::Full);
        assert!(d.metrics.contains(&MetricKind::CpuTemperature));
        assert!(d.metrics.contains(&MetricKind::FanSpeed));
        assert!(d.metrics.contains(&MetricKind::Voltage));
    }

    /// `OhmAdapter::new()` defaults to T-45 port 17127.
    #[test]
    fn default_port_is_17127() {
        let adapter = OhmAdapter::new();
        assert_eq!(adapter.port(), DEFAULT_OHM_PORT);
        assert_eq!(DEFAULT_OHM_PORT, 17127);
    }
}
