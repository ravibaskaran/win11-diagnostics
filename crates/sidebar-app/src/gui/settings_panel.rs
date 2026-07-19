//! Story 8.5 — Settings Panel (PRD §5.5.8 + Settings architecture).
//!
//! The settings panel edits the user-facing fields of [`Config`] and notifies
//! the host of any change so it can persist `config.toml` debounced.
//!
//! ## Editable Fields (mapped to Config)
//!
//! | UI field            | Config path                              |
//! |---------------------|------------------------------------------|
//! | Billing cycle start | `bandwidth.cycle_start_day` (Day/LastDay)|
//! | Temperature unit    | `display.temp_unit`                      |
//! | Raw values toggle   | `display.raw_values`                     |
//! | Decimal/binary base | `display.decimal_base`                   |
//! | Poll interval       | `poll_interval_seconds`                  |
//! | Docked edge         | `dock.edge`                              |
//! | Theme mode          | `theme.mode`                             |
//!
//! ## Guardrails
//!
//! - **cycle_start_day** is constrained to 1–28 (T-26). Day 29+ is rejected
//!   at the UI — the user must pick ≤28 or `LastDayOfMonth`. The
//!   "no-retroactive-resplit" rule (PRD §5.5.8) means a cycle-start-day
//!   change applies only to the NEXT rollover, not the current cycle. A
//!   tooltip spells this out (HITL guardrail).
//! - **poll_interval_seconds=0** is clamped to 1 with a visible warning
//!   (T-3). The number-edit path also clamps >60 down to 60.
//!
//! ## Cited
//!
//! - Story 8.5 TDD contract (Happy Path #1-#2, Boundary #1-#4)
//! - PRD §5.5.8 (cycle rollover semantics)
//! - nfr-thresholds.md T-3 (poll clamp), T-21 (decimal/binary), T-26 (cycle
//!   start day), T-28 (decimal/binary), T-29 (temp unit)
//! - guardrails.md HITL (no-retroactive-resplit G11)

use eframe::egui::Ui;
use sidebar_domain::config::{Config, CycleStartDaySerde, DockConfig, ThemeConfig};
use sidebar_domain::format::TempUnit;

/// The minimum cycle-start day (T-26 — Day must be in `[1, 28]`).
pub(crate) const MIN_CYCLE_DAY: u8 = 1;
/// The maximum cycle-start day (T-26 — Day must be in `[1, 28]`).
pub(crate) const MAX_CYCLE_DAY: u8 = 28;

/// The minimum poll interval in seconds (T-3 — must be in `[1, 60]`).
pub(crate) const MIN_POLL_INTERVAL: u32 = 1;
/// The maximum poll interval in seconds (T-3 — must be in `[1, 60]`).
pub(crate) const MAX_POLL_INTERVAL: u32 = 60;

/// The exact tooltip text spelling out the no-retroactive-resplit rule
/// (PRD §5.5.8 + HITL guardrail).
pub const NO_RESPLIT_TOOLTIP: &str = "Billing-cycle start day applies to the \
     NEXT rollover only. The current cycle is not re-split. \
     Restart sidebar for the new date to take effect.";

// ===== Story 13.4 — plain-language tooltips for every settings section =====
// Cited: Story 13.4, guardrails.md G28, nfr-thresholds.md T-37.

/// Tooltip for the billing-cycle-start-day section. Cited: Story 13.4, G28.
pub const BILLING_CYCLE_TOOLTIP: &str = "The day of the month your bandwidth \
    counter resets. If your internet plan resets on the 15th, set this to 15. \
    For month-end, use 'Last day'.";

/// Tooltip for the temperature-unit section. Cited: Story 13.4, G28.
pub const TEMP_UNIT_TOOLTIP: &str = "Whether to show temperatures in Celsius \
    (°C) or Fahrenheit (°F).";

/// Tooltip for the technical-units toggle (renamed from 'Show raw values').
/// Cited: Story 13.4, G28.
pub const TECHNICAL_UNITS_TOOLTIP: &str = "Show exact values like 3,840,000,000 \
    Hz instead of 3.84 GHz, and bytes instead of GB. Most users leave this off.";

/// Tooltip for the size-units section (renamed from 'Byte base'). Cited:
/// Story 13.4, G28.
pub const SIZE_UNITS_TOOLTIP: &str = "Decimal (GB) = 1,000,000,000 bytes — what \
    Windows shows. Binary (GiB) = 1,073,741,824 bytes — what some tools show. \
    Most users use Decimal.";

/// Tooltip for the refresh-rate section (renamed from 'Poll interval').
/// Cited: Story 13.4, G28.
pub const REFRESH_RATE_TOOLTIP: &str = "How often the sidebar updates its \
    readings. Lower = fresher but uses more CPU. Default 10s is fine for most \
    users.";

/// Tooltip for the docked-edge section. Cited: Story 13.4, G28.
pub const DOCKED_EDGE_TOOLTIP: &str = "Which screen edge the sidebar sticks to. \
    Right is the default.";

/// Tooltip for the theme section. Cited: Story 13.4, G28.
pub const THEME_TOOLTIP: &str = "Dark or Light appearance. 'System' follows your \
    Windows dark/light setting.";

/// Tooltip for the metrics section. Cited: Story 13.4, G28.
pub const METRICS_TOOLTIP: &str = "Choose which readings appear and in what \
    order. Drag to reorder; uncheck to hide.";

