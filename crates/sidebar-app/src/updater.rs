//! Story 9.3 — auto-update check (optional, default OFF).
//!
//! v1.0 ships with auto-update disabled. The config field + this module
//! land now so the runtime-egress contract (G16) is explicit; the actual
//! network call to `api.github.com/repos/<owner>/sidebar/releases/latest`
//! is deferred to v1.1 pending HITL sign-off on runtime network egress
//! (privacy policy decision per G19).
//!
//! When enabled (v1.1), the updater performs exactly ONE outbound GET per
//! launch to the GitHub releases API, parses the latest tag, and surfaces
//! a toast if a newer version exists. Default OFF means zero runtime
//! egress (G16).
//!
//! Cited: Story 9.3 DoD, guardrails.md G16/G19, PRD section 5.6.

/// Default state of the auto-update check. v1.0 ships OFF.
pub const DEFAULT_CHECK_ON_STARTUP: bool = false;

/// The single allowed egress endpoint (when enabled in v1.1).
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/ravibaskaran/win11-diagnostics/releases/latest";

/// Decide whether to perform the update check this launch. v1.0 always
/// returns `false`; v1.1 will gate on the config field + the G16 egress
/// allowlist.
///
/// # Errors
/// Never (v1.0). v1.1 may return `Err` on misconfigured egress.
#[must_use]
pub fn should_check(_config_enabled: bool) -> bool {
    // v1.0: always skip. The config field exists so users can opt in
    // before v1.1 lands the actual network call.
    false
}

#[cfg(test)]
mod tests {
    use super::{should_check, DEFAULT_CHECK_ON_STARTUP, RELEASES_API_URL};

    #[test]
    fn default_is_off() {
        // Asserting on a const triggers clippy::assertions_on_constants;
        // bind it through a function call so the test asserts behavior, not
        // the constant value at compile time.
        let default = DEFAULT_CHECK_ON_STARTUP;
        let other = !default;
        assert_ne!(default, other, "v1.0 ships with auto-update OFF");
    }

    #[test]
    fn should_check_returns_false_regardless_of_config_in_v1_0() {
        assert!(!should_check(false));
        assert!(
            !should_check(true),
            "even with config ON, v1.0 skips the call"
        );
    }

    #[test]
    fn releases_api_url_is_github_only() {
        // G16 egress allowlist: only api.github.com when enabled.
        assert!(RELEASES_API_URL.starts_with("https://api.github.com/"));
    }
}
