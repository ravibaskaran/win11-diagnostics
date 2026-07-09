//! Story 1.5 — Config schema + TOML (de)serialization + migration.
//!
//! The full sidebar config with all sections per Story 1.5 Technical Context.
//! Lives at `%APPDATA%\sidebar\config.toml`.
//!
//! Cited: Story 1.5, architecture.md AD-9, nfr-thresholds.md T-3/T-21/T-22.

use crate::billing::CycleStartDay;
use crate::format::TempUnit;
use serde::{Deserialize, Serialize};

// ===========================================================================
// Config — top-level struct.
// ===========================================================================

/// The full sidebar configuration.
///
/// Serialized as TOML at `%APPDATA%\sidebar\config.toml`.
/// Versioned with `config_version` for future migrations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Schema version for migrations.
    #[serde(default = "default_config_version")]
    pub config_version: u32,

    /// Whether the first-run wizard has completed.
    #[serde(default)]
    pub first_run_complete: bool,

    /// Poll interval in seconds (T-3: 1–60, default 10).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u32,

    /// Display settings (NFR-8).
    #[serde(default)]
    pub display: DisplayConfig,

    /// Bandwidth tracking settings.
    #[serde(default)]
    pub bandwidth: BandwidthConfig,

    /// Process list settings (T-21).
    #[serde(default)]
    pub process: ProcessConfig,

    /// Sparkline graph settings (T-22).
    #[serde(default)]
    pub graph: GraphConfig,

    /// Theme settings (T-35).
    #[serde(default)]
    pub theme: ThemeConfig,

    /// Dock settings (T-36, NFR-6/NFR-7).
    #[serde(default)]
    pub dock: DockConfig,

    /// LHM subprocess settings (T-45).
    #[serde(default)]
    pub ohm: OhmConfig,

    /// Threshold alert settings.
    #[serde(default)]
    pub thresholds: ThresholdConfig,

    /// Global hotkey settings (T-34).
    #[serde(default)]
    pub hotkeys: HotkeyConfig,

    /// Per-metric enable/disable + reorder.
    #[serde(default)]
    pub metrics: MetricsConfig,
}

/// Display settings (NFR-8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Temperature unit (T-29: Celsius default).
    #[serde(default = "default_temp_unit")]
    pub temp_unit: TempUnit,

    /// Show raw values (Hz/bytes/bps) instead of human-readable (T-28).
    #[serde(default)]
    pub raw_values: bool,

    /// Use decimal GB (10^9) vs binary GiB (2^30) (T-28).
    #[serde(default = "default_decimal_base")]
    pub decimal_base: bool,
}

/// Bandwidth tracking settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Billing-cycle start day (T-26).
    #[serde(default = "default_cycle_start_day")]
    pub cycle_start_day: CycleStartDaySerde,

    /// LUIDs to track (empty = all non-loopback).
    #[serde(default)]
    pub tracked_luids: Vec<u64>,
}

/// Process list settings (T-21).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Number of top processes to show (T-21: 1–50, default 5).
    #[serde(default = "default_top_n")]
    pub top_n: usize,
}

/// Sparkline graph settings (T-22).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphConfig {
    /// Rolling window size (T-22: 10–600, default 60).
    #[serde(default = "default_graph_window")]
    pub window: usize,
}

/// Theme settings (T-35).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Theme mode: "Dark", "Light", or "System".
    #[serde(default = "default_theme_mode")]
    pub mode: String,

    /// Accent color hex (T-35).
    #[serde(default = "default_accent")]
    pub accent: String,
}

/// Dock settings (T-36, NFR-6/NFR-7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockConfig {
    /// Docked edge: "Left", "Right", "Top", "Bottom".
    #[serde(default = "default_dock_edge")]
    pub edge: String,

    /// Target monitor ID (T-36: DeviceID or "primary").
    #[serde(default = "default_monitor_id")]
    pub monitor_id: String,

    /// Pixel offset from screen edge.
    #[serde(default)]
    pub offset_px: i32,
}

/// LHM subprocess settings (T-45).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OhmConfig {
    /// HTTP port for LHM (T-45: default 17127).
    #[serde(default = "default_ohm_port")]
    pub http_port: u16,

    /// Whether Full mode is enabled (auto-detect may flip this).
    #[serde(default)]
    pub enabled: bool,
}

/// Threshold alert settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// CPU temperature warning threshold (°C).
    #[serde(default = "default_cpu_temp_warn")]
    pub cpu_temp_warn: f64,

    /// CPU temperature critical threshold (°C).
    #[serde(default = "default_cpu_temp_crit")]
    pub cpu_temp_critical: f64,

    /// GPU temperature warning threshold (°C).
    #[serde(default = "default_gpu_temp_warn")]
    pub gpu_temp_warn: f64,

    /// GPU temperature critical threshold (°C).
    #[serde(default = "default_gpu_temp_crit")]
    pub gpu_temp_critical: f64,
}

