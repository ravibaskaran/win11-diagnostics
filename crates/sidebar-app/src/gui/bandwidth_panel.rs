//! Story 8.4 — Bandwidth Panel (v2 MARQUEE, PRD §3 Tier 4 + §5.5.8).
//!
//! The bandwidth panel renders the monthly-bandwidth-tracking summary:
//! per-NIC rows (friendly name + RX/TX/total bytes formatted human-readably)
//! and a countdown to the next billing-cycle reset. A history strip below
//! shows the prior cycle's totals.
//!
//! This module is pure-render: it reads a `&BandwidthView` (Story 5.3 DTO)
//! and writes into an `egui::Ui`. No IO, no mutation.
//!
//! ## Empty / Boundary States (PRD §5.5.8)
//!
//! - **Empty** (no tracked NICs): renders the [`EMPTY_TEXT`] placeholder —
//!   "No network adapters tracked".
//! - **`days_until_reset == 0`**: renders [`RESETS_TODAY`] — "Resets today"
//!   — instead of "0 days until reset" (avoids the misleading "0 days").
//! - **History NIC absent from current**: annotated `(disconnected)` so the
//!   user understands the prior-cycle row has no live counterpart.
//!
//! ## Cited
//!
//! - Story 8.4 TDD contract (Happy Path #1-#2, Boundary #1-#3)
//! - PRD §3 Tier 4 + §5.5.8 (MARQUEE feature — HITL G11 visual review)
//! - architecture.md §6 (GUI crate layering)
//! - sidebar-bandwidth::view (Story 5.3 DTO)
//! - sidebar-domain::format (Story 1.3 `format_bytes`)

use eframe::egui::Ui;
use sidebar_bandwidth::view::{BandwidthView, NICtotals};
use sidebar_domain::config::DisplayConfig;
use sidebar_domain::format;

/// Empty-state placeholder (PRD §5.5.8 — exact string).
pub const EMPTY_TEXT: &str = "No network adapters tracked";

/// String shown when `days_until_reset == 0` (PRD §5.5.8 — exact string).
pub const RESETS_TODAY: &str = "Resets today";

/// Annotation appended to a history NIC's name when the same LUID is absent
/// from the current cycle (Boundary #3 — disconnected).
pub const DISCONNECTED_TAG: &str = "(disconnected)";

/// Banner shown when `BandwidthView::degraded` is set (v1.0 audit 1-A).
/// The accountant hit a persistent `archive_cycle` failure streak — the
/// cycle total is stuck and won't auto-advance. A restart usually clears
/// it (frees the SQLite lock / picks up a schema fix).
pub const DEGRADED_BANNER: &str =
    "Bandwidth cycle reset failed — totals may be stale. Restart Sidebar.";

/// Render the bandwidth panel: per-NIC rows + history strip below.
///
/// - `view` — the Story 5.3 DTO from `Arc<RwLock<BandwidthView>>`.
/// - `display` — `[display]` config controlling decimal/binary base + raw
///   byte rendering (mirrors the metric-row display config).
///
/// Layout:
/// 1. Per-NIC rows (one row per entry in `view.current`): friendly name,
///    RX, TX, total bytes — each formatted via `format_bytes`.
/// 2. The reset-countdown string (either "N days until reset (YYYY-MM-DD)"
///    or [`RESETS_TODAY`] when N == 0).
/// 3. A separator, then the history strip at a smaller font (one row per
///    `view.history` entry, annotated `(disconnected)` when its LUID is
///    absent from `current`).
///
/// Empty `view.current` (no tracked NICs) renders [`EMPTY_TEXT`] and returns.
pub fn render(ui: &mut Ui, view: &BandwidthView, display: &DisplayConfig) {
    if view.degraded {
        // v1.0 audit 1-A — surface a persistent archive-cycle failure even
        // when no NICs are tracked (the cycle is still stuck). Renders above
        // the empty/normal body so the user sees it first.
        ui.label(egui::RichText::new(DEGRADED_BANNER).color(ui.visuals().warn_fg_color));
    }
    if view.current.is_empty() {
        ui.label(EMPTY_TEXT);
        return;
    }
    render_current(ui, view, display);
    render_reset(ui, view);
    if !view.history.is_empty() {
        ui.separator();
        render_history(ui, view, display);
    }
}

/// Render the per-NIC current-cycle rows.
fn render_current(ui: &mut Ui, view: &BandwidthView, display: &DisplayConfig) {
    for nic in &view.current {
        let row = nic_row(nic, display);
        ui.label(row);
    }
}

