//! v1.0 parity localization (i18n) — per-locale label-table system.
//!
//! The reference SidebarDiagnostics ships 48 community translations. sidebar
//! v1.0 ships the i18n *infrastructure* (label-key lookup + a Language
//! selector + two languages: English `en` as the canonical base + Spanish
//! `es` as the proof-of-concept second language) so adding more languages
//! post-v1 is a pure data contribution (one match arm per `Label`).
//!
//! ## Design
//!
//! - [`Label`] is a closed enum of every user-visible string the GUI emits.
//!   Adding a new GUI string = adding a variant + a line in each language's
//!   `translate` match (the compiler enforces coverage via the exhaustive
//!   match — a missing variant fails to compile).
//! - [`Language`] is the user-selectable locale (`en`, `es`, …). Stored in
//!   `Config.language`; the GUI calls [`t`] to resolve a label.
//! - [`t`] looks up the label in the requested language, falling back to
//!   English when a language's table omits a key (defensive — should never
//!   happen given the exhaustive match, but guards against future partial
//!   tables).
//!
//! ## Adding a language (post-v1)
//!
//! 1. Add a `Language` variant + its `code()` / `display_name()`.
//! 2. Add a match arm in `translate()` that covers every `Label` variant
//!    (the compiler will list any you miss).
//! 3. Add the variant to the `Language::all()` slice so the Settings picker
//!    surfaces it.
//!
//! ## Cited
//! PRD §3 (v1.0 parity: localization), OQ-5 (locale-stable → extensible),
//! Story 12.7 (prior DEFER → now IN via this module).

/// A user-visible GUI string. Every variant MUST appear in every language's
/// `translate` match — the exhaustive match is the coverage gate.
///
/// Keep this list focused on stable UI chrome (section headings, button
/// labels, toggle text). Dynamic data (sensor names, formatted values) is
/// not localized — it comes from the hardware/OS.
///
/// Variant names are descriptive (kebab-case self-documents the key) so
/// per-variant doc comments would be pure noise; `missing_docs` is allowed
/// on this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names, missing_docs)]
pub enum Label {
    // Settings section headings.
    BillingCycleStartDay,
    TemperatureUnit,
    SizeUnits,
    RefreshRate,
    TemperatureAlerts,
    Appearance,
    Position,
    Startup,
    Metrics,
    Hotkeys,
    DockedEdge,
    TargetMonitor,
    Theme,
    Language,
    // Common verbs / nouns.
    Acknowledge,
    Snooze5m,
    OpenSettings,
    ExportCsv,
    StartMonitoring,
    SetupSaved,
    // Section sub-labels.
    SidebarWidth,
    FontSize,
    UiScale,
    BlinkAlerts,
    BackgroundColor,
    BgOpacity,
    FontColor,
    HorizontalOffset,
    VerticalOffset,
    StartHidden,
    PauseSensorsWhenHidden,
    StartSidebarWhenWindowsStarts,
    // Graph popup.
    HistorySamples,
    WaitingForSamples,
    CurrentMinMax,
    // Degradation / status.
    HardwareMonitorStopped,
    SensorDataStale,
}

/// A selectable UI locale. v1.0 ships `English` (canonical) + `Spanish`
/// (proof-of-concept). Post-v1, adding a language is a pure-data change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// English — the canonical base; every label MUST exist here.
    #[default]
    English,
    /// Español — proof-of-concept second language (v1.0).
    Spanish,
}

impl Language {
    /// The BCP-47-ish code stored in config.toml (`en`, `es`).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
        }
    }

    /// Parse a config code back into a `Language`. Unknown codes (including
    /// future codes added post-v1) fall back to English.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "es" | "esp" | "spanish" | "español" => Language::Spanish,
            // Default to English for `en`, empty, and any unrecognized code.
            _ => Language::English,
        }
    }

    /// The endonym/display name shown in the Language picker (in the language
    /// itself, the convention for locale selectors).
    #[must_use]
    pub fn display_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
        }
    }

    /// All languages the build supports, in picker order. English first so
    /// it's the default-selection anchor.
    ///
    /// v1.0 ships English-only. Spanish translations exist in the i18n table
    /// for section headings + action buttons, but many sub-labels, hotkey
    /// descriptions, bandwidth panel strings, and the About dialog remain
    /// hardcoded English. Shipping a half-translated UI is worse than
    /// English-only — the Language picker returns when full coverage lands.
    #[must_use]
    pub fn all() -> &'static [Language] {
        &[Language::English]
    }
}