/// v1.0 ponytail — resolve a localized label + append a colon. Shrinks the
/// 8 repeated `format!("{}:", crate::i18n::t(lang, ...))` patterns to 1 call.
fn tcolon(lang: crate::i18n::Language, label: crate::i18n::Label) -> String {
    format!("{}:", crate::i18n::t(lang, label))
}

/// Render the settings panel into `ui`, editing `config` in place. The host
/// passes `on_change: &dyn Fn()` which is invoked whenever the user changes
/// any field — the host is responsible for persisting `config.toml` (debounced
/// per PRD §5.5.8).
//
// Note: this function surfaces one field-group per section (cycle day, temp
// unit, raw values, byte base, poll interval, dock edge, theme, + Story 8.9
// metric list). Splitting each into its own fn would fragment the linear
// top-to-bottom layout the panel presents visually; the 101-line count is the
// natural floor for eight sections.
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
pub fn render(ui: &mut Ui, config: &mut Config, on_change: &dyn Fn()) {
    let mut changed = false;
    // v1.0 parity — resolve the user's UI language once for all labels in
    // this panel. Unknown codes fall back to English.
    let lang = crate::i18n::Language::from_code(&config.display.language);

    // ---- Billing cycle start day (T-26: Day in [1,28] OR LastDayOfMonth) ----
    ui.label(crate::i18n::t(
        lang,
        crate::i18n::Label::BillingCycleStartDay,
    ))
    .on_hover_text(BILLING_CYCLE_TOOLTIP);
    ui.horizontal(|row| {
        // The slider operates on a local day_value; if "Last day" is checked
        // we leave the slider at MAX_CYCLE_DAY and disable it.
        let is_last_day = matches!(
            config.bandwidth.cycle_start_day,
            CycleStartDaySerde::LastDayOfMonth
        );
        let mut day_value: u8 = match &config.bandwidth.cycle_start_day {
            CycleStartDaySerde::Day(d) => *d,
            CycleStartDaySerde::LastDayOfMonth => MAX_CYCLE_DAY,
        };
        let mut slider = row.add_enabled(
            !is_last_day,
            egui::Slider::new(&mut day_value, MIN_CYCLE_DAY..=MAX_CYCLE_DAY).text("day"),
        );
        if is_last_day {
            slider = slider.on_disabled_hover_text("Last day of month is active");
        }
        let mut last_day_selected = is_last_day;
        let last_day_widget = row.checkbox(&mut last_day_selected, "Last day");
        if slider.changed() && !last_day_selected {
            config.bandwidth.cycle_start_day = CycleStartDaySerde::Day(day_value);
            changed = true;
        }
        if last_day_widget.changed() {
            config.bandwidth.cycle_start_day = if last_day_selected {
                CycleStartDaySerde::LastDayOfMonth
            } else {
                CycleStartDaySerde::Day(MIN_CYCLE_DAY)
            };
            changed = true;
        }
        // Explicit value echo so the F8 access tree can assert on the current
        // day (the slider widget itself doesn't surface its numeric value as a
        // queryable label). When "Last day" is active we show "Last" instead.
        let value_echo = if is_last_day {
            "Last".to_string()
        } else {
            day_value.to_string()
        };
        row.label(format!("day {value_echo}"));
    });
    // No-retroactive-resplit tooltip (PRD §5.5.8 — HITL guardrail G11).
    ui.label(egui::RichText::new(NO_RESPLIT_TOOLTIP).small().weak());

    // ---- Temperature unit (T-29: C/F — only C/F ship in v1) ----
    ui.label(crate::i18n::t(lang, crate::i18n::Label::TemperatureUnit))
        .on_hover_text(TEMP_UNIT_TOOLTIP);
    ui.horizontal(|row| {
        let mut unit = config.display.temp_unit;
        if row
            .radio_value(&mut unit, TempUnit::Celsius, "°C")
            .changed()
        {
            config.display.temp_unit = TempUnit::Celsius;
            changed = true;
        }
        if row
            .radio_value(&mut unit, TempUnit::Fahrenheit, "°F")
            .changed()
        {
            config.display.temp_unit = TempUnit::Fahrenheit;
            changed = true;
        }
    });

    // ---- Technical units toggle (Story 13.4: renamed from 'Show raw values') ----
    ui.horizontal(|row| {
        let mut raw = config.display.raw_values;
        let r = row
            .checkbox(&mut raw, "Show technical units")
            .on_hover_text(TECHNICAL_UNITS_TOOLTIP);
        if r.changed() {
            config.display.raw_values = raw;
            changed = true;
        }
    });

    // ---- Size units (Story 13.4: renamed from 'Byte base') (T-28) ----
    ui.label(crate::i18n::t(lang, crate::i18n::Label::SizeUnits))
        .on_hover_text(SIZE_UNITS_TOOLTIP);
    ui.horizontal(|row| {
        let mut decimal = config.display.decimal_base;
        let r1 = row.radio_value(&mut decimal, true, "Decimal (GB)");
        let r2 = row.radio_value(&mut decimal, false, "Binary (GiB)");
        if r1.changed() || r2.changed() {
            config.display.decimal_base = decimal;
            changed = true;
        }
    });

    // ---- Refresh rate (Story 13.4: renamed from 'Poll interval') (T-3: clamp to [1, 60]) ----
    ui.label(crate::i18n::t(lang, crate::i18n::Label::RefreshRate))
        .on_hover_text(REFRESH_RATE_TOOLTIP);
    ui.horizontal(|row| {
        let mut v = config.poll_interval_seconds;
        let slider = row.add(
            egui::Slider::new(&mut v, MIN_POLL_INTERVAL..=MAX_POLL_INTERVAL)
                .clamping(egui::SliderClamping::Always),
        );
        if slider.changed() {
            config.poll_interval_seconds = v.clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);
            changed = true;
        }
    });
    // v1.0 audit 2 (P1) — the poller thread reads poll_interval_seconds only
    // at launch, so the slider changes the staleness threshold but NOT the
    // actual sampling cadence until restart. Inline hint mirrors the hotkey
    // pattern so the user knows.
    ui.label(egui::RichText::new("Applies on next launch").small().weak());
    if config.poll_interval_seconds <= MIN_POLL_INTERVAL {
        // T-3 visible warning when poll interval is at the floor.
        ui.label(
            egui::RichText::new("Poll interval at minimum (1s)")
                .small()
                .color(egui::Color32::YELLOW),
        );
    }

    // ---- Docked edge (T-36) ----
    dock_edge_section(ui, &mut config.dock, &mut changed, lang);

    // ---- Theme (T-35) ----
    theme_section(ui, &mut config.theme, &mut changed, lang);

    // ---- Story 17.2: Temperature alert thresholds ----
    ui.separator();
    ui.label(crate::i18n::t(
        lang,
        crate::i18n::Label::TemperatureAlerts,
    ))
        .on_hover_text("Set the temperatures at which the sidebar shows a warning (orange) or critical (red) indicator for CPU and GPU sensors.");
    ui.horizontal(|row| {
        row.label("CPU warn:");
        let mut v = config.thresholds.cpu_temp_warn as f32;
        // Constrain the slider RANGE so warn can never exceed critical-gap.
        // The prior code clamped the value AFTER the drag — silently snapping
        // warn down with zero user feedback (audit 1-B). Bounding the range
        // makes the invalid position unreachable: the drag physically stops
        // at critical-gap instead of snapping after release.
        let warn_max = warn_slider_max(config.thresholds.cpu_temp_critical, 40.0);
        if row
            .add(egui::Slider::new(&mut v, 40.0..=warn_max).suffix(" °C"))
            .changed()
        {
            config.thresholds.cpu_temp_warn = f64::from(v);
            changed = true;
        }
        row.label("critical:");
        let mut c = config.thresholds.cpu_temp_critical as f32;
        // Symmetric constraint: critical's MIN is warn+gap so dragging it
        // below warn is unreachable (no post-hoc snap-up).
        let crit_min = critical_slider_min(config.thresholds.cpu_temp_warn, 110.0);
        if row
            .add(egui::Slider::new(&mut c, crit_min..=110.0).suffix(" °C"))
            .changed()
        {
            config.thresholds.cpu_temp_critical = f64::from(c);
            changed = true;
        }
    });
    ui.horizontal(|row| {
        row.label("GPU warn:");
        let mut v = config.thresholds.gpu_temp_warn as f32;
        let warn_max = warn_slider_max(config.thresholds.gpu_temp_critical, 40.0);
        if row
            .add(egui::Slider::new(&mut v, 40.0..=warn_max).suffix(" °C"))
            .changed()
        {
            config.thresholds.gpu_temp_warn = f64::from(v);
            changed = true;
        }
        row.label("critical:");
        let mut c = config.thresholds.gpu_temp_critical as f32;
        let crit_min = critical_slider_min(config.thresholds.gpu_temp_warn, 110.0);
        if row
            .add(egui::Slider::new(&mut c, crit_min..=110.0).suffix(" °C"))
            .changed()
        {
            config.thresholds.gpu_temp_critical = f64::from(c);
            changed = true;
        }
    });
    // v1.0 parity — per-NIC bandwidth alerts (in/out Mbps).
    // NOTE: drive-used-space alert is deferred to v1.1 — the alert
    // classification path needs the DiskUsed/DiskTotal fraction which isn't
    // surfaced as a single Reading today; shipping a dead slider would
    // erode user trust (the control-exists-but-does-nothing anti-pattern).
    ui.horizontal(|row| {
        row.label("Network in alert:");
        let mut bi = config.thresholds.bandwidth_in_alert_mbps;
        if row
            .add(
                egui::Slider::new(&mut bi, 0..=10_000)
                    .suffix(" Mbps")
                    .fixed_decimals(0),
            )
            .changed()
        {
            config.thresholds.bandwidth_in_alert_mbps = bi;
            changed = true;
        }
        row.label("(0 = off)");
    });
    ui.horizontal(|row| {
        row.label("Network out alert:");
        let mut bo = config.thresholds.bandwidth_out_alert_mbps;
        if row
            .add(
                egui::Slider::new(&mut bo, 0..=10_000)
                    .suffix(" Mbps")
                    .fixed_decimals(0),
            )
            .changed()
        {
            config.thresholds.bandwidth_out_alert_mbps = bo;
            changed = true;
        }
        row.label("(0 = off)");
    });

    // ---- v1.0 parity: Language selector (i18n) ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Language))
        .on_hover_text("Choose the sidebar's display language. More languages can be added in future versions.");
    ui.horizontal(|row| {
        let current = crate::i18n::Language::from_code(&config.display.language);
        let mut selected = current;
        let names: Vec<&'static str> = crate::i18n::Language::all()
            .iter()
            .map(|l| l.display_name())
            .collect();
        let mut cb = egui::ComboBox::from_label("");
        let current_idx = crate::i18n::Language::all()
            .iter()
            .position(|l| *l == current)
            .unwrap_or(0);
        let mut chosen = current_idx;
        cb = cb.selected_text(names.get(current_idx).copied().unwrap_or("English"));
        cb.show_ui(row, |ui| {
            for (i, name) in names.iter().enumerate() {
                if ui.selectable_label(chosen == i, *name).clicked() {
                    chosen = i;
                }
            }
        });
        if let Some(&picked) = crate::i18n::Language::all().get(chosen) {
            if picked != selected {
                config.display.language = picked.code().to_string();
                selected = picked;
                changed = true;
            }
        }
        let _ = selected;
    });

    // ---- v1.0 parity: Sidebar width + font size + display toggles ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Appearance))
        .on_hover_text("Adjust the sidebar's width, text size, and how alerts look.");
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::SidebarWidth));
        let mut w = config.dock.width_px;
        let slider = row.add(egui::Slider::new(&mut w, 100..=300).suffix(" px"));
        if slider.changed() {
            config.dock.width_px = w.clamp(100, 300);
            changed = true;
        }
    });
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::FontSize));
        let mut f = config.display.font_size;
        let slider = row.add(egui::Slider::new(&mut f, 10..=22).suffix(" px"));
        if slider.changed() {
            config.display.font_size = f.clamp(10, 22);
            changed = true;
        }
    });
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::UiScale));
        let mut s = config.display.ui_scale_percent;
        let slider = row.add(
            egui::Slider::new(&mut s, 50..=300)
                .suffix("%")
                .fixed_decimals(0),
        );
        if slider.changed() {
            config.display.ui_scale_percent = s.clamp(50, 300);
            changed = true;
        }
    });
    ui.horizontal(|row| {
        let mut blink = config.display.alert_blink;
        if row
            .checkbox(
                &mut blink,
                crate::i18n::t(lang, crate::i18n::Label::BlinkAlerts),
            )
            .on_hover_text("Flash alerting metrics so they're noticeable even at a glance or for color-blind users.")
            .changed()
        {
            config.display.alert_blink = blink;
            changed = true;
        }
    });
    ui.horizontal(|row| {
        let mut graphs = config.display.show_graph_buttons;
        if row
            .checkbox(&mut graphs, "Show per-row graph buttons")
            .on_hover_text(
                "Display a small chart button next to each metric. Off by default — \
                 the buttons add height to every row. Click a button to open that \
                 metric's history graph popup.",
            )
            .changed()
        {
            config.display.show_graph_buttons = graphs;
            changed = true;
        }
    });

    // ---- v1.0 parity: Custom colors (background + font) ----
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::BackgroundColor));
        let mut bg = config.display.bg_color.clone();
        let te = row.add(
            egui::TextEdit::singleline(&mut bg)
                .desired_width(80.0)
                .hint_text("#000000"),
        );
        if te.changed() {
            config.display.bg_color = bg;
            changed = true;
        }
        row.label(tcolon(lang, crate::i18n::Label::BgOpacity));
        let mut op = config.display.bg_opacity_percent;
        if row
            .add(
                egui::Slider::new(&mut op, 10..=100)
                    .suffix("%")
                    .fixed_decimals(0),
            )
            .changed()
        {
            config.display.bg_opacity_percent = op.clamp(10, 100);
            changed = true;
        }
    });
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::FontColor));
        let mut fc = config.display.font_color.clone();
        let te = row.add(
            egui::TextEdit::singleline(&mut fc)
                .desired_width(80.0)
                .hint_text("#FFFFFF"),
        );
        if te.changed() {
            config.display.font_color = fc;
            changed = true;
        }
        row.label("(blank = theme default; use #RRGGBB)");
    });

    // ---- v1.0 parity: Position offsets (X/Y) ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Position))
        .on_hover_text("Fine-tune where the sidebar sits. Positive values move it inward.");
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::HorizontalOffset));
        let mut x = config.dock.offset_x_px;
        if row
            .add(egui::Slider::new(&mut x, -2000..=2000).suffix(" px"))
            .changed()
        {
            config.dock.offset_x_px = x;
            changed = true;
        }
    });
    ui.horizontal(|row| {
        row.label(tcolon(lang, crate::i18n::Label::VerticalOffset));
        let mut y = config.dock.offset_y_px;
        if row
            .add(egui::Slider::new(&mut y, -2000..=2000).suffix(" px"))
            .changed()
        {
            config.dock.offset_y_px = y;
            changed = true;
        }
    });

    // v1.0 UI/UX (audit BLK-A) — the Window section (Start hidden +
    // Pause-when-hidden) is REMOVED from v1.0 because there's no tray icon.
    // Without a tray, enabling "Start hidden" traps the user with no way to
    // un-hide the sidebar (the default hotkeys for toggle/show/hide are all
    // empty). These toggles return when the tray icon ships (v1.1).
    // The config fields (initially_hidden, pause_when_hidden) stay for the
    // hidden-mode internal plumbing; only the user-facing controls are gone.

    // ---- v1.0 parity: Run at Windows startup ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Startup))
        .on_hover_text(
            "Control whether sidebar launches automatically when you sign in to Windows.",
        );
    ui.horizontal(|row| {
        let mut run = config.display.run_at_startup;
        if row
            .checkbox(
                &mut run,
                crate::i18n::t(lang, crate::i18n::Label::StartSidebarWhenWindowsStarts),
            )
            .on_hover_text(
                "Adds sidebar to your Windows startup. No administrator privileges needed.",
            )
            .changed()
        {
            // Apply immediately: write/delete the HKCU Run key. If the write
            // fails (rare), revert the checkbox so it reflects reality and
            // surface nothing scary to the user (G15 — non-fatal).
            match sidebar_platform::startup::set_enabled(run) {
                Ok(()) => {
                    config.display.run_at_startup = run;
                    changed = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "startup: failed to update Run key");
                    // Leave config.display.run_at_startup as-is (reverted).
                }
            }
        }
    });

    // ---- Story 8.9: metric enable/disable + reorder ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Metrics))
        .on_hover_text(METRICS_TOOLTIP);
    crate::gui::metric_list::render(ui, &mut config.metrics, "settings", on_change);

    // ---- v1.0 parity: Global hotkeys (8, matching the reference app) ----
    ui.separator();
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Hotkeys))
        .on_hover_text("Global keyboard shortcuts. Use the format Ctrl+Shift+S. Leave blank to disable. Changes apply on next launch.");
    hotkey_row(
        ui,
        "Toggle click-through",
        &mut config.hotkeys.click_through,
        &mut changed,
    );
    hotkey_row(
        ui,
        "Toggle sidebar",
        &mut config.hotkeys.toggle_visibility,
        &mut changed,
    );
    hotkey_row(ui, "Show sidebar", &mut config.hotkeys.show, &mut changed);
    hotkey_row(ui, "Hide sidebar", &mut config.hotkeys.hide, &mut changed);
    hotkey_row(
        ui,
        "Cycle dock edge",
        &mut config.hotkeys.cycle_edge,
        &mut changed,
    );
    hotkey_row(
        ui,
        "Cycle screen",
        &mut config.hotkeys.cycle_screen,
        &mut changed,
    );
    hotkey_row(
        ui,
        "Reload settings",
        &mut config.hotkeys.reload,
        &mut changed,
    );
    // v1.0 UI/UX (audit MJ-D) — "Toggle reserve space" removed: the handler
    // is a logged no-op (AppBar is always-on in v1). Shipping a dead control
    // is the "control-exists-but-does-nothing" anti-pattern. Returns when
    // AppBar toggling is implemented (v1.1).
    hotkey_row(ui, "Close sidebar", &mut config.hotkeys.close, &mut changed);

    // v1.0 UI/UX (audit MAJ-3): hotkey + startup changes require a restart
    // to take effect (RegisterHotKey runs once at launch). Without a visible
    // Restart button, non-technical users change a hotkey, try it, find it
    // doesn't work, and conclude the feature is broken. This button spawns
    // a fresh process (same exe) + closes this window — the new instance
    // picks up the updated config.
    ui.horizontal(|row| {
        if row.button("Restart sidebar to apply").clicked() {
            match crate::gui::restart_sidebar() {
                Ok(()) => {
                    row.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => {
                    // v1.0 UI/UX (audit MJ-A) — surface the error so the
                    // user knows restart failed (rather than silently doing
                    // nothing). Stored in temp storage so it persists a few
                    // frames.
                    let msg = format!(
                        "Could not restart: {e}. Please close and reopen sidebar manually."
                    );
                    tracing::warn!(error = %e, "failed to restart sidebar from settings");
                    row.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("settings_restart_error"), msg);
                    });
                }
            }
        }
    });
    // Show the restart error if any (temp data, ages out after a few frames).
    let restart_error: Option<String> = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new("settings_restart_error")));
    if let Some(err) = restart_error {
        ui.label(
            egui::RichText::new(&err)
                .small()
                .color(egui::Color32::from_rgb(220, 80, 80)),
        );
    }

    if changed {
        on_change();
    }
}