/// Render the reset countdown ("12 days until reset (2026-07-31)" or
/// [`RESETS_TODAY`]).
fn render_reset(ui: &mut Ui, view: &BandwidthView) {
    ui.horizontal(|row| {
        row.label(reset_countdown_label(view));
        // Story 17.3 — CSV export button. Writes to %TEMP% and shows
        // the path or error to the user (not just tracing logs).
        if row
            .small_button(crate::i18n::t(
                crate::i18n::Language::English,
                crate::i18n::Label::ExportCsv,
            ))
            .clicked()
        {
            let csv = export_bandwidth_csv(view);
            // v1.0 UI/UX (audit MJ-Z5) — write to the user's Documents folder
            // so a non-technical user can actually find the file. %TEMP% is
            // invisible (hidden AppData\Local\Temp). Fall back to temp if
            // Documents is unavailable (G15 — non-fatal).
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let filename = format!("sidebar-bandwidth-{stamp}.csv");
            // Resolve Documents folder: USERPROFILE\Documents on Windows.
            let docs = std::env::var_os("USERPROFILE")
                .map(|h| std::path::Path::new(&h).join("Documents"))
                .filter(|p| p.exists());
            let path = match docs {
                Some(d) => d.join(&filename),
                None => std::env::temp_dir().join(&filename),
            };
            let message = match std::fs::write(&path, csv) {
                Ok(()) => {
                    tracing::info!(path = %path.display(), "bandwidth CSV exported");
                    format!("Exported CSV to {}", path.display())
                }
                Err(e) => {
                    tracing::warn!(error = %e, "CSV export failed");
                    format!("CSV export failed: {e}")
                }
            };
            // Cert v1.0 — use temp (frame-scoped) storage, NOT persisted.
            // insert_persisted serializes to eframe Storage and reloads the
            // status on every future launch, leaving a stale "Exported CSV
            // to ..." or error banner visible forever. Temp data ages out
            // after a few frames, matching the intended transient feedback.
            row.ctx()
                .data_mut(|data| data.insert_temp(export_status_id(), message));
        }
    });
    let status: Option<String> = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(export_status_id()));
    if let Some(message) = status {
        ui.label(message);
    }
}

/// Story 17.3 — generate a CSV string from the BandwidthView. RFC 4180
/// compliant: CRLF line terminators, quoted fields with doubled quotes,
/// embedded CR/LF replaced with space to avoid breaking row boundaries.
#[allow(clippy::format_push_string)]
fn export_bandwidth_csv(view: &BandwidthView) -> String {
    let mut out = String::from("luid,adapter_name,rx_bytes,tx_bytes\r\n");
    for nic in &view.current {
        let name = nic.friendly_name.as_deref().unwrap_or("unknown");
        out.push_str(&format!(
            "{},{},{},{}\r\n",
            nic.luid,
            csv_field(name),
            nic.rx_bytes,
            nic.tx_bytes
        ));
    }
    for nic in &view.history {
        let name = nic.friendly_name.as_deref().unwrap_or("unknown");
        out.push_str(&format!(
            "{},{},{},{}\r\n",
            nic.luid,
            csv_field(name),
            nic.rx_bytes,
            nic.tx_bytes
        ));
    }
    out
}

fn csv_field(value: &str) -> String {
    let cleaned = value.replace(['\r', '\n'], " ");
    format!("\"{}\"", cleaned.replace('"', "\"\""))
}

fn export_status_id() -> egui::Id {
    egui::Id::new("bandwidth_export_status")
}

/// Render the prior-cycle history strip at a smaller font. Each NIC's row
/// annotates `(disconnected)` when its LUID is absent from `current`.
fn render_history(ui: &mut Ui, view: &BandwidthView, display: &DisplayConfig) {
    if view.history.is_empty() {
        return;
    }
    for nic in &view.history {
        let disconnected = !view.current.iter().any(|c| c.luid == nic.luid);
        let label = history_row(nic, display, disconnected);
        // Smaller-font: RichText::small() drops one text-style tier; the F8
        // access tree still surfaces the text as a queryable node.
        ui.label(egui::RichText::new(label).small());
    }
}

/// Build the human-readable label for a current-cycle NIC row:
/// "Wi-Fi (192.168.1.5)  RX 50 GB  TX 20 GB  Total 70 GB".
///
/// The IPv4 (when present + on Windows) is appended to the adapter name so
/// the user can see which IP each tracked NIC has — matching the reference
/// SidebarDiagnostics app's NetworkIP metric (v1.0 parity).
#[must_use]
pub(crate) fn nic_row(nic: &NICtotals, display: &DisplayConfig) -> String {
    let name = nic
        .friendly_name
        .clone()
        .unwrap_or_else(|| format!("NIC 0x{:x}", nic.luid));
    // v1.0 parity — append the NIC's IPv4 address (best-effort: None on
    // non-Windows, disconnected adapters, or IPv6-only).
    let name_with_ip = match sidebar_platform::net_info::ipv4_for_luid(nic.luid) {
        Some(ip) => format!("{name} ({ip})"),
        None => name,
    };
    let rx = format_bytes_with_config(nic.rx_bytes, display);
    let tx = format_bytes_with_config(nic.tx_bytes, display);
    let total = format_bytes_with_config(nic.rx_bytes + nic.tx_bytes, display);
    format!("{name_with_ip}  RX {rx}  TX {tx}  Total {total}")
}

