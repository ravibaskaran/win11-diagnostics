//! Story 7.3 — Two-Tier Auto-Detect Probe (launch-time).
//!
//! This module is the **read-only** launch-time probe that classifies the
//! sidebar's runtime tier ([`ProviderTier::Basic`] vs [`ProviderTier::Full`])
//! by delegating to [`OhmSupervisor::probe`] (Story 6.4). It runs once at
//! sidebar startup BEFORE the GUI / poller spin up; the resolved tier is the
//! initial value of the runtime `AppState.tier` (Story 8.1 will define the
//! `AppState` struct; for now we expose the [`ProbeResult`] the launch
//! sequence consumes).
//!
//! ## No-UAC guarantee (G11, success metric)
//!
//! This probe is **read-only** — it issues HTTP GETs against
//! `http://127.0.0.1:<port>/data.json` and inspects the body. It NEVER calls
//! [`OhmSupervisor::launch_elevated`] (the only UAC-triggering code path in
//! the codebase). On a default first launch with LHM not installed, sidebar
//! MUST come up at Basic tier with **zero** UAC prompts. UAC is a
//! user-initiated action (the status pill click in Story 8.2); the launch
//! probe's job is purely to detect. This is the Story 7.3 success metric.
//!
//! ## Probe sequence (T-10 + T-45)
//!
//! 1. Probe the configured port (`[ohm] http_port`, default 17127 per
//!    Story 1.5) via [`OhmSupervisor::probe`]. T-10 caps the underlying HTTP
//!    GET at 500ms; [`OhmSupervisor::probe`] already maps timeout → Basic.
//! 2. If that returns Full, we're done — Full tier with the resolved port.
//! 3. If it returns Basic (port occupied by a non-LHM service, or connection
//!    refused, or timeout), walk the T-45 fallback chain (17128..17137)
//!    one port at a time via [`OhmSupervisor::probe`]. The first port that
//!    returns Full wins; its port is the resolved port.
//! 4. If no port in the chain returns Full, the probe returns Basic with a
//!    hint: "LHM not running — click the status pill to install/launch it"
//!    (when the config port refused connection) OR "LHM port unavailable —
//!    all T-45 candidates occupied" (when every port was a non-LHM body).
//!
//! The chain is the same shape as [`pick_free_port`] in Story 6.4, but we
//! CANNOT reuse `pick_free_port` directly because its semantics differ: it
//! returns the first FREE port (connection-refused OR LHM-signature), which
//! is what we want for `launch_elevated`. The launch probe instead wants the
//! first LHM-ANSWERING port; a connection-refused port is NOT interesting to
//! us here (we never launch — we only detect). So we walk the chain with
//! [`OhmSupervisor::probe`] directly.
//!
//! ## Tier-change broadcast (T-38)
//!
//! If the caller supplies `previous_tier: Some(Basic)` and the probe resolves
//! Full, OR `previous_tier: Some(Full)` and the probe resolves Basic, the
//! [`TierChangeCallback`] (Story 6.4 seam) is fired. The actual coalesced
//! `Event::TierChanged` channel lands in Story 7.4 — here we just hook the
//! callback so the wiring is real.
//!
//! ## Cited
//!
//! - Story 7.3 TDD contract (Happy Path #1-#2, Boundary #1-#4)
//! - architecture.md §5.2 + AD-7 (revised)
//! - nfr-thresholds.md T-10 (500ms probe timeout), T-38 (tier-change
//!   broadcast 500ms coalesce), T-45 (port fallback 17127-17137)
//! - guardrails.md G11 (HITL on "no UAC on default first launch")

use sidebar_adapter_ohm::http::{HttpClient, OhmError};
use sidebar_platform::ohm_supervisor::{
    is_lhm_signature, OhmSupervisor, TierChangeCallback, PORT_RANGE_END, PORT_RANGE_START,
};
use sidebar_sensor::descriptor::ProviderTier;

