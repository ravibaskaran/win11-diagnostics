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

    /// Story 17.5 — the tier at last shutdown. If "full" and the current
    /// launch is Basic (the elevated child was reaped on crash), the GUI
    /// shows a "click pill to re-enable" message.
    #[serde(default)]
    pub last_tier: String,

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
#[allow(clippy::struct_excessive_bools)]
pub struct DisplayConfig {
    /// Temperature unit (T-29: Celsius default).
    #[serde(default = "default_temp_unit")]
    pub temp_unit: TempUnit,

    /// UI language code (v1.0 parity: `en` default, `es` shipped). The
    /// sidebar-app i18n module parses this into a `Language`; unknown codes
    /// fall back to English.
    #[serde(default = "default_language")]
    pub language: String,

    /// Show raw values (Hz/bytes/bps) instead of human-readable (T-28).
    #[serde(default)]
    pub raw_values: bool,

    /// Use decimal GB (10^9) vs binary GiB (2^30) (T-28).
    #[serde(default = "default_decimal_base")]
    pub decimal_base: bool,

    /// Exclude the sidebar from supported screen-capture APIs (default OFF).
    #[serde(default)]
    pub hide_from_capture: bool,

    /// Force opaque window background (default OFF). Set to true when the
    /// wgpu surface doesn't support CompositeAlphaMode transparency (some
    /// GPU/driver combos on Win11 log a warning + render opaque anyway).
    /// This flag explicitly disables the transparent request so the warning
    /// is suppressed and the window renders cleanly as borderless-opaque.
    #[serde(default)]
    pub force_opaque: bool,

    /// Font size in pixels (v1.0 parity: 10/12/14/16/18 presets, default 14).
    /// Applied as egui's base font size; title/small sizes derive from it.
    /// Stored as u16 so the `as f32` zoom conversion is lossless (u16 fits in
    /// f32's 24-bit mantissa, satisfying clippy::cast_precision_loss).
    #[serde(default = "default_font_size")]
    pub font_size: u16,

    /// Whether alerting metrics blink between normal and alert color (default
    /// ON for accessibility — color alone is insufficient for color-blind
    /// users). v1.0 parity with reference's AlertBlink.
    #[serde(default = "default_alert_blink")]
    pub alert_blink: bool,

    /// Whether sidebar launches on Windows startup (default OFF). v1.0 parity
    /// with reference's RunAtStartup. Implemented via the Windows Run
    /// registry key (HKCU\...\Run) — per-user, no admin required.
    #[serde(default)]
    pub run_at_startup: bool,

    /// UI scale multiplier (v1.0 parity: 0.5–3.0, default 1.0). Composes with
    /// `font_size` so the user can independently tune text size and overall
    /// scale. Stored as a fixed-point u16 (scale × 100) so serde is exact.
    #[serde(default = "default_ui_scale")]
    pub ui_scale_percent: u16,

    /// Sidebar background color as `#RRGGBB` (v1.0 parity: reference BGColor).
    /// Empty string = use theme-derived background (the v1 default).
    #[serde(default)]
    pub bg_color: String,

    /// Background opacity 0–100 (v1.0 parity: reference BGOpacity × 100).
    /// Default 85 (matches reference's 0.85). 100 = fully opaque.
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity_percent: u16,

    /// Font color as `#RRGGBB` (v1.0 parity: reference FontColor). Empty =
    /// use theme-derived text color (the v1 default).
    #[serde(default)]
    pub font_color: String,

    /// Whether to show the per-row 📈 graph-popup buttons (v1.0 UI/UX:
    /// default OFF — the buttons double every row's height which breaks the
    /// glanceable-sidebar mandate. Users who want per-metric graphs enable
    /// this in Settings; the popup is a power-user feature).
    #[serde(default)]
    pub show_graph_buttons: bool,