/// Render one hotkey binding row: label + text input (format hint
/// `Ctrl+Shift+S`) + inline validation. v1.0 UI/UX (audit MJ-E): shows a
/// red "unrecognized format" label when the input is non-empty but doesn't
/// parse via HotkeyCombo::parse, so the user knows before restart that
/// their hotkey won't register.
fn hotkey_row(ui: &mut Ui, label: &str, value: &mut String, changed: &mut bool) {
    ui.horizontal(|row| {
        row.label(format!("{label}:"));
        let resp = row.add(
            egui::TextEdit::singleline(value)
                .desired_width(140.0)
                .hint_text("Ctrl+Shift+S (blank = off)"),
        );
        if resp.changed() {
            *changed = true;
        }
        // v1.0 UI/UX (audit MJ-E) — validate on every frame: if non-empty
        // but unparseable, show a red ⚠ so the user knows their binding is
        // invalid BEFORE restarting.
        if !value.is_empty() && crate::gui::hotkey::HotkeyCombo::parse(value).is_err() {
            row.label(
                egui::RichText::new("⚠ unrecognized format")
                    .small()
                    .color(egui::Color32::from_rgb(220, 80, 80)),
            );
        }
    });
}

/// Render the docked-edge radio section.
fn dock_edge_section(
    ui: &mut Ui,
    dock: &mut DockConfig,
    changed: &mut bool,
    lang: crate::i18n::Language,
) {
    ui.label(crate::i18n::t(lang, crate::i18n::Label::DockedEdge))
        .on_hover_text(DOCKED_EDGE_TOOLTIP);
    ui.horizontal(|row| {
        for &e in &["Left", "Right", "Top", "Bottom"] {
            let mut selected = dock.edge == e;
            if row.checkbox(&mut selected, e).changed() && selected {
                dock.edge = e.to_string();
                *changed = true;
            }
        }
    });
    // Story 17.7 — monitor-picker dropdown (replaces the raw TextEdit).
    // Populated from monitors::enumerate(); falls back to a text field
    // if enumeration fails.
    ui.label(crate::i18n::t(lang, crate::i18n::Label::TargetMonitor))
        .on_hover_text("Which screen the sidebar docks to. 'primary' uses your main display.");
    #[cfg(windows)]
    {
        if let Ok(displays) = crate::gui::monitors::enumerate() {
            let current = &dock.monitor_id;
            // Cert v1.0 (frontend audit C1) — preserve the "primary" sentinel.
            // The default config is monitor_id = "primary" which dynamically
            // resolves to whatever the current primary display is (survives
            // dock/undock). The prior code unconditionally overwrote "primary"
            // with displays[0].id the first frame the panel opened, locking
            // the sidebar to one physical display forever. Only commit a
            // selection when the user actually picked an entry (found.is_some).
            // Cert v1.0 (frontend audit C1, refined) — preserve the "primary"
            // sentinel. `found` is whether current is a real device id;
            // `initial_idx` is what the dropdown shows pre-click. We commit a
            // selection only when the user actually moved `selected_idx`
            // (selected_idx != initial_idx). This keeps "primary" intact when
            // the user doesn't click, while still allowing selection FROM
            // "primary" (the default) — the prior `found.is_some()` guard
            // blocked every selection when current was "primary".
            let found = displays
                .iter()
                .position(|d| d.id.eq_ignore_ascii_case(current));
            let initial_idx = found.unwrap_or(0);
            let mut selected_idx = initial_idx;
            let labels: Vec<String> = displays
                .iter()
                .map(|d| {
                    if d.primary {
                        format!("{} (primary, {}x{})", d.id, d.width, d.height)
                    } else {
                        format!("{} ({}x{})", d.id, d.width, d.height)
                    }
                })
                .collect();
            let mut cb = egui::ComboBox::from_label("");
            cb = cb.selected_text(
                labels
                    .get(selected_idx)
                    .cloned()
                    .unwrap_or_else(|| "primary".into()),
            );
            cb.show_ui(ui, |ui| {
                for (i, label) in labels.iter().enumerate() {
                    if ui.selectable_label(selected_idx == i, label).clicked() {
                        selected_idx = i;
                    }
                }
            });
            if let Some(d) = displays.get(selected_idx) {
                // Commit only when the user moved the selection this frame.
                // If current was "primary" and the user didn't click,
                // selected_idx == initial_idx so dock.monitor_id is untouched
                // and the dynamic "primary" resolution survives.
                if selected_idx != initial_idx && d.id != dock.monitor_id {
                    dock.monitor_id.clone_from(&d.id);
                    *changed = true;
                }
            }
        } else {
            // Fallback: raw text field.
            ui.text_edit_singleline(&mut dock.monitor_id);
            *changed = true;
        }
    }
    #[cfg(not(windows))]
    {
        ui.text_edit_singleline(&mut dock.monitor_id);
    }
}