/// Outcome of the launch-time probe. Consumed by the launch sequence to seed
/// `AppState.tier` (Story 8.1) and drive the status-pill rendering (Full vs
/// Basic).
///
/// `hint` carries an actionable, user-facing string surfaced via the status
/// pill (e.g. "LHM not running — click to install"). It is `None` when the
/// probe resolved Full cleanly OR when there is nothing actionable to say
/// (Basic on default config, no LHM installed → hint is "install LHM").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// The resolved tier — `Full` if any port in the T-45 chain returned the
    /// LHM signature, otherwise `Basic`.
    pub tier: ProviderTier,
    /// The port on which LHM was detected (only set when `tier == Full`).
    /// When `Basic`, this is `None` — there is no resolved port.
    pub resolved_port: Option<u16>,
    /// Optional user-facing hint, surfaced via the status pill. `None` when
    /// the probe cleanly resolved Full OR when the hint is empty.
    pub hint: Option<String>,
}

/// Run the launch-time two-tier auto-detect probe against the given
/// supervisor.
///
/// Walks the T-45 chain starting at `config_port` (the user's configured
/// `[ohm] http_port`, default 17127 per Story 1.5) then 17128..17137 if the
/// config port returned Basic. The first port returning Full wins; if none
/// do, the result is Basic + a hint.
///
/// **Never calls `launch_elevated`** — this is the no-UAC guarantee.
///
/// If `previous_tier` is `Some` and differs from the resolved tier, the
/// `tier_change` callback is fired (T-38 broadcast seam; Story 7.4 wires the
/// real coalesced `Event::TierChanged` channel on top of this).
///
/// # Arguments
///
/// - `supervisor` — the [`OhmSupervisor`] (generic over `C: HttpClient`).
///   Production wires a `RealHttpClient`; tests inject a `MockHttpClient`.
/// - `config_port` — the user-configured `[ohm] http_port` from Story 1.5.
///   Default 17127. The probe tries this first; on Basic it walks the rest
///   of the T-45 chain.
/// - `previous_tier` — the tier recorded from the previous session
///   (`Some(Full)` if LHM was running when sidebar last exited, `None` on
///   first launch). Used to fire the tier-change callback on transition.
/// - `tier_change` — optional T-38 broadcast callback. Fired exactly once if
///   `previous_tier` differs from the resolved tier; not fired otherwise.
///
/// # Notes
///
/// - The T-45 walk is bounded: at most 10 HTTP probes × 500ms T-10 timeout =
///   ≤ 5s worst case (every port times out). In practice a connection-refused
///   is sub-millisecond on localhost, so the chain walks in milliseconds.
/// - `config_port` outside the T-45 range (e.g. a user-configured 8085) is
///   tried first as-is; the fallback chain still walks 17127..17137 (per the
///   spec, the fallback is anchored at the canonical 17127 — user config
///   just changes which port is tried first).
#[must_use]
pub fn run_launch_probe<C: HttpClient>(
    supervisor: &OhmSupervisor<C>,
    config_port: u16,
    previous_tier: Option<ProviderTier>,
    tier_change: Option<&TierChangeCallback>,
) -> ProbeResult {
    // Build the probe walk order: config_port first, then the rest of the
    // T-45 chain (17127..17137) skipping whichever port == config_port so
    // we don't probe it twice. If config_port is OUTSIDE the T-45 range
    // (e.g. user-configured 8085), it's tried first then the full chain.
    let mut ports: Vec<u16> = Vec::with_capacity(11);
    ports.push(config_port);
    for p in PORT_RANGE_START..=PORT_RANGE_END {
        if p != config_port {
            ports.push(p);
        }
    }

    // Walk the chain. The first port that returns Full wins. We also classify
    // the failure mode to compose the right hint:
    // - any port returned a non-LHM body → "port unavailable" hint
    // - every port refused/timeout → "install LHM" hint
    let mut saw_non_lhm_body = false;
    let mut saw_connection_refused = false;
    for port in ports {
        match supervisor.probe(port) {
            ProviderTier::Full => {
                let result = ProbeResult {
                    tier: ProviderTier::Full,
                    resolved_port: Some(port),
                    hint: None,
                };
                fire_tier_change(tier_change, previous_tier, ProviderTier::Full);
                tracing::info!(port, config_port, "launch probe resolved Full tier");
                return result;
            }
            ProviderTier::Basic | ProviderTier::Both => {
                // We can't directly tell from `probe()`'s Basic return
                // WHETHER the port refused connection (free) vs returned a
                // foreign body (occupied) — the supervisor collapses both to
                // Basic. We re-probe via the supervisor's client to classify
                // the failure mode for the hint. This is a second 500ms-T-10
                // HTTP GET per non-Full port; in practice the refused/timeout
                // path is sub-ms on localhost, so the cost is negligible.
                //
                // The re-probe is read-only and never triggers UAC.
                if let Some(reason) = classify_basic_port(supervisor, port) {
                    if reason == BasicReason::NonLhmBody {
                        saw_non_lhm_body = true;
                    } else {
                        saw_connection_refused = true;
                    }
                }
                tracing::debug!(
                    port,
                    config_port,
                    "launch probe: port returned Basic, walking T-45 chain"
                );
            }
        }
    }

    // No port in the chain returned Full — Basic tier. Compose the hint.
    let hint_str = compose_basic_hint(saw_non_lhm_body, saw_connection_refused);
    let hint = Some(hint_str);
    let result = ProbeResult {
        tier: ProviderTier::Basic,
        resolved_port: None,
        hint,
    };
    fire_tier_change(tier_change, previous_tier, ProviderTier::Basic);
    tracing::info!(
        config_port,
        saw_non_lhm_body,
        saw_connection_refused,
        "launch probe resolved Basic tier (no LHM detected on T-45 chain)"
    );
    result
}

