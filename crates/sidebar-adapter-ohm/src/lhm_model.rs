//! `LhmNode` serde model for LibreHardwareMonitor's `/data.json` tree.
//!
//! LHM's HTTP server responds to `GET /data.json` with a top-level JSON array
//! of `LhmNode`s. Each node is either a `Node` (folder) with `children`, or a
//! `Sensor` leaf carrying `value` + parent-path `id`. The sensor category
//! (temperature/power/fan/voltage/clock/load) is embedded in the leaf's `id`
//! path — e.g. `/amdcpu/0/temperature/0` → category `temperature` on parent
//! `amdcpu/0`.
//!
//! ## `#[serde(default)]` forward-compat (Boundary #6)
//!
//! Every field is `#[serde(default)]` so an LHM version bump that adds a new
//! field (v0.9.6 → v0.9.7 schema drift) does NOT break parsing. We never
//! `#[serde(deny_unknown_fields)]`. `min`/`value`/`max` are `Option<f64>`
//! because some sensors (e.g. a transiently-unread clock) can omit `value`
//! (Boundary #4: that sensor is skipped, the rest are returned).
//!
//! ## Cited
//!
//! - Story 3.6 TDD contract (Boundary #4 missing-value skip, Boundary #6
//!   schema drift tolerance)
//! - architecture.md AD-2 (revised) — LHM HTTP bridge
//! - HttpServer.cs on LHM master, retrieved 2026-07-08
//! - nfr-thresholds.md T-20 (finite values only — enforced in translation)

use serde::{Deserialize, Serialize};

/// A node in the LibreHardwareMonitor `/data.json` tree.
///
/// `Node` variant has `children`; `Sensor` variant carries `value`. We model
/// both shapes with a single struct (rather than an untagged enum) because the
/// discriminator is the `type` string, and untagged enums suffer from
/// expensive deserialization + poor diagnostics on drift. Each field is
/// `Option`/`#[serde(default)]` for tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LhmNode {
    /// LHM canonical id path, e.g. `/amdcpu/0/temperature/0`. The leading
    /// segments identify the parent hardware (`amdcpu/0`, `gpu-nvidia/1`,
    /// `hdd/0`). The trailing segment (after the last slash) identifies the
    /// sensor instance within its category.
    #[serde(default)]
    pub id: String,

    /// Human-readable label, e.g. "CPU Package", "GPU Temperature".
    #[serde(default)]
    pub text: String,

    /// Children (only populated when `type == "Node"`). Empty for sensor
    /// leaves.
    #[serde(default)]
    pub children: Vec<LhmNode>,

    /// Minimum observed value (sensors only). Absent on folder nodes.
    #[serde(default)]
    pub min: Option<f64>,

    /// Current value (sensors only). Absent on folder nodes, and absent on
    /// sensors that LHM has not yet read. A `None` here causes the sensor to
    /// be skipped (Boundary #4) — never emitted as NaN (T-20).
    #[serde(default)]
    pub value: Option<f64>,

    /// Maximum observed value (sensors only).
    #[serde(default)]
    pub max: Option<f64>,

    /// Icon index for the LHM UI (ignored by this adapter).
    #[serde(default)]
    pub imageindex: Option<i32>,

    /// `"Node"` or `"Sensor"`. Defaults to an empty string when absent
    /// (treated as `Node` with no useful children — skipped silently).
    #[serde(default, rename = "type")]
    pub node_type: String,
}

impl LhmNode {
    /// Returns `true` when this node is a folder (carries children).
    #[inline]
    #[must_use]
    pub fn is_node(&self) -> bool {
        self.node_type.eq_ignore_ascii_case("Node")
    }

    /// Returns `true` when this node is a sensor leaf (carries `value`).
    #[inline]
    #[must_use]
    pub fn is_sensor(&self) -> bool {
        self.node_type.eq_ignore_ascii_case("Sensor")
    }
}

#[cfg(test)]
mod tests {
    //! Boundary #6 (schema drift) + Boundary #4 (missing value) tests on the
    //! model itself. The full adapter contract is exercised in
    //! [`crate::readings_from_json`] tests in lib.rs.

    use super::*;

    /// Boundary #6 (schema drift tolerance). A future LHM version that adds a
    /// field (`maxlifetime_seconds`) MUST parse without error.
    #[test]
    fn lhm_node_tolerates_unknown_future_fields() {
        let json = r#"{
            "id": "/amdcpu/0/temperature/0",
            "text": "CPU Package",
            "type": "Sensor",
            "value": 65.0,
            "maxlifetime_seconds": 12345,
            "vendor_specific_blob": { "whatever": [1, 2, 3] }
        }"#;
        let node: LhmNode = serde_json::from_str(json).expect("unknown fields ignored");
        assert!(node.is_sensor());
        assert_eq!(node.value, Some(65.0));
    }

    /// Boundary #4 (missing value). A sensor without `value` parses; the
    /// translation layer skips it. This test just verifies the model doesn't
    /// error on absence — the skip semantics are tested in lib.rs.
    #[test]
    fn lhm_node_sensor_without_value_parses() {
        let json = r#"{
            "id": "/amdcpu/0/temperature/0",
            "text": "CPU Package",
            "type": "Sensor"
        }"#;
        let node: LhmNode = serde_json::from_str(json).expect("missing value OK at parse time");
        assert!(node.value.is_none());
    }

    /// A folder node (no value) parses with all fields defaulted.
    #[test]
    fn lhm_node_folder_minimal_parses() {
        let json = r#"{ "id": "/amdcpu/0", "text": "AMD Ryzen", "type": "Node" }"#;
        let node: LhmNode = serde_json::from_str(json).expect("folder node minimal");
        assert!(node.is_node());
        assert!(!node.is_sensor());
        assert!(node.children.is_empty());
        assert!(node.value.is_none());
    }
}