/// Resolve a label for the given language, falling back to English if the
/// language's table omits the key (defensive — the exhaustive match makes
/// this unreachable in practice, but guards future partial tables).
///
/// # Example
/// ```
/// use sidebar_app::i18n::{t, Label, Language};
/// assert_eq!(t(Language::English, Label::Metrics), "Metrics");
/// assert_eq!(t(Language::Spanish, Label::Metrics), "Métricas");
/// ```
#[must_use]
pub fn t(lang: Language, label: Label) -> &'static str {
    match lang {
        Language::English => translate_en(label),
        Language::Spanish => translate_es(label).unwrap_or_else(|| translate_en(label)),
    }
}

/// Canonical English table. Every label MUST appear here.
fn translate_en(l: Label) -> &'static str {
    match l {
        Label::BillingCycleStartDay => "Billing cycle start day",
        Label::TemperatureUnit => "Temperature unit",
        Label::SizeUnits => "Size units",
        Label::RefreshRate => "Refresh rate (seconds)",
        Label::TemperatureAlerts => "Temperature alerts",
        Label::Appearance => "Appearance",
        Label::Position => "Position",
        Label::Startup => "Startup",
        Label::Metrics => "Metrics",
        Label::Hotkeys => "Hotkeys",
        Label::DockedEdge => "Docked edge",
        Label::TargetMonitor => "Target monitor",
        Label::Theme => "Theme",
        Label::Language => "Language",
        Label::Acknowledge => "Acknowledge",
        Label::Snooze5m => "Snooze 5m",
        Label::OpenSettings => "Open settings",
        Label::ExportCsv => "Export CSV",
        Label::StartMonitoring => "Start monitoring",
        Label::SetupSaved => "Setup saved.",
        Label::SidebarWidth => "Sidebar width",
        Label::FontSize => "Font size",
        Label::UiScale => "UI scale",
        Label::BlinkAlerts => "Blink alerts",
        Label::BackgroundColor => "Background color",
        Label::BgOpacity => "Opacity",
        Label::FontColor => "Font color",
        Label::HorizontalOffset => "Horizontal offset",
        Label::VerticalOffset => "Vertical offset",
        Label::StartHidden => "Start hidden (show via tray)",
        Label::PauseSensorsWhenHidden => "Pause sensors when hidden",
        Label::StartSidebarWhenWindowsStarts => "Start sidebar when Windows starts",
        Label::HistorySamples => "history",
        Label::WaitingForSamples => "Waiting for samples…",
        Label::CurrentMinMax => "current",
        Label::HardwareMonitorStopped => "Hardware monitor stopped. Click the pill to restart it.",
        Label::SensorDataStale => "Sensor data is stale. The hardware monitor may be unresponsive.",
    }
}