/// Reasons a port returned Basic (used to compose the right user-facing hint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BasicReason {
    /// Connection refused / timeout — the port is free, LHM just isn't
    /// running. Hint: "install / launch LHM".
    Free,
    /// Something answered with a non-LHM body — port occupied by a foreign
    /// service. Hint: "LHM port unavailable".
    NonLhmBody,
}

/// Re-probe `port` via the supervisor's underlying client to classify WHY it
/// returned Basic. Returns `None` if the second probe's outcome doesn't match
/// the first (a flaky port — shouldn't happen on localhost; treat as
/// inconclusive).
///
/// This issues one more HTTP GET (T-10 bounded). Read-only — no UAC. The
/// first probe via `OhmSupervisor::probe` already collapsed the failure to
/// Basic for the tier decision; this second probe inspects the raw error /
/// body to compose the right user-facing hint.
fn classify_basic_port<C: HttpClient>(
    supervisor: &OhmSupervisor<C>,
    port: u16,
) -> Option<BasicReason> {
    let url = format!("http://127.0.0.1:{port}/data.json");
    let client = supervisor.client();
    match client.get(&url) {
        Ok(body) => {
            // Something answered. If it's the LHM signature, the first
            // probe() should have returned Full — a mismatch means the port
            // is flaky or LHM just came up between probes. Treat as
            // inconclusive (None) so we don't mislead the hint.
            if is_lhm_signature(&body) {
                None
            } else {
                Some(BasicReason::NonLhmBody)
            }
        }
        // Both connection-refused and timeout mean the port is free (LHM just
        // isn't running). T-10 timeouts on localhost almost always indicate
        // nothing is listening — a foreign service that accepted the
        // connection but didn't respond would still register as "free" from
        // our perspective, since launch_elevated will retry the port with its
        // own T-11 budget.
        Err(OhmError::HttpFailed(_) | OhmError::Timeout) => Some(BasicReason::Free),
        // Non-JSON body or JSON parse failure — a foreign service answered.
        Err(OhmError::NotJson(_) | OhmError::Parse(_)) => Some(BasicReason::NonLhmBody),
        // G16 rejection is a policy signal, not evidence that a foreign
        // service occupies the port. Keep the hint inconclusive.
        Err(OhmError::RejectedUrl(reason)) => {
            tracing::warn!(%reason, port, "launch probe URL rejected by G16 policy");
            None
        }
    }
}