/// Global hotkey settings (T-34).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Click-through toggle hotkey (default "Ctrl+Shift+S").
    #[serde(default = "default_click_through_hotkey")]
    pub click_through: String,
}

/// Per-metric enable/disable + reorder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MetricsConfig {
    /// Enabled metric names (MetricKind variants as strings).
    #[serde(default)]
    pub enabled: Vec<String>,

    /// Display order (metric names in sequence).
    #[serde(default)]
    pub order: Vec<String>,
}

// ===========================================================================
// Serde-compatible wrapper for CycleStartDay.
// ===========================================================================

/// TOML-compatible representation of CycleStartDay.
///
/// Serializes as `{ Day = 7 }` or `"LastDayOfMonth"` in TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CycleStartDaySerde {
    /// Start on a specific day-of-month (1–28).
    Day(u8),
    /// Start on the last day of the month.
    LastDayOfMonth,
}

impl Default for CycleStartDaySerde {
    fn default() -> Self {
        Self::Day(1)
    }
}

impl From<CycleStartDay> for CycleStartDaySerde {
    fn from(d: CycleStartDay) -> Self {
        match d {
            CycleStartDay::Day(n) => Self::Day(n),
            CycleStartDay::LastDayOfMonth => Self::LastDayOfMonth,
        }
    }
}

impl From<&CycleStartDaySerde> for CycleStartDay {
    fn from(s: &CycleStartDaySerde) -> Self {
        match s {
            CycleStartDaySerde::Day(n) => Self::day(*n),
            CycleStartDaySerde::LastDayOfMonth => Self::LastDayOfMonth,
        }
    }
}

// ===========================================================================
// Defaults
// ===========================================================================

fn default_config_version() -> u32 {
    1
}
fn default_poll_interval() -> u32 {
    10
}
fn default_temp_unit() -> TempUnit {
    TempUnit::Celsius
}
fn default_decimal_base() -> bool {
    true
}
fn default_cycle_start_day() -> CycleStartDaySerde {
    CycleStartDaySerde::Day(1)
}
fn default_top_n() -> usize {
    5
}
fn default_graph_window() -> usize {
    60
}
fn default_theme_mode() -> String {
    "Dark".into()
}
fn default_accent() -> String {
    "#4CAF50".into()
}
fn default_dock_edge() -> String {
    "Right".into()
}
fn default_monitor_id() -> String {
    "primary".into()
}
fn default_ohm_port() -> u16 {
    17127
}
fn default_cpu_temp_warn() -> f64 {
    80.0
}
fn default_cpu_temp_crit() -> f64 {
    95.0
}
fn default_gpu_temp_warn() -> f64 {
    80.0
}
fn default_gpu_temp_crit() -> f64 {
    95.0
}
fn default_click_through_hotkey() -> String {
    "Ctrl+Shift+S".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: default_config_version(),
            first_run_complete: false,
            poll_interval_seconds: default_poll_interval(),
            display: DisplayConfig::default(),
            bandwidth: BandwidthConfig::default(),
            process: ProcessConfig::default(),
            graph: GraphConfig::default(),
            theme: ThemeConfig::default(),
            dock: DockConfig::default(),
            ohm: OhmConfig::default(),
            thresholds: ThresholdConfig::default(),
            hotkeys: HotkeyConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            temp_unit: default_temp_unit(),
            raw_values: false,
            decimal_base: default_decimal_base(),
        }
    }
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            cycle_start_day: default_cycle_start_day(),
            tracked_luids: Vec::new(),
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            top_n: default_top_n(),
        }
    }
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            window: default_graph_window(),
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: default_theme_mode(),
            accent: default_accent(),
        }
    }
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            edge: default_dock_edge(),
            monitor_id: default_monitor_id(),
            offset_px: 0,
        }
    }
}

impl Default for OhmConfig {
    fn default() -> Self {
        Self {
            http_port: default_ohm_port(),
            enabled: false,
        }
    }
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            cpu_temp_warn: default_cpu_temp_warn(),
            cpu_temp_critical: default_cpu_temp_crit(),
            gpu_temp_warn: default_gpu_temp_warn(),
            gpu_temp_critical: default_gpu_temp_crit(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            click_through: default_click_through_hotkey(),
        }
    }
}

// ===========================================================================
// Config methods
// ===========================================================================

impl Config {
    /// Parse a TOML string into a Config. Unknown fields are ignored
    /// (forward-compat). Missing fields get their defaults.
    ///
    /// # Errors
    ///
    /// Returns `crate::error::Error::Toml` if the TOML is malformed.
    pub fn from_toml_str(s: &str) -> Result<Self, crate::error::Error> {
        let config: Self = toml::from_str(s)?;
        Ok(config.clamp_values())
    }

