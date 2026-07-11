//! Story 12.7 — localization scaffold.
//!
//! Doc-marked DEFERRED per epics-and-stories.md. v1 ships locale-stable
//! (OQ-5: `.` decimal separator, no thousands separator). This module
//! defines the `Locale` enum + the `v1_default()` so the format API can
//! accept a locale param once the v1 locale-stable behavior is proven.
//!
//! Cited: Story 12.7 DoD, PRD OQ-5, nfr-thresholds.md T-28/T-29/T-30.

/// Supported locales. v1 ships `LocaleStable` only; `En`/`De`/`Fr` etc.
/// land post-v1 alongside per-locale label tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    /// v1 default: locale-stable (`.` decimal, no thousands sep, 24h clock).
    /// All format_* functions assume this; they don't yet take a Locale param.
    LocaleStable,
}

impl Locale {
    /// The v1 default locale. Story 12.7 acceptance: until format_* functions
    /// gain a Locale param, this is the implicit default everywhere.
    #[must_use]
    pub const fn v1_default() -> Self {
        Self::LocaleStable
    }
}

/// The decimal separator for `locale`. v1 returns `.` for LocaleStable;
/// future locales may return `,`.
#[must_use]
pub fn decimal_separator(locale: Locale) -> char {
    match locale {
        Locale::LocaleStable => '.',
    }
}

/// The thousands separator for `locale`. v1 returns `\0` (no separator)
/// per OQ-5; future locales may return `.` or `,`.
#[must_use]
pub fn thousands_separator(locale: Locale) -> Option<char> {
    match locale {
        Locale::LocaleStable => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decimal_separator, thousands_separator, Locale};

    #[test]
    fn v1_default_is_locale_stable() {
        assert_eq!(Locale::v1_default(), Locale::LocaleStable);
    }

    #[test]
    fn locale_stable_uses_dot_decimal() {
        assert_eq!(decimal_separator(Locale::LocaleStable), '.');
    }

    #[test]
    fn locale_stable_has_no_thousands_separator() {
        assert!(thousands_separator(Locale::LocaleStable).is_none());
    }
}