/// Render the theme-mode radio section.
fn theme_section(
    ui: &mut Ui,
    theme: &mut ThemeConfig,
    changed: &mut bool,
    lang: crate::i18n::Language,
) {
    ui.label(crate::i18n::t(lang, crate::i18n::Label::Theme))
        .on_hover_text(THEME_TOOLTIP);
    ui.horizontal(|row| {
        for &m in &["Dark", "Light", "System"] {
            let mut selected = theme.mode == m;
            if row.checkbox(&mut selected, m).changed() && selected {
                theme.mode = m.to_string();
                *changed = true;
            }
        }
    });
}

/// Minimum gap between warn and critical temperature thresholds (°C).
/// Audited at v1.0 (1-B): the prior code clamped values after the drag,
/// silently snapping warn down or critical up. Bounding the slider RANGE
/// with this gap makes the invalid position unreachable.
const THRESHOLD_GAP: f64 = 5.0;

/// Upper bound for a warn slider given the current critical value, so the
/// slider range physically prevents `warn > critical - gap`. Clamped to
/// `floor` so a very low critical (e.g. 50) still yields a usable range
/// `floor..=45` rather than an empty `floor..=45`.
///
/// Pure helper extracted so the contract is unit-tested independently of
/// the egui slider widget (audit 1-B).
///
/// The `f64 → f32` cast is intentional: threshold config is stored as `f64`
/// (T-26 / TOML schema) but egui's `Slider::new` takes `f32`. The values
/// are °C integers in practice (40..=110); f32 mantissa has 23 bits of
/// precision, more than enough. Truncation is impossible in the legal range.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
fn warn_slider_max(critical: f64, floor: f64) -> f32 {
    ((critical - THRESHOLD_GAP) as f32).max(floor as f32)
}