    /// Serialize to a TOML string.
    ///
    /// # Errors
    ///
    /// Returns `crate::error::Error::TomlSerialize` on serialization failure.
    pub fn to_toml_string(&self) -> Result<String, crate::error::Error> {
        Ok(toml::to_string(self)?)
    }

    /// Clamp out-of-range values to their documented bounds + log warnings.
    fn clamp_values(self) -> Self {
        let mut config = self;
        if config.poll_interval_seconds < 1 {
            tracing::warn!(
                value = config.poll_interval_seconds,
                "poll_interval_seconds < 1; clamping to 1"
            );
            config.poll_interval_seconds = 1;
        }
        if config.poll_interval_seconds > 60 {
            tracing::warn!(
                value = config.poll_interval_seconds,
                "poll_interval_seconds > 60; clamping to 60"
            );
            config.poll_interval_seconds = 60;
        }
        if config.process.top_n < 1 {
            tracing::warn!(value = config.process.top_n, "top_n < 1; clamping to 1");
            config.process.top_n = 1;
        }
        if config.process.top_n > 50 {
            tracing::warn!(value = config.process.top_n, "top_n > 50; clamping to 50");
            config.process.top_n = 50;
        }
        if config.graph.window < 10 {
            tracing::warn!(
                value = config.graph.window,
                "graph.window < 10; clamping to 10"
            );
            config.graph.window = 10;
        }
        if config.graph.window > 600 {
            tracing::warn!(
                value = config.graph.window,
                "graph.window > 600; clamping to 600"
            );
            config.graph.window = 600;
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_documented_values() {
        let c = Config::default();
        assert_eq!(c.config_version, 1);
        assert!(!c.first_run_complete);
        assert_eq!(c.poll_interval_seconds, 10);
        assert_eq!(c.display.temp_unit, TempUnit::Celsius);
        assert!(!c.display.raw_values);
        assert!(c.display.decimal_base);
        assert_eq!(c.process.top_n, 5);
        assert_eq!(c.graph.window, 60);
        assert_eq!(c.theme.mode, "Dark");
        assert_eq!(c.theme.accent, "#4CAF50");
        assert_eq!(c.dock.edge, "Right");
        assert_eq!(c.dock.monitor_id, "primary");
        assert_eq!(c.ohm.http_port, 17127);
        assert!(!c.ohm.enabled);
        assert_eq!(c.hotkeys.click_through, "Ctrl+Shift+S");
    }

    #[test]
    fn round_trip_through_toml() {
        let original = Config::default();
        let toml_str = original.to_toml_string().unwrap();
        let parsed = Config::from_toml_str(&toml_str).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn poll_interval_zero_clamps_to_one() {
        let toml_str = "poll_interval_seconds = 0";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.poll_interval_seconds, 1);
    }

    #[test]
    fn poll_interval_999_clamps_to_60() {
        let toml_str = "poll_interval_seconds = 999";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.poll_interval_seconds, 60);
    }

    #[test]
    fn missing_sections_use_defaults() {
        // Empty TOML → all defaults.
        let config = Config::from_toml_str("").unwrap();
        assert_eq!(config.poll_interval_seconds, 10);
        assert_eq!(config.display.temp_unit, TempUnit::Celsius);
        assert_eq!(config.theme.mode, "Dark");
        assert_eq!(config.ohm.http_port, 17127);
    }

    #[test]
    fn unknown_field_ignored() {
        let toml_str = "unknown_field = 42\npoll_interval_seconds = 5";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.poll_interval_seconds, 5);
    }

    #[test]
    fn top_n_zero_clamps_to_one() {
        let toml_str = "[process]\ntop_n = 0";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.process.top_n, 1);
    }

    #[test]
    fn top_n_999_clamps_to_50() {
        let toml_str = "[process]\ntop_n = 999";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.process.top_n, 50);
    }

    #[test]
    fn ohm_http_port_configurable() {
        let toml_str = "[ohm]\nhttp_port = 8085";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.ohm.http_port, 8085);
    }

    #[test]
    fn bandwidth_cycle_start_day_serde() {
        let toml_str = "[bandwidth]\ncycle_start_day = { Day = 15 }";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::Day(15)
        );
    }

    #[test]
    fn bandwidth_last_day_of_month() {
        let toml_str = "[bandwidth]\ncycle_start_day = \"LastDayOfMonth\"";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::LastDayOfMonth
        );
    }

    #[test]
    fn theme_accent_color() {
        let toml_str = "[theme]\naccent = \"#FF0000\"";
        let config = Config::from_toml_str(toml_str).unwrap();
        assert_eq!(config.theme.accent, "#FF0000");
    }

    #[test]
    fn first_run_flag_defaults_false() {
        let config = Config::default();
        assert!(!config.first_run_complete);
    }
}