/// Compose the Basic-tier hint from the per-port observations.
///
/// - If at least one port refused connection (free port, LHM just not
///   running) → "LHM not detected — click the status pill to install/launch
///   LibreHardwareMonitor". This is the dominant case.
/// - If no port refused (every port was occupied or timed out) → "LHM port
///   unavailable — all T-45 candidates (17127-17137) occupied by other
///   services". The user needs to free up a port or reconfigure.
fn compose_basic_hint(saw_non_lhm_body: bool, saw_connection_refused: bool) -> String {
    if saw_connection_refused {
        // At least one port was free — LHM just isn't running.
        "LHM not detected — click the status pill to install/launch LibreHardwareMonitor \
             (will request elevation)"
            .to_string()
    } else if saw_non_lhm_body {
        // Every port was occupied by a non-LHM service.
        format!(
            "LHM port unavailable — all T-45 candidates ({PORT_RANGE_START}-{PORT_RANGE_END}) \
             occupied by other services"
        )
    } else {
        // Inconclusive (every probe timed out, or classify returned None for
        // all ports). Default to the install hint — the user clicking the
        // pill will surface the real error via launch_elevated.
        "LHM not detected — click the status pill to install/launch LibreHardwareMonitor \
             (will request elevation)"
            .to_string()
    }
}