    /// Whether the sidebar starts hidden on launch (v1.0 parity: reference
    /// InitiallyHidden, default OFF). When true, sidebar launches minimized
    /// to the tray and the user shows it via the tray icon or hotkey.
    #[serde(default)]
    pub initially_hidden: bool,

    /// Whether polling pauses while the sidebar window is hidden (v1.0 parity:
    /// reference pause-when-hidden, default ON to save CPU/battery when the
    /// user has hidden the sidebar).
    #[serde(default = "default_pause_when_hidden")]
    pub pause_when_hidden: bool,
}

/// Bandwidth tracking settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Billing-cycle start day (T-26).
    #[serde(default = "default_cycle_start_day")]
    pub cycle_start_day: CycleStartDaySerde,
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

    /// Pixel offset from screen edge (legacy 1D offset; superseded by
    /// `offset_x_px`/`offset_y_px` for new configs but retained for TOML
    /// back-compat with pre-v1.0 configs).
    #[serde(default)]
    pub offset_px: i32,

    /// Horizontal pixel offset from the docked edge (v1.0 parity: reference
    /// XOffset, range -2000..2000, default 0).
    #[serde(default)]
    pub offset_x_px: i32,

    /// Vertical pixel offset from the docked edge (v1.0 parity: reference
    /// YOffset, range -2000..2000, default 0).
    #[serde(default)]
    pub offset_y_px: i32,

    /// Sidebar window width in pixels (v1.0 parity: 100–300, default 180).
    /// The reference product exposes this as `SidebarWidth`. Stored as u16
    /// so the `as f32` viewport conversion is lossless.
    #[serde(default = "default_sidebar_width")]
    pub width_px: u16,
}

impl DockConfig {
    /// Return true if any field that affects the window's on-screen position
    /// differs between `self` and `other`. Used by the GUI to detect settings-
    /// panel mutations of edge / monitor_id / offset_x / offset_y and re-dock
    /// the window live (v1.0 audit 2 — P1: previously the four controls
    /// silently flipped config with zero visible effect).
    ///
    /// `width_px` is deliberately excluded: it is applied live via a separate
    /// `ViewportCommand::InnerSize` path and does not require re-docking.
    #[must_use]
    pub fn position_changed(&self, other: &Self) -> bool {
        self.edge != other.edge
            || self.monitor_id != other.monitor_id
            || self.offset_x_px != other.offset_x_px
            || self.offset_y_px != other.offset_y_px
    }
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

    /// Drive used-space warning threshold (%, 0 = disabled). v1.0 parity
    /// with reference's `UsedSpaceAlert`. A drive whose used fraction
    /// exceeds this flips to the alert color (and blinks if alert_blink).
    #[serde(default = "default_drive_used_warn")]
    pub drive_used_warn: u32,

    /// Per-NIC inbound bandwidth alert threshold in Mbps (0 = disabled). v1.0
    /// parity with reference's `BandwidthInAlert`. A NIC whose live RX rate
    /// exceeds this flips to the alert color.
    #[serde(default)]
    pub bandwidth_in_alert_mbps: u32,

    /// Per-NIC outbound bandwidth alert threshold in Mbps (0 = disabled). v1.0
    /// parity with reference's `BandwidthOutAlert`.
    #[serde(default)]
    pub bandwidth_out_alert_mbps: u32,
}