/// Build the reset-countdown label: "12 days until reset (2026-07-31)" or
/// [`RESETS_TODAY`] when `days_until_reset == 0`.
#[must_use]
pub(crate) fn reset_countdown_label(view: &BandwidthView) -> String {
    if view.days_until_reset == 0 {
        RESETS_TODAY.to_string()
    } else {
        format!(
            "{} days until reset ({})",
            view.days_until_reset, view.next_reset_date
        )
    }
}

/// Build the history-strip label for one archived-cycle NIC, with optional
/// `(disconnected)` annotation.
#[must_use]
pub(crate) fn history_row(nic: &NICtotals, display: &DisplayConfig, disconnected: bool) -> String {
    let name = nic
        .friendly_name
        .clone()
        .unwrap_or_else(|| format!("NIC 0x{:x}", nic.luid));
    let rx = format_bytes_with_config(nic.rx_bytes, display);
    let tx = format_bytes_with_config(nic.tx_bytes, display);
    let total = format_bytes_with_config(nic.rx_bytes + nic.tx_bytes, display);
    let tag = if disconnected {
        format!(" {DISCONNECTED_TAG}")
    } else {
        String::new()
    };
    format!("{name}{tag}  RX {rx}  TX {tx}  Total {total}")
}

/// Format bytes per the DisplayConfig (decimal vs binary base, raw toggle).
/// Mirrors the metric-row dispatch (Story 8.3).
#[must_use]
pub(crate) fn format_bytes_with_config(bytes: u64, display: &DisplayConfig) -> String {
    if display.raw_values {
        format!("{bytes} B")
    } else {
        format::format_bytes(
            bytes,
            crate::gui::metric_row::base_from_config(display.decimal_base),
        )
    }
}

#[cfg(test)]
mod tests {
    //! Story 8.4 TDD contract tests (F8 egui_kittest).
    //!
    //! RED phase: every assertion is expected to FAIL — `render` is a no-op
    //! stub, so the kittest access tree contains nothing bandwidth-related.

    use super::*;
    use chrono::NaiveDate;
    use egui_kittest::kittest::NodeT;
    use egui_kittest::Harness;
    use sidebar_domain::config::DisplayConfig;

    const GB: u64 = 1_000_000_000;

    fn default_display() -> DisplayConfig {
        DisplayConfig {
            temp_unit: sidebar_domain::format::TempUnit::Celsius,
            raw_values: false,
            decimal_base: true,
            ..Default::default()
        }
    }

    fn nic(luid: u64, name: &str, rx_gb: u64, tx_gb: u64) -> NICtotals {
        NICtotals {
            luid,
            friendly_name: Some(name.to_string()),
            rx_bytes: rx_gb * GB,
            tx_bytes: tx_gb * GB,
        }
    }

    fn all_labels(harness: &Harness<'_>) -> Vec<String> {
        let root = harness.root();
        root.children_recursive()
            .filter_map(|n| {
                let node = n.accesskit_node();
                node.label().or_else(|| node.value())
            })
            .collect()
    }

    // ===== Happy Path #1: one NIC (rx=50GB, tx=20GB, total=70GB, days=12) =====