/// Fire the T-38 tier-change callback if the tier transitioned. No-op when
/// `previous_tier == Some(resolved)` OR when no callback is wired. The actual
/// coalesced `Event::TierChanged` channel lands in Story 7.4; this is the
/// seam.
fn fire_tier_change(
    tier_change: Option<&TierChangeCallback>,
    previous_tier: Option<ProviderTier>,
    resolved: ProviderTier,
) {
    let Some(cb) = tier_change else {
        return;
    };
    match previous_tier {
        Some(prev) if prev != resolved => {
            cb(resolved);
        }
        // No previous (first launch) OR no transition — do NOT fire. First
        // launch resolving Full is not a transition; it's the initial state.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    //! Story 7.3 TDD contract tests (RED — stub returns Basic, so the
    //! Full-resolving tests fail until the GREEN commit).
    //!
    //! The mock HttpClient here mirrors the pattern in
    //! `sidebar-platform/src/ohm_supervisor.rs` Story 6.4 tests: a hand-rolled
    //! `mock!` HttpClient whose `.expect_get()` controls the per-port response.
    //! The supervisor is constructed against a TempDir (the LHM exe path
    //! doesn't need to exist for `probe()` — only `launch_elevated` checks).
    //!
    //! Cited:
    //!   - Story 7.3 TDD contract (Happy Path #1-#2, Boundary #1-#4)
    //!   - nfr-thresholds.md T-10 (500ms), T-38 (broadcast coalesce), T-45
    //!   - guardrails.md G11 (no UAC on default first launch — success metric)

    use super::*;
    use mockall::mock;
    use sidebar_adapter_ohm::http::OhmError;
    use sidebar_platform::ohm_supervisor::OhmSupervisor;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// Minimal LHM-shaped `/data.json` body — top-level array, first element
    /// has `Text` + `Children` (PascalCase signature, the canonical LHM v0.9.x
    /// shape).
    const LHM_BODY: &str = r#"[
      { "id": "/", "Text": "root", "Type": "Node",
        "Children": [
          { "id": "/amdcpu/0", "Text": "AMD Ryzen", "Type": "Node", "Children": [] }
        ]
      }
    ]"#;

    /// A non-LHM body (HTML 404 from a foreign service on the port).
    const NON_LHM_BODY: &str = "<html><body>404 Not Found</body></html>";

    // Auto-mock HttpClient for probe tests (mirrors the ohm adapter +
    // supervisor test pattern).
    mock! {
        pub FakeClient {}
        impl HttpClient for FakeClient {
            fn get(&self, url: &str) -> std::result::Result<String, OhmError>;
        }
    }

    /// Build a supervisor pointing at a TempDir. The LHM exe path need not
    /// exist for `probe()` (only `launch_elevated` checks); we never call
    /// `launch_elevated` here — that is the no-UAC guarantee.
    fn supervisor(client: MockFakeClient) -> (OhmSupervisor<MockFakeClient>, TempDir) {
        let dir = TempDir::new().expect("TempDir");
        let sv = OhmSupervisor::new(client, PathBuf::from(dir.path()));
        (sv, dir)
    }

    // ==========================================================
    // Happy Path #1 — probe resolves Full on the config port
    // ==========================================================

    /// Story 7.3 Happy Path #1. Mock probe returns LHM JSON on the config
    /// port (17127) → `tier = Full`, `resolved_port = Some(17127)`, no hint.
    /// Cited: Story 7.3 TDD contract.
    #[test]
    fn probe_resolves_full_when_lhm_answers_on_config_port() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(LHM_BODY.to_string()));
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Full, "LHM answered → Full");
        assert_eq!(
            result.resolved_port,
            Some(17_127),
            "resolved_port = config port on Full"
        );
        assert!(result.hint.is_none(), "no hint on clean Full resolution");
    }

    // ==========================================================
    // Happy Path #2 — probe resolves Basic, no UAC, hint surfaced
    // ==========================================================

    /// Story 7.3 Happy Path #2. Mock probe returns connection-refused on every
    /// port in the chain → `tier = Basic`, no UAC (verified by construction:
    /// `run_launch_probe` never invokes `launch_elevated`). A hint is
    /// surfaced to the user via the status pill.
    /// Cited: Story 7.3 TDD contract + G11 (no UAC on default first launch).
    #[test]
    fn probe_resolves_basic_with_hint_when_lhm_not_running() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Basic, "no LHM → Basic");
        assert!(result.resolved_port.is_none(), "no resolved port on Basic");
        // Hint is surfaced so the status pill can guide the user.
        assert!(result.hint.is_some(), "Basic must carry a user-facing hint");
        let hint = result.hint.expect("hint");
        assert!(
            hint.to_lowercase().contains("lhm") || hint.to_lowercase().contains("install"),
            "hint must mention LHM/install: {hint}"
        );
    }

    // ==========================================================
    // Boundary #1 — T-10 timeout on config port → Basic
    // ==========================================================

    /// Story 7.3 Boundary #1 (T-10). The HTTP probe times out (500ms T-10)
    /// on every port in the chain → Basic. Cited: T-10.
    #[test]
    fn probe_returns_basic_on_t10_timeout() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| Err(OhmError::Timeout));
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Basic, "T-10 timeout → Basic");
        assert!(result.resolved_port.is_none());
    }

    // ==========================================================
    // Boundary — G16 rejection remains Basic without a false occupied-port hint
    // ==========================================================

    /// A rejected URL is a policy signal, not evidence that a foreign service
    /// occupies the port. The launch probe must remain Basic and use its
    /// inconclusive/install guidance rather than claiming port exhaustion.
    #[test]
    fn probe_treats_g16_rejection_as_inconclusive_basic() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| {
            Err(OhmError::RejectedUrl(
                "test-only non-loopback target".to_string(),
            ))
        });
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Basic);
        assert!(result.resolved_port.is_none());
        let hint = result.hint.expect("inconclusive Basic must carry a hint");
        assert!(
            hint.contains("not detected") && !hint.contains("port unavailable"),
            "G16 rejection must not claim port exhaustion: {hint}"
        );
    }

    #[test]
    fn probe_continues_after_g16_rejection_to_full_fallback() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|url| {
            if url.contains(":17127") {
                Err(OhmError::RejectedUrl(
                    "test-only non-loopback target".to_string(),
                ))
            } else {
                Ok(LHM_BODY.to_string())
            }
        });
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Full);
        assert_eq!(result.resolved_port, Some(17_128));
        assert!(
            result.hint.is_none(),
            "fallback Full resolution has no hint"
        );
    }

    // ==========================================================
    // Boundary #2 — LHM not installed → Basic + "install LHM" hint
    // ==========================================================

    /// Story 7.3 Boundary #2. LHM is not installed (every port refuses
    /// connection). The probe returns Basic with a hint that surfaces
    /// "install LHM" guidance to the user. Cited: Story 7.3 Boundary #2.
    #[test]
    fn probe_returns_basic_with_install_hint_when_lhm_not_installed() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Basic);
        let hint = result.hint.expect("install hint");
        assert!(
            hint.to_lowercase().contains("install") || hint.to_lowercase().contains("lhm"),
            "hint must mention install/LHM: {hint}"
        );
    }

    // ==========================================================
    // Boundary #3 — rapid relaunch: LHM already running on 17127
    // ==========================================================

    /// Story 7.3 Boundary #3. Sidebar is relaunched while LHM (from the
    /// previous session) is still answering on 17127. The probe must succeed
    /// IMMEDIATELY on the config port (one HTTP roundtrip, sub-ms). Cited:
    /// Story 7.3 Boundary #3.
    #[test]
    fn probe_succeeds_immediately_on_rapid_relaunch() {
        let call_count: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let count_clone = call_count.clone();
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(move |_| {
            *count_clone.lock().expect("count") += 1;
            Ok(LHM_BODY.to_string())
        });
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, Some(ProviderTier::Full), None);
        assert_eq!(result.tier, ProviderTier::Full);
        assert_eq!(result.resolved_port, Some(17_127));
        // The config-port probe alone resolved Full — we should NOT have
        // walked further down the fallback chain.
        let n = *call_count.lock().expect("count");
        assert_eq!(
            n, 1,
            "rapid-relaunch probe must resolve in 1 HTTP roundtrip"
        );
    }

    // ==========================================================
    // Boundary #4 — config port occupied, LHM on fallback port 17128
    // ==========================================================

    /// Story 7.3 Boundary #4. Port 17127 is occupied by a non-LHM service
    /// (returns HTML body); LHM is running on 17128. The probe must walk
    /// 17127 (Basic) → 17128 (Full) within the T-45 fallback chain. Cited:
    /// Story 7.3 Boundary #4, T-45.
    #[test]
    fn probe_falls_back_to_17128_when_17127_occupied() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|url| {
            if url.contains(":17127") {
                Ok(NON_LHM_BODY.to_string())
            } else if url.contains(":17128") {
                Ok(LHM_BODY.to_string())
            } else {
                Err(OhmError::HttpFailed("connection refused".to_string()))
            }
        });
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Full, "LHM on 17128 → Full");
        assert_eq!(
            result.resolved_port,
            Some(17_128),
            "resolved port must be the fallback 17128, not the occupied 17127"
        );
    }

    // ==========================================================
    // Tier-change broadcast (T-38) — fires on transition
    // ==========================================================

    /// Story 7.3 / T-38. Previous session ended at Basic; launch probe
    /// resolves Full (LHM came up between sessions). The tier-change callback
    /// is fired exactly once with `Full`. Cited: T-38, Story 7.3 design.
    #[test]
    fn tier_change_callback_fires_on_basic_to_full_transition() {
        let received: Arc<Mutex<Vec<ProviderTier>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = received.clone();
        let cb: TierChangeCallback = Box::new(move |tier| {
            rx.lock().expect("lock").push(tier);
        });

        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| Ok(LHM_BODY.to_string()));
        let (sv, _dir) = supervisor(mock);

        let _ = run_launch_probe(&sv, 17_127, Some(ProviderTier::Basic), Some(&cb));
        let got = received.lock().expect("lock").clone();
        assert_eq!(
            got,
            vec![ProviderTier::Full],
            "callback fired once with Full"
        );
    }

    /// Story 7.3 / T-38 inverse. Previous session ended at Full; launch probe
    /// now resolves Basic (LHM was uninstalled). The callback fires with
    /// `Basic`. Cited: T-38.
    #[test]
    fn tier_change_callback_fires_on_full_to_basic_transition() {
        let received: Arc<Mutex<Vec<ProviderTier>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = received.clone();
        let cb: TierChangeCallback = Box::new(move |tier| {
            rx.lock().expect("lock").push(tier);
        });

        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor(mock);

        let _ = run_launch_probe(&sv, 17_127, Some(ProviderTier::Full), Some(&cb));
        let got = received.lock().expect("lock").clone();
        assert_eq!(got, vec![ProviderTier::Basic]);
    }

    /// Story 7.3 / T-38 no-op. Previous tier == resolved tier → callback is
    /// NOT fired (no transition). Cited: T-38.
    #[test]
    fn tier_change_callback_not_fired_when_tier_unchanged() {
        let received: Arc<Mutex<Vec<ProviderTier>>> = Arc::new(Mutex::new(Vec::new()));
        let rx = received.clone();
        let cb: TierChangeCallback = Box::new(move |tier| {
            rx.lock().expect("lock").push(tier);
        });

        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| Ok(LHM_BODY.to_string()));
        let (sv, _dir) = supervisor(mock);

        let _ = run_launch_probe(&sv, 17_127, Some(ProviderTier::Full), Some(&cb));
        let got = received.lock().expect("lock").clone();
        assert!(got.is_empty(), "no transition → no callback");
    }

    // ==========================================================
    // No-UAC guarantee (G11, success metric)
    // ==========================================================

    /// Story 7.3 success metric (G11). The launch probe MUST NOT trigger UAC
    /// on a default first launch (LHM not installed). Since `run_launch_probe`
    /// is a pure function over the supervisor + client (no ShellExecuteW),
    /// this is verified by construction: the function signature exposes no
    /// mutable supervisor access (`&OhmSupervisor` not `&mut`) and the only
    /// method called on it is `probe` (read-only HTTP GET). This test
    /// documents the contract — it cannot trigger UAC even on the worst-case
    /// Basic path.
    #[test]
    fn launch_probe_never_triggers_uac_on_default_first_launch() {
        let mut mock = MockFakeClient::new();
        // Worst case: every port refuses connection (LHM not installed).
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor(mock);

        // previous_tier = None (first launch), no tier_change callback.
        let result = run_launch_probe(&sv, 17_127, None, None);

        // Verifies: Basic tier, no crash, no UAC. (UAC would require a
        // mutable supervisor + launch_elevated call, which this code path
        // cannot reach — the function takes &OhmSupervisor, not &mut.)
        assert_eq!(result.tier, ProviderTier::Basic);
        assert!(result.hint.is_some(), "user-facing hint on first launch");
    }

    // ==========================================================
    // All T-45 ports occupied by non-LHM → Basic + "port unavailable" hint
    // ==========================================================

    /// Story 7.3 / T-45 "out of fallback chain". Every port 17127..17137 is
    /// occupied by a non-LHM service. The probe returns Basic with a hint
    /// mentioning port unavailability. Cited: T-45.
    #[test]
    fn probe_returns_basic_when_all_t45_ports_occupied() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Ok(NON_LHM_BODY.to_string()));
        let (sv, _dir) = supervisor(mock);

        let result = run_launch_probe(&sv, 17_127, None, None);
        assert_eq!(result.tier, ProviderTier::Basic);
        let hint = result.hint.expect("hint");
        assert!(
            hint.to_lowercase().contains("port") || hint.to_lowercase().contains("unavailable"),
            "hint must mention port unavailability: {hint}"
        );
    }
}