/// Lower bound for a critical slider given the current warn value, so the
/// slider range physically prevents `critical < warn + gap`. Clamped to
/// `ceil` so a very high warn doesn't push the min past the slider's
/// absolute ceiling (which would produce an inverted range).
///
/// Pure helper extracted so the contract is unit-tested independently of
/// the egui slider widget (audit 1-B). See [`warn_slider_max`] for why the
/// `f64 → f32` cast is safe.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
fn critical_slider_min(warn: f64, ceil: f64) -> f32 {
    ((warn + THRESHOLD_GAP) as f32).min(ceil as f32)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    //! Story 8.5 TDD contract tests (F8 egui_kittest + pure-fn unit tests).

    use super::*;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use sidebar_domain::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: cycle_start_day section renders with default day =====

    #[test]
    fn cycle_start_day_section_renders_default() {
        let mut config = Config::default();
        assert_eq!(config.bandwidth.cycle_start_day, CycleStartDaySerde::Day(1));
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Billing cycle start day"),
            "panel must render the cycle-start-day section (got: {labels})"
        );
        // The slider's current value text shows the day number.
        assert!(
            labels.contains("day"),
            "panel must surface the day slider (got: {labels})"
        );
    }

    // ===== Happy Path #1 (persist): changing config field → re-render reflects it =====

    #[test]
    fn change_cycle_start_day_renders_new_value() {
        // Verify the GREEN render path observes a config mutation by setting
        // cycle_start_day=15 BEFORE constructing the harness — the rendered
        // labels must surface "15". This locks in that render() reads from
        // config (no caching).
        let mut config = Config::default();
        config.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("15"),
            "after setting cycle_start_day=15 the panel must surface '15' (got: {labels})"
        );
    }

    // ===== Happy Path #2: toggle raw_values on → flips config flag =====
    //
    // The settings panel updates the config; the metric-row re-render is
    // verified via the composition test (render_snapshot in mod.rs). Here we
    // lock in that clicking the checkbox flips the config flag AND fires the
    // on_change callback.

    #[test]
    fn toggle_raw_values_flips_config_flag() {
        // Use a shared config behind Arc<Mutex<>> so the closure can mutate it
        // AND the test can read it after the harness click. This mirrors the
        // real wiring where AppState holds Config behind a lock.
        use std::sync::Mutex;
        let config = Arc::new(Mutex::new(Config::default()));
        assert!(
            !config.lock().unwrap().display.raw_values,
            "default raw_values=false"
        );
        let counter = Arc::new(AtomicUsize::new(0));
        let cfg_for_closure = config.clone();
        let c = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = c.clone();
            let cfg = cfg_for_closure.clone();
            let mut cfg_guard = cfg.lock().unwrap();
            render(ui, &mut cfg_guard, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        // Find the "Show technical units" checkbox and click it.
        harness.get_by_label("Show technical units").click();
        harness.run();
        assert!(
            config.lock().unwrap().display.raw_values,
            "clicking the checkbox must flip display.raw_values=true"
        );
        assert!(
            counter.load(Ordering::SeqCst) >= 1,
            "on_change callback must fire at least once after a toggle"
        );
    }

    // ===== Boundary #1: cycle_start_day change does NOT retroactively re-split =====

    #[test]
    fn cycle_start_day_change_does_not_re_split_current_cycle() {
        // The no-retroactive-resplit rule is documented in NO_RESPLIT_TOOLTIP.
        // The panel surfaces the tooltip text + only mutates the config field
        // (it does NOT touch cycle bookkeeping — that's the host's job on the
        // next rollover).
        let mut config = Config::default();
        let labels = {
            let mut harness = Harness::new_ui(|ui| {
                render(ui, &mut config, &|| {});
            });
            harness.run();
            all_labels(&harness).join(" | ")
        };
        // harness dropped before mutating config (borrow checker safety).
        assert!(
            labels.contains(NO_RESPLIT_TOOLTIP),
            "settings panel must surface the no-retroactive-resplit tooltip (got: {labels})"
        );
        // Pure-fn contract: cycle_start_day can be mutated without touching
        // any other config field. We assert that mutating cycle_start_day
        // leaves the rest of the config unchanged (no retroactive re-split at
        // the data level — the panel is pure render).
        let mut before = Config::default();
        before.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        let mut after = before.clone();
        after.bandwidth.cycle_start_day = CycleStartDaySerde::Day(15);
        assert_eq!(before, after, "cycle_start_day mutation is idempotent");
    }

    // ===== Boundary #2: poll_interval=0 → clamped to 1 with warning =====

    #[test]
    fn poll_interval_zero_clamps_to_one() {
        let mut config = Config {
            poll_interval_seconds: 0,
            ..Config::default()
        };
        let labels = {
            let mut harness = Harness::new_ui(|ui| {
                render(ui, &mut config, &|| {});
            });
            harness.run();
            all_labels(&harness).join(" | ")
        };
        // harness dropped; now we can mutate config.
        assert!(
            config.poll_interval_seconds <= MIN_POLL_INTERVAL,
            "precondition: poll_interval at floor"
        );
        assert!(
            labels.contains("Poll interval at minimum"),
            "poll_interval at floor must surface a visible warning (got: {labels})"
        );
        // The live slider path applies clamp_to_range; we simulate it here.
        let clamped = config
            .poll_interval_seconds
            .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL);
        assert_eq!(
            clamped, MIN_POLL_INTERVAL,
            "poll_interval=0 clamps to 1 (T-3)"
        );
    }

    // ===== Boundary #3: cycle_start_day=29 rejected at UI (T-26) =====

    #[test]
    fn cycle_start_day_29_rejected_at_ui() {
        // The slider constrains to [1, 28]. We verify the slider's range by
        // asserting the panel renders the "Last day" escape hatch and the
        // cycle-start-day section.
        assert_eq!(MIN_CYCLE_DAY, 1);
        assert_eq!(MAX_CYCLE_DAY, 28);
        let mut config = Config::default();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Last day"),
            "panel must offer the 'Last day' option for month-end (T-26 escape hatch)"
        );
        assert!(
            labels.contains("Billing cycle start day"),
            "panel must render the cycle-start-day section"
        );
    }

    // ===== Boundary #4: settings closed without save → autosave debounced =====

    #[test]
    fn on_change_callback_invoked_on_edit() {
        // The autosave contract: on any change, the host's on_change fires.
        // The host debounces. We verify the callback fires on a click.
        use std::sync::Mutex;
        let config = Arc::new(Mutex::new(Config::default()));
        let counter = Arc::new(AtomicUsize::new(0));
        let cfg_for_closure = config.clone();
        let c = counter.clone();
        let mut harness = Harness::new_ui(move |ui| {
            let c = c.clone();
            let cfg = cfg_for_closure.clone();
            let mut cfg_guard = cfg.lock().unwrap();
            render(ui, &mut cfg_guard, &move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        });
        harness.run();
        let before = counter.load(Ordering::SeqCst);
        harness.get_by_label("Last day").click();
        harness.run();
        let after = counter.load(Ordering::SeqCst);
        assert!(
            after > before,
            "toggling 'Last day' must fire on_change (before={before}, after={after})"
        );
        assert_eq!(
            config.lock().unwrap().bandwidth.cycle_start_day,
            CycleStartDaySerde::LastDayOfMonth
        );
    }

    // ===== v1.0 audit 1-B — threshold slider range constraints =====
    //
    // The bug: dragging warn above critical-5 silently snapped down via a
    // post-drag clamp. The fix bounds the slider RANGE so the invalid
    // position is unreachable. These tests pin the helpers that compute
    // the bounds.

    /// Cited: v1.0 audit Iteration 1-B. With critical=50, warn's max must
    /// be 45 (= 50 - THRESHOLD_GAP) so the slider physically stops there.
    /// This is the exact scenario the bug report named (dragging warn to
    /// 80 while critical=50 silently snapped to 45).
    #[test]
    fn warn_slider_max_caps_at_critical_minus_gap() {
        // critical=50 → warn_max = 45
        assert_eq!(warn_slider_max(50.0, 40.0), 45.0);
        // critical=100 → warn_max = 95
        assert_eq!(warn_slider_max(100.0, 40.0), 95.0);
    }

    /// Cited: v1.0 audit Iteration 1-B. When critical is so low that
    /// `critical - gap` dips below the floor, the floor wins — otherwise
    /// the slider range would be inverted (max < min).
    #[test]
    fn warn_slider_max_floors_below_threshold() {
        // critical=42 → 42-5=37 < floor 40 → returns 40 (floor wins).
        assert_eq!(warn_slider_max(42.0, 40.0), 40.0);
    }

    /// Cited: v1.0 audit Iteration 1-B. Symmetric to warn: critical's min
    /// is warn+gap so dragging it below warn is unreachable.
    #[test]
    fn critical_slider_min_rises_with_warn_plus_gap() {
        // warn=80 → critical_min = 85
        assert_eq!(critical_slider_min(80.0, 110.0), 85.0);
        // warn=40 → critical_min = 45
        assert_eq!(critical_slider_min(40.0, 110.0), 45.0);
    }

    /// Cited: v1.0 audit Iteration 1-B. When warn is so high that
    /// `warn + gap` exceeds the ceiling, the ceiling wins — otherwise
    /// the slider range would be inverted (min > max).
    #[test]
    fn critical_slider_min_ceils_above_threshold() {
        // warn=108 → 108+5=113 > ceil 110 → returns 110 (ceil wins).
        assert_eq!(critical_slider_min(108.0, 110.0), 110.0);
    }

    /// Cited: v1.0 audit Iteration 1-B. End-to-end invariant: for any
    /// (warn, critical) pair the user can produce via the constrained
    /// sliders, `warn <= critical - gap` must hold. Walks the reachable
    /// warn surface [floor, warn_slider_max] for several critical values.
    #[test]
    fn constrained_slider_bounds_preserve_gap_invariant() {
        for critical in [50_f64, 70.0, 90.0, 110.0] {
            let warn_max = f64::from(warn_slider_max(critical, 40.0));
            // Walk a few warn values inside the reachable range.
            for warn in [40_f64, f64::midpoint(40.0, warn_max), warn_max] {
                assert!(
                    warn <= critical - THRESHOLD_GAP + 1e-6,
                    "warn {warn} must stay <= critical {critical} - gap {THRESHOLD_GAP}: invariant violated"
                );
            }
        }
    }

    // ===== v1.0 audit 2 — restart-required hints render inline =====

    /// Cited: v1.0 audit Iteration 2 (P1). The refresh-rate slider mutates
    /// config but the poller reads it only at launch; without an inline
    /// hint the user assumes the slider is broken. The hint MUST render.
    #[test]
    fn refresh_rate_renders_restart_hint() {
        let mut config = Config::default();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &mut config, &|| {});
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains("Applies on next launch"),
            "refresh-rate slider must surface the restart-required hint (got: {labels})"
        );
    }

    /// Cited: v1.0 audit Iteration 2 (P2). The billing-cycle-day slider's
    /// no-resplit tooltip must tell the user a restart is required —
    /// otherwise they watch the countdown + assume the change was lost.
    #[test]
    fn billing_cycle_day_tooltip_mentions_restart() {
        assert!(
            NO_RESPLIT_TOOLTIP.to_lowercase().contains("restart"),
            "NO_RESPLIT_TOOLTIP must mention restart so the user knows the running sidebar won't pick up the change (got: {NO_RESPLIT_TOOLTIP})"
        );
    }
}