    #[test]
    fn one_nic_renders_rx_tx_total_and_days() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        // format_bytes uses 3 sig figs (T-30): 50GB → "50.0 GB".
        assert!(
            labels.contains("50.0 GB"),
            "must render RX=50.0 GB (got: {labels})"
        );
        assert!(
            labels.contains("20.0 GB"),
            "must render TX=20.0 GB (got: {labels})"
        );
        assert!(
            labels.contains("70.0 GB"),
            "must render total=70.0 GB (got: {labels})"
        );
        assert!(
            labels.contains("12 days until reset"),
            "must render '12 days until reset' (got: {labels})"
        );
    }

    // ===== Happy Path #2: history 1 prior cycle → renders below =====

    #[test]
    fn one_history_cycle_renders_smaller() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![nic(1, "Wi-Fi", 40, 10)],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        // History renders RX=40GB and TX=10GB. We assert on those values; the
        // "smaller font" is a visual property the F8 access tree doesn't
        // carry — the GREEN-phase implementation uses RichText::small() so
        // manual smoke (G11) confirms the visual contract.
        assert!(
            labels.contains("40.0 GB"),
            "history row must render RX=40.0 GB (got: {labels})"
        );
        assert!(
            labels.contains("10.0 GB"),
            "history row must render TX=10.0 GB (got: {labels})"
        );
    }

    // ===== Boundary #1: empty BandwidthView → "No network adapters tracked" =====

    #[test]
    fn empty_view_renders_placeholder() {
        let view = BandwidthView {
            current: vec![],
            history: vec![],
            days_until_reset: 30,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(EMPTY_TEXT),
            "empty view must render '{EMPTY_TEXT}' (got: {labels})"
        );
    }

    // ===== Boundary #2: days_until_reset=0 → "Resets today" =====

    #[test]
    fn days_zero_renders_resets_today() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 0,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(RESETS_TODAY),
            "days_until_reset=0 must render '{RESETS_TODAY}' (got: {labels})"
        );
        // And must NOT render the "N days until reset" form.
        assert!(
            !labels.contains("days until reset"),
            "days_until_reset=0 must NOT render 'days until reset' (got: {labels})"
        );
    }

    // ===== Boundary #3: NIC in history not current → "(disconnected)" =====

    #[test]
    fn history_nic_absent_from_current_is_disconnected() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![nic(99, "Ethernet", 40, 10)],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(DISCONNECTED_TAG),
            "history NIC absent from current must be annotated '{DISCONNECTED_TAG}' (got: {labels})"
        );
    }

    // ===== Pure-fn sanity: nic_row + reset_countdown_label + history_row =====

    #[test]
    fn nic_row_formats_rx_tx_total() {
        let n = nic(1, "Wi-Fi", 50, 20);
        let row = nic_row(&n, &default_display());
        // format_bytes uses 3 sig figs (T-30): 50GB → "50.0 GB", 70GB → "70.0 GB".
        assert!(row.contains("Wi-Fi"));
        assert!(row.contains("50.0 GB"), "got: {row}");
        assert!(row.contains("20.0 GB"), "got: {row}");
        assert!(row.contains("70.0 GB"), "got: {row}");
    }

    #[test]
    fn reset_countdown_label_with_days() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let s = reset_countdown_label(&view);
        assert_eq!(s, "12 days until reset (2026-07-31)");
    }

    #[test]
    fn reset_countdown_label_resets_today() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 0,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        assert_eq!(reset_countdown_label(&view), RESETS_TODAY);
    }

    #[test]
    fn history_row_with_disconnected_tag() {
        let n = nic(99, "Ethernet", 40, 10);
        let row = history_row(&n, &default_display(), true);
        assert!(row.contains(DISCONNECTED_TAG));
        assert!(row.contains("Ethernet"));
    }

    #[test]
    fn history_row_without_disconnected_tag() {
        let n = nic(1, "Wi-Fi", 40, 10);
        let row = history_row(&n, &default_display(), false);
        assert!(!row.contains(DISCONNECTED_TAG));
    }

    #[test]
    fn export_bandwidth_csv_quotes_adapter_names() {
        let view = BandwidthView {
            current: vec![nic(7, "Wi-Fi, \"Lab\"\nVPN", 1, 2)],
            history: vec![],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };

        // RFC 4180: CRLF line terminators, embedded newlines replaced with
        // space to avoid breaking row boundaries in strict parsers.
        assert_eq!(
            export_bandwidth_csv(&view),
            "luid,adapter_name,rx_bytes,tx_bytes\r\n7,\"Wi-Fi, \"\"Lab\"\" VPN\",1000000000,2000000000\r\n"
        );
    }

    // ===== v1.0 audit 1-A — degraded banner surfaces persistent archive failure

    /// Cited: v1.0 audit Iteration 1-A. When `BandwidthView::degraded` is
    /// true, the panel MUST render [`DEGRADED_BANNER`] so the user knows the
    /// cycle total is stuck (instead of silently showing a stale number).
    #[test]
    fn degraded_renders_banner() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: true,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            labels.contains(DEGRADED_BANNER),
            "degraded view must render banner (got: {labels})"
        );
    }

    /// Cited: v1.0 audit Iteration 1-A. A healthy view MUST NOT render the
    /// degraded banner — guards against accidentally defaulting it on.
    #[test]
    fn healthy_does_not_render_degraded_banner() {
        let view = BandwidthView {
            current: vec![nic(1, "Wi-Fi", 50, 20)],
            history: vec![],
            days_until_reset: 12,
            next_reset_date: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            degraded: false,
        };
        let display = default_display();
        let mut harness = Harness::new_ui(|ui| {
            render(ui, &view, &display);
        });
        harness.run();
        let labels = all_labels(&harness).join(" | ");
        assert!(
            !labels.contains(DEGRADED_BANNER),
            "healthy view must not render degraded banner (got: {labels})"
        );
    }
}