/// Global hotkey settings (T-34 + v1.0 parity with reference's 8 hotkeys).
///
/// Each field is a config string like `"Ctrl+Shift+S"` or empty `""` to
/// disable that hotkey. Empty strings are the default for all but
/// `click_through` (which ships enabled) — matching the reference's
/// "all default unbound" posture so the user opts into the ones they want.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Click-through toggle hotkey (default "Ctrl+Shift+S").
    #[serde(default = "default_click_through_hotkey")]
    pub click_through: String,
    /// Toggle sidebar visibility (show if hidden / hide if shown). v1.0 parity.
    #[serde(default)]
    pub toggle_visibility: String,
    /// Show the sidebar (un-hide). v1.0 parity.
    #[serde(default)]
    pub show: String,
    /// Hide the sidebar. v1.0 parity.
    #[serde(default)]
    pub hide: String,
    /// Cycle dock edge (Left→Right→Top→Bottom→Left). v1.0 parity.
    #[serde(default)]
    pub cycle_edge: String,
    /// Cycle target screen (rotate through enumerated monitors). v1.0 parity.
    #[serde(default)]
    pub cycle_screen: String,
    /// Reload settings from config.toml. v1.0 parity.
    #[serde(default)]
    pub reload: String,
    /// Toggle AppBar reserve-space on/off. v1.0 parity.
    #[serde(default)]
    pub toggle_reserve_space: String,
    /// Close the app. v1.0 parity.
    #[serde(default)]
    pub close: String,
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
        match d.day_value() {
            Some(n) => Self::Day(n),
            None => Self::LastDayOfMonth,
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
/// v1.0 parity — drive used-space alert, 90% default.
fn default_drive_used_warn() -> u32 {
    90
}
/// v1.0 parity — font size in px (reference default x14).
fn default_font_size() -> u16 {
    14
}
/// v1.0 parity — alert blink ON by default for accessibility.
fn default_alert_blink() -> bool {
    true
}
/// v1.0 parity — sidebar width default 180px (reference default).
fn default_sidebar_width() -> u16 {
    180
}
/// v1.0 parity — UI scale default 100 (= 1.0× multiplier).
fn default_ui_scale() -> u16 {
    100
}
/// v1.0 parity — background opacity default 85% (reference 0.85).
fn default_bg_opacity() -> u16 {
    85
}
/// v1.0 parity — pause-when-hidden default ON (saves CPU/battery).
fn default_pause_when_hidden() -> bool {
    true
}

/// v1.0 parity — UI language default English (`en`).
fn default_language() -> String {
    "en".to_string()
}
fn default_click_through_hotkey() -> String {
    "Ctrl+Shift+S".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: default_config_version(),
            first_run_complete: false,
            last_tier: String::new(),
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
            language: default_language(),
            raw_values: false,
            decimal_base: default_decimal_base(),
            hide_from_capture: false,
            force_opaque: false,
            font_size: default_font_size(),
            alert_blink: default_alert_blink(),
            run_at_startup: false,
            ui_scale_percent: default_ui_scale(),
            bg_color: String::new(),
            bg_opacity_percent: default_bg_opacity(),
            font_color: String::new(),
            show_graph_buttons: false,
            initially_hidden: false,
            pause_when_hidden: default_pause_when_hidden(),
        }
    }
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            cycle_start_day: default_cycle_start_day(),
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
            offset_x_px: 0,
            offset_y_px: 0,
            width_px: default_sidebar_width(),
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
            drive_used_warn: default_drive_used_warn(),
            bandwidth_in_alert_mbps: 0,
            bandwidth_out_alert_mbps: 0,
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            click_through: default_click_through_hotkey(),
            toggle_visibility: String::new(),
            show: String::new(),
            hide: String::new(),
            cycle_edge: String::new(),
            cycle_screen: String::new(),
            reload: String::new(),
            toggle_reserve_space: String::new(),
            close: String::new(),
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
        // T-26: cycle_start_day Day(d) where d ∉ [1, 28] must clamp + warn
        // The `CycleStartDaySerde` -> `CycleStartDay` -> back round-trip uses
        // the non-panicking `clamped_day` validator so malformed user config
        // is safe in both debug and release builds. LastDayOfMonth passes
        // through unchanged; direct `day()` construction remains strict in
        // debug builds for programmer-facing invariant checks.
        let clamped_day = match config.bandwidth.cycle_start_day {
            CycleStartDaySerde::Day(n) => {
                // Coerce through the non-panicking configuration validator;
                // round-trip back to the serde form (clamped payload).
                CycleStartDaySerde::from(CycleStartDay::clamped_day(n))
            }
            other @ CycleStartDaySerde::LastDayOfMonth => other,
        };
        config.bandwidth.cycle_start_day = clamped_day;
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
        assert!(!c.display.hide_from_capture);
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
    fn hide_from_capture_round_trips_when_enabled() {
        let config = Config::from_toml_str("[display]\nhide_from_capture = true").unwrap();
        assert!(config.display.hide_from_capture);
        let parsed = Config::from_toml_str(&config.to_toml_string().unwrap()).unwrap();
        assert!(parsed.display.hide_from_capture);
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

    // ----- Boundary #4: cycle_start_day out of range clamps to [1, 28] (T-26).

    /// Cited: Story 1.4 Boundary, T-26. TOML deserializes `Day = 29` into
    /// `CycleStartDaySerde::Day(29)` without complaint. `clamp_values` MUST
    /// use the non-panicking configuration validator so the stored value is
    /// `Day(28)` in both debug and release builds.
    #[test]
    fn cycle_start_day_out_of_range_clamps_to_28() {
        // Currently fails: clamp_values doesn't touch cycle_start_day, so
        // the value stays at Day(29) (silent T-26 violation).
        let toml_str = "[bandwidth]\ncycle_start_day = { Day = 29 }";
        let config = Config::from_toml_str(toml_str).expect("must parse");
        assert_eq!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::Day(28),
            "Day(29) must clamp to Day(28) at config load (T-26)"
        );
    }

    /// Cited: T-26 — `Day(0)` clamps UP to `Day(1)` in all build profiles.
    #[test]
    fn cycle_start_day_zero_clamps_to_1() {
        let toml_str = "[bandwidth]\ncycle_start_day = { Day = 0 }";
        let config = Config::from_toml_str(toml_str).expect("must parse");
        assert_eq!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::Day(1),
            "Day(0) must clamp to Day(1) at config load (T-26)"
        );
    }

    /// Cited: T-26 — `LastDayOfMonth` is valid and passes through clamping
    /// unchanged.
    #[test]
    fn cycle_start_day_last_day_of_month_passes_through() {
        let toml_str = "[bandwidth]\ncycle_start_day = \"LastDayOfMonth\"";
        let config = Config::from_toml_str(toml_str).expect("must parse");
        assert_eq!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::LastDayOfMonth
        );
    }

    // ===== v1.0 audit 2 — DockConfig::position_changed =====
    //
    // The bug: settings-panel mutations of edge / monitor_id / offset_x /
    // offset_y flipped config but never called send_dock_position, so the
    // window stayed put. The fix detects the change post-render + re-docks.
    // This test pins the diff helper.

    #[test]
    fn position_changed_false_for_identical_dock() {
        let a = DockConfig::default();
        assert!(!a.position_changed(&DockConfig::default()));
    }

    #[test]
    fn position_changed_true_when_edge_differs() {
        let a = DockConfig::default();
        let mut b = a.clone();
        b.edge = "Top".to_string();
        assert!(a.position_changed(&b));
    }

    #[test]
    fn position_changed_true_when_monitor_id_differs() {
        let a = DockConfig::default();
        let mut b = a.clone();
        b.monitor_id = "DISPLAY2".to_string();
        assert!(a.position_changed(&b));
    }

    #[test]
    fn position_changed_true_when_offset_x_differs() {
        let a = DockConfig::default();
        let mut b = a.clone();
        b.offset_x_px = 42;
        assert!(a.position_changed(&b));
    }

    #[test]
    fn position_changed_true_when_offset_y_differs() {
        let a = DockConfig::default();
        let mut b = a.clone();
        b.offset_y_px = -17;
        assert!(a.position_changed(&b));
    }

    /// width_px changes do NOT count as a position change — width is applied
    /// live via InnerSize and never requires re-docking.
    #[test]
    fn position_changed_false_when_only_width_differs() {
        let a = DockConfig::default();
        let mut b = a.clone();
        b.width_px = 250;
        assert!(!a.position_changed(&b));
    }
}