/// Spanish table (proof-of-concept). Returns `None` for any key not yet
/// translated so the caller falls back to English; the exhaustive match
/// below ensures every variant is at least considered.
///
/// The `Option` wrap is the documented fallback contract — `t()` uses it
/// to fall back to English — so `unnecessary_wraps` is allowed here.
#[allow(clippy::unnecessary_wraps)]
fn translate_es(l: Label) -> Option<&'static str> {
    Some(match l {
        Label::BillingCycleStartDay => "Día de inicio del ciclo de facturación",
        Label::TemperatureUnit => "Unidad de temperatura",
        Label::SizeUnits => "Unidades de tamaño",
        Label::RefreshRate => "Frecuencia de actualización (segundos)",
        Label::TemperatureAlerts => "Alertas de temperatura",
        Label::Appearance => "Apariencia",
        Label::Position => "Posición",
        Label::Startup => "Inicio",
        Label::Metrics => "Métricas",
        Label::Hotkeys => "Atajos de teclado",
        Label::DockedEdge => "Borde acoplado",
        Label::TargetMonitor => "Monitor de destino",
        Label::Theme => "Tema",
        Label::Language => "Idioma",
        Label::Acknowledge => "Confirmar",
        Label::Snooze5m => "Posponer 5m",
        Label::OpenSettings => "Abrir ajustes",
        Label::ExportCsv => "Exportar CSV",
        Label::StartMonitoring => "Iniciar monitorización",
        Label::SetupSaved => "Configuración guardada.",
        Label::SidebarWidth => "Ancho de la barra lateral",
        Label::FontSize => "Tamaño de fuente",
        Label::UiScale => "Escala de interfaz",
        Label::BlinkAlerts => "Parpadear alertas",
        Label::BackgroundColor => "Color de fondo",
        Label::BgOpacity => "Opacidad",
        Label::FontColor => "Color de fuente",
        Label::HorizontalOffset => "Desplazamiento horizontal",
        Label::VerticalOffset => "Desplazamiento vertical",
        Label::StartHidden => "Iniciar oculto (mostrar desde la bandeja)",
        Label::PauseSensorsWhenHidden => "Pausar sensores cuando esté oculto",
        Label::StartSidebarWhenWindowsStarts => "Iniciar la barra lateral con Windows",
        Label::HistorySamples => "historial",
        Label::WaitingForSamples => "Esperando muestras…",
        Label::CurrentMinMax => "actual",
        Label::HardwareMonitorStopped => {
            "El monitor de hardware se detuvo. Haga clic en el indicador para reiniciarlo."
        }
        Label::SensorDataStale => {
            "Los datos del sensor están desactualizados. El monitor de hardware puede no responder."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every label must resolve to a non-empty string in English (the
    /// canonical base). This is the coverage gate for the en table.
    #[test]
    fn english_table_covers_every_label_with_nonempty_text() {
        for label in all_labels() {
            let s = t(Language::English, label);
            assert!(!s.is_empty(), "English label {label:?} is empty");
        }
    }

    /// The Spanish table must cover every label (no fallback to English).
    /// This test stays even though Spanish is not in `all()` for v1.0 — it
    /// ensures the table is complete when the language is re-enabled.
    #[test]
    fn spanish_table_covers_every_label() {
        for label in all_labels() {
            let en = t(Language::English, label);
            let es = t(Language::Spanish, label);
            assert_ne!(
                es, en,
                "Spanish label {label:?} fell back to English — add the translation"
            );
        }
    }

    /// v1.0 — only English ships in the Language picker. Spanish returns
    /// when full coverage lands (the table is complete, but sub-labels in
    /// settings/bandwidth/about are still hardcoded English).
    #[test]
    fn language_picker_ships_english_only() {
        assert_eq!(Language::all(), &[Language::English]);
    }

    /// `from_code` round-trips every language's code + accepts common aliases.
    #[test]
    fn from_code_round_trips_and_accepts_aliases() {
        for &lang in Language::all() {
            assert_eq!(Language::from_code(lang.code()), lang);
        }
        // Aliases + unknown → English.
        assert_eq!(Language::from_code("español"), Language::Spanish);
        assert_eq!(Language::from_code(""), Language::English);
        assert_eq!(Language::from_code("fr"), Language::English); // post-v1
    }

    /// `display_name` returns the endonym (the language's own name).
    #[test]
    fn display_name_returns_endonym() {
        assert_eq!(Language::English.display_name(), "English");
        assert_eq!(Language::Spanish.display_name(), "Español");
    }

    /// Sample spot-check that translations actually differ.
    #[test]
    fn spanish_metrics_label_differs_from_english() {
        assert_eq!(t(Language::English, Label::Metrics), "Metrics");
        assert_eq!(t(Language::Spanish, Label::Metrics), "Métricas");
    }

    /// A helper that iterates every `Label` variant. Kept here (not on the
    /// enum) so production code doesn't carry the array; the test is the
    /// only consumer.
    fn all_labels() -> Vec<Label> {
        vec![
            Label::BillingCycleStartDay,
            Label::TemperatureUnit,
            Label::SizeUnits,
            Label::RefreshRate,
            Label::TemperatureAlerts,
            Label::Appearance,
            Label::Position,
            Label::Startup,
            Label::Metrics,
            Label::Hotkeys,
            Label::DockedEdge,
            Label::TargetMonitor,
            Label::Theme,
            Label::Language,
            Label::Acknowledge,
            Label::Snooze5m,
            Label::OpenSettings,
            Label::ExportCsv,
            Label::StartMonitoring,
            Label::SetupSaved,
            Label::SidebarWidth,
            Label::FontSize,
            Label::UiScale,
            Label::BlinkAlerts,
            Label::BackgroundColor,
            Label::BgOpacity,
            Label::FontColor,
            Label::HorizontalOffset,
            Label::VerticalOffset,
            Label::StartHidden,
            Label::PauseSensorsWhenHidden,
            Label::StartSidebarWhenWindowsStarts,
            Label::HistorySamples,
            Label::WaitingForSamples,
            Label::CurrentMinMax,
            Label::HardwareMonitorStopped,
            Label::SensorDataStale,
        ]
    }
}
