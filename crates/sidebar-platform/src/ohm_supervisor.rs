//! `OhmSupervisor` — LHM subprocess probe/launch/monitor/teardown (Story 6.4).
//!
//! ## Role
//!
//! LibreHardwareMonitor (LHM) runs as a bundled elevated subprocess that
//! exposes an HTTP sensor tree on `127.0.0.1:<port>/data.json`. This module
//! owns the lifecycle:
//! 1. [`OhmSupervisor::probe`] — HTTP reachability check, classifies
//!    [`ProviderTier::Full`] vs [`ProviderTier::Basic`] (AD-7).
//! 2. [`OhmSupervisor::launch_elevated`] — pick a free port (T-45 fallback
//!    chain 17127..17137), patch the LHM config file, `ShellExecuteW("runas")`
//!    with `SW_HIDE`, re-probe within T-11 (5s).
//! 3. [`OhmSupervisor::is_child_alive`] — monitor helper for the poller.
//! 4. [`OhmSupervisor::shutdown`] — kill child **only if sidebar launched it**
//!    (G10 ownership semantics).
//!
//! ## Job Object wrapping (G10)
//!
//! Sidebar-launched LHM is placed in a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. If the sidebar host crashes, the
//! kernel closes the job handle and reaps the elevated child — no orphans.
//! This is essential because `ShellExecuteW("runas")` launches LHM elevated
//! (UAC), and an unprivileged parent cannot `Stop-Process` an elevated child
//! (Access Denied).
//!
//! ## #1 gotcha — HTTP server OFF by default
//!
//! LHM v0.9.6 ships with `runWebServerMenuItem=false`. Without setting BOTH
//! `runWebServerMenuItem=true` AND `listenerPort=<port>` in
//! `LibreHardwareMonitor.exe.config` before launch, LHM starts cleanly but
//! listens on zero ports — the probe WILL see connection-refused and conclude
//! "Full unavailable" incorrectly. The config patch in
//! [`patch_lhm_config`] enforces both keys.
//!
//! ## LHM config format
//!
//! `resources/LibreHardwareMonitor.exe.config` is a standard .NET
//! `<configuration>` XML file. The bundled copy has `<startup>` + `<runtime>`
//! sections but NO `<appSettings>` — we must inject one. The patcher uses
//! targeted string insertion (dep-free, no XML parser dependency, avoiding
//! the quick-xml RUSTSEC issue). See [`patch_lhm_config`].
//!
//! ## Tier-change broadcast (T-38)
//!
//! The Event channel (`Event::TierChanged(Tier)`) is specified in §6 but not
//! yet implemented (Story 7.4). This module defines a minimal
//! [`TierChangeBroadcaster`] wrapper over `tokio::broadcast` so the monitor
//! task can emit transitions; Story 7.4 will wire the receiver. The
//! [`OhmSupervisor`] stores an `Option<Sender<ProviderTier>>` — `None` until
//! a channel is attached.
//!
//! ## Threading model
//!
//! `OhmSupervisor` is **sync** (no tokio dependency). The monitor task is
//! spawned by the app layer (Story 7.x) using `tokio::task::spawn_blocking`
//! to wait on the child handle. This keeps `sidebar-platform` runtime-free
//! and matches the pdh/dwm/window module discipline.
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! Every `unsafe` block (ShellExecuteW, CreateJobObjectW,
//! AssignProcessToJobObject, OpenProcess, WaitForSingleObject,
//! TerminateProcess, CloseHandle) carries a `// SAFETY:` comment citing the
//! invariant. HITL review (G11) is mandatory before merge.
//!
//! ## Cited
//!
//! - Story 6.4 TDD contract (Happy Path #1-#2, Boundary #1-#12)
//! - architecture.md AD-8 + §6 (flows D/E)
//! - nfr-thresholds.md T-10 (500ms HTTP timeout), T-11 (5s launch),
//!   T-38 (tier broadcast 500ms coalesce), T-45 (port 17127-17137)
//! - guardrails.md G10 (Job Object orphan prevention), G11 (HITL)

use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use sidebar_adapter_ohm::http::{HttpClient, OhmError, DEFAULT_OHM_PORT};
use sidebar_domain::error::Result;
use sidebar_sensor::descriptor::ProviderTier;

#[allow(unused_imports)]
use tracing::{debug, info, warn};

/// T-11: 5s launch timeout — from `ShellExecuteW("runas")` return to first
/// successful HTTP probe on the chosen port. Cited: nfr-thresholds.md T-11.
pub const LAUNCH_TIMEOUT_MS: u64 = 5_000;

/// T-45: the first candidate port in the LHM HTTP fallback chain.
/// `OhmSupervisor` probes 17127 first; on collision it walks 17128..17137.
pub const PORT_RANGE_START: u16 = 17_127;

/// T-45: the last candidate port (inclusive). 10 candidates total
/// (17127-17137). If all occupied → Full mode unavailable.
pub const PORT_RANGE_END: u16 = 17_137;

/// T-11: re-probe interval during the launch wait. We poll the HTTP endpoint
/// every 200ms (≈25 attempts within the 5s budget) rather than blocking on a
/// single 500ms-timeout probe.
#[allow(dead_code)] // GREEN: consumed by launch_elevated's wait loop.
const LAUNCH_REPROBE_INTERVAL_MS: u64 = 200;

/// `ShellExecuteW` returns an HINSTANCE; Win32 docs define a return value
/// `<= 32` as an error code. Cited: Story 6.4 Technical Context.
const SHELLEXECUTE_ERROR_THRESHOLD: i32 = 32;

/// LHM config key: enables the HTTP web server (OFF by default in v0.9.6).
#[allow(dead_code)] // GREEN: consumed by patch_lhm_config.
const CONFIG_KEY_WEB_SERVER: &str = "runWebServerMenuItem";

/// LHM config key: the TCP port the HTTP server binds.
#[allow(dead_code)] // GREEN: consumed by patch_lhm_config.
const CONFIG_KEY_LISTENER_PORT: &str = "listenerPort";

/// Minimal tier-change broadcaster wrapping a tokio-style sink. We use a
/// boxed callback rather than a `tokio::broadcast` channel to keep
/// `sidebar-platform` runtime-free. Story 7.4 will supply a real broadcast
/// sender; for now this is the seam.
///
/// The callback receives the new tier; the monitor task fires it on child
/// exit (Full → Basic). Coalescing (T-38 500ms) is the app layer's
/// responsibility — it owns the broadcast receiver.
pub type TierChangeCallback = Box<dyn Fn(ProviderTier) + Send + Sync>;

/// Owns the LHM subprocess lifecycle.
///
/// Generic over `C: HttpClient` so unit tests inject a `MockHttpClient`
/// (Story 6.4 Happy Path #1-#2). Production wires [`RealHttpClient`] via the
/// [`OhmSupervisor::new`] constructor.
///
/// State held:
/// - `client` — the HTTP probe client (reused from Story 3.6 adapter).
/// - `lhm_exe` — absolute path to `LibreHardwareMonitor.exe`.
/// - `lhm_config` — absolute path to `LibreHardwareMonitor.exe.config`.
/// - `child_handle` — `HANDLE` to the launched child (or `None`).
/// - `job_handle` — Job Object HANDLE wrapping the child (G10), or `None`.
/// - `sidebar_launched` — `true` iff sidebar invoked `ShellExecuteW` (G10
///   ownership: user-started LHM is left running on shutdown).
/// - `resolved_port` — the port the supervisor launched LHM on (for
///   re-probe + adapter wiring).
/// - `tier_tx` — optional tier-change broadcaster (T-38).
#[allow(dead_code)] // GREEN: fields consumed by launch_elevated/shutdown/is_child_alive.
pub struct OhmSupervisor<C: HttpClient> {
    client: C,
    lhm_exe: PathBuf,
    lhm_config: PathBuf,
    child_handle: Option<usize>,
    job_handle: Option<usize>,
    sidebar_launched: bool,
    resolved_port: Option<u16>,
    tier_tx: Option<TierChangeCallback>,
}

impl<C: HttpClient> OhmSupervisor<C> {
    /// Construct with an HTTP client + path to the LHM install directory
    /// (`LibreHardwareMonitor.exe` + `.config` are resolved from it).
    ///
    /// Tests use this to inject a `MockHttpClient` + a TempDir path.
    #[must_use]
    pub fn new(client: C, lhm_dir: impl AsRef<Path>) -> Self {
        let dir = lhm_dir.as_ref().to_path_buf();
        let lhm_exe = dir.join("LibreHardwareMonitor.exe");
        let lhm_config = dir.join("LibreHardwareMonitor.exe.config");
        Self {
            client,
            lhm_exe,
            lhm_config,
            child_handle: None,
            job_handle: None,
            sidebar_launched: false,
            resolved_port: None,
            tier_tx: None,
        }
    }

    /// Attach a tier-change broadcaster (T-38). The monitor task will invoke
    /// it when the child exits (Full → Basic transition). Pass `None` to
    /// detach.
    pub fn set_tier_change_broadcaster(&mut self, tx: Option<TierChangeCallback>) {
        self.tier_tx = tx;
    }

    /// The port the supervisor resolved during the last `launch_elevated`,
    /// or `None` if LHM was never launched by this supervisor.
    #[must_use]
    pub fn resolved_port(&self) -> Option<u16> {
        self.resolved_port
    }

    /// `true` iff sidebar launched the child (G10 ownership check).
    #[must_use]
    pub fn sidebar_launched(&self) -> bool {
        self.sidebar_launched
    }

    /// Probe `GET http://127.0.0.1:<port>/data.json` (T-10 500ms timeout via
    /// the HttpClient impl). Returns [`ProviderTier::Full`] if the body looks
    /// like the LHM JSON signature (top-level array, first element has
    /// `Text`/`text` + `Children`/`children`); [`ProviderTier::Basic`] on
    /// connection-refused, timeout, or non-LHM body (Boundary #10).
    ///
    /// Cited: Story 6.4 Happy Path #1-#2, Boundary #10. AD-7. T-10.
    pub fn probe(&self, port: u16) -> ProviderTier {
        // RED stub: always returns Basic (deliberately wrong).
        let _ = (self, port);
        ProviderTier::Basic
    }

    /// Launch LHM elevated. Three steps (AD-8 step 2):
    /// 1. Pick a free port per T-45 (probe 17127..17137).
    /// 2. Patch the LHM config file (`runWebServerMenuItem` + `listenerPort`).
    /// 3. `ShellExecuteW("runas", SW_HIDE)` + wait T-11 (5s) for HTTP probe.
    ///
    /// Returns the resolved port on success.
    ///
    /// # Errors
    /// - [`Error::Platform`] if the LHM binary is missing.
    /// - [`Error::Platform`] if all ports in the T-45 chain are occupied.
    /// - [`Error::Platform`] if `ShellExecuteW` returns an error (≤32).
    /// - [`Error::Platform`] if the launch timeout T-11 elapses without a
    ///   successful HTTP probe.
    ///
    /// Cited: Story 6.4 Boundary #1, #5, #6, #8, #9, #11. T-11, T-45, G10.
    pub fn launch_elevated(&mut self) -> Result<u16> {
        // RED stub: returns Ok(default) without doing any work.
        self.resolved_port = Some(DEFAULT_OHM_PORT);
        Ok(DEFAULT_OHM_PORT)
    }

    /// `true` iff the child handle is still open and the process is running.
    /// Used by the monitor task to detect LHM crash (Boundary #3, #7).
    ///
    /// Cited: Story 6.4 Boundary #3. G10.
    #[must_use]
    pub fn is_child_alive(&self) -> bool {
        // RED stub: always false.
        false
    }

    /// Terminate the child **only if sidebar launched it** (G10). User-started
    /// LHM is left running. Closes the Job Object handle (which also kills
    /// the child if `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is set).
    ///
    /// # Errors
    /// Returns [`Error::Platform`] on `TerminateProcess` failure (logged but
    /// propagated so the caller can decide).
    ///
    /// Cited: Story 6.4 Boundary #4. G10. T-39 (shutdown hierarchy).
    pub fn shutdown(&mut self) -> Result<()> {
        // RED stub: no-op.
        Ok(())
    }
}

/// Check whether an HTTP body looks like the LHM `/data.json` signature.
///
/// The LHM signature is: top-level JSON array, first element is an object
/// containing `Text`/`text` + `Children`/`children` (case-insensitive — LHM
/// v0.9.x emits PascalCase, but we tolerate camelCase for forward-compat).
///
/// Returns `true` if the body matches. Used by [`OhmSupervisor::probe`] for
/// the Full vs Basic classification (Boundary #10 — non-LHM discrimination).
///
/// Cited: Story 6.4 Boundary #10. AD-7.
#[must_use]
pub fn is_lhm_signature(body: &str) -> bool {
    // RED stub: always false.
    let _ = body;
    false
}

/// Walk the T-45 port fallback chain (17127..17137) and return the first port
/// that is NOT occupied by an LHM-signature service. A port is "occupied" if
/// the HTTP probe returns a body that does NOT match the LHM signature (a
/// foreign service) — in that case we skip it. A port that returns
/// connection-refused is "free".
///
/// Returns `Ok(port)` on the first free port, or `Err` if all 10 candidates
/// are occupied by non-LHM services (Boundary #9 — port fallback; the spec's
/// "out of fallback chain" → Basic).
///
/// Cited: Story 6.4 Boundary #9. T-45.
pub fn pick_free_port<C: HttpClient>(client: &C) -> Result<u16> {
    // RED stub: returns the default port.
    let _ = client;
    Ok(DEFAULT_OHM_PORT)
}

/// Patch the LHM config file (`.config` XML) to set BOTH keys before launch:
/// - `runWebServerMenuItem=true` (the #1 gotcha — HTTP server OFF by default).
/// - `listenerPort=<port>` (the chosen T-45 port).
///
/// ## Approach — dep-free string insertion (F1-tested)
///
/// The bundled `.config` has `<startup>` + `<runtime>` but no `<appSettings>`.
/// We inject an `<appSettings>` block immediately after `<configuration>` if
/// absent; if present, we update the two keys in-place via targeted string
/// replacement. This avoids adding an XML parser dep (quick-xml has the
/// RUSTSEC issue noted in the workspace; roxmltree is read-only).
///
/// The file is small (~600 bytes) and we control its content (it ships in
/// `resources/`), so string-replacement is robust. The test suite (F1 TempDir)
/// verifies the four cases: (a) file absent → created, (b) file present
/// without appSettings → injected, (c) file present with appSettings lacking
/// the keys → keys added, (d) file present with the keys → values updated.
///
/// Cited: Story 6.4 Boundary #11 (the #1 gotcha). T-45.
///
/// # Errors
/// Returns [`Error::Platform`] on I/O failure.
pub fn patch_lhm_config(config_path: &Path, port: u16) -> Result<()> {
    // RED stub: no-op.
    let _ = (config_path, port);
    Ok(())
}

/// Decode a `ShellExecuteW` HINSTANCE return value. Win32 docs: a value `<=32`
/// is an error code; `>32` is a success HINSTANCE. This helper centralizes the
/// i32-cast + threshold check so call sites stay clean.
///
/// Cited: Story 6.4 Technical Context (HINSTANCE error decoding).
#[must_use]
pub fn is_shellexecute_error(hinstance_as_i32: i32) -> bool {
    hinstance_as_i32 <= SHELLEXECUTE_ERROR_THRESHOLD
}

/// Broadcast a tier change via the optional callback (T-38). No-op if no
/// broadcaster is attached (Story 7.4 wires the real channel).
#[allow(dead_code)] // GREEN: called from shutdown (Full→Basic) + monitor task.
fn broadcast_tier_change(tx: Option<&TierChangeCallback>, tier: ProviderTier) {
    if let Some(cb) = tx {
        cb(tier);
    }
}

#[cfg(test)]
mod tests {
    //! Story 6.4 TDD contract tests.
    //!
    //! These tests are split into:
    //! 1. Pure-logic tests (signature check, port fallback, config patch,
    //!    ShellExecute decoding) — fully hermetic, no FFI.
    //! 2. Supervisor-level tests via `MockHttpClient` — probe/launch logic.
    //! 3. `#[ignore]` integration tests — real ShellExecuteW + Job Object
    //!    (need real UAC + real LHM binary; sdd-verify manual smoke).
    //!
    //! Cited:
    //!   - Story 6.4 TDD contract (Happy Path #1-#2, Boundary #1-#12)
    //!   - architecture.md AD-8 + §6
    //!   - nfr-thresholds.md T-10, T-11, T-38, T-45
    //!   - guardrails.md G10 (Job Object), G11 (HITL)

    use super::*;
    use mockall::mock;
    use std::fs;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    // ----- fixture helpers -----

    /// A minimal LHM-shaped `/data.json` body (top-level array, first element
    /// has `Text` + `Children`).
    const LHM_SIGNATURE_BODY: &str = r#"[
      { "id": "/", "text": "root", "type": "Node",
        "children": [
          { "id": "/amdcpu/0", "text": "AMD Ryzen", "type": "Node", "children": [] }
        ]
      }
    ]"#;

    /// A non-LHM body (HTML 404 from a foreign service on the port).
    const NON_LHM_BODY: &str = "<html><body>404 Not Found</body></html>";

    // Auto-mock HttpClient for supervisor tests (mirrors the ohm adapter).
    // Use `std::result::Result` explicitly — the module-level `Result` import
    // (from sidebar_domain::error) is a 1-generic alias and collides.
    mock! {
        pub FakeClient {}
        impl HttpClient for FakeClient {
            fn get(&self, url: &str) -> std::result::Result<String, OhmError>;
        }
    }

    /// Build a supervisor pointing at a TempDir (LHM exe path need not exist
    /// for probe/config tests; only `launch_elevated` checks existence).
    fn supervisor_in_tempdir(client: MockFakeClient) -> (OhmSupervisor<MockFakeClient>, TempDir) {
        let dir = TempDir::new().expect("TempDir");
        let sv = OhmSupervisor::new(client, PathBuf::from(dir.path()));
        (sv, dir)
    }

    // ==========================================================
    // Happy Path #1 — probe returns LHM JSON → Tier::Full
    // ==========================================================

    /// Story 6.4 Happy Path #1. Mock HTTP probe returns LHM-shaped JSON →
    /// `probe()` returns `Tier::Full`. Cited: Story 6.4 TDD contract.
    #[test]
    fn probe_returns_full_on_lhm_signature() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(LHM_SIGNATURE_BODY.to_string()));
        let (sv, _dir) = supervisor_in_tempdir(mock);
        assert_eq!(sv.probe(DEFAULT_OHM_PORT), ProviderTier::Full);
    }

    /// Story 6.4 Happy Path #2. Mock HTTP probe returns connection-refused →
    /// `Tier::Basic`, no UAC. Cited: Story 6.4 TDD contract.
    #[test]
    fn probe_returns_basic_on_connection_refused() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor_in_tempdir(mock);
        assert_eq!(sv.probe(DEFAULT_OHM_PORT), ProviderTier::Basic);
    }

    // ==========================================================
    // Boundary #10 — non-LHM discrimination
    // ==========================================================

    /// Story 6.4 Boundary #10. Something returns HTTP 200 on 17127 but the
    /// body isn't LHM JSON → `Tier::Basic` (treated as occupied). Cited:
    /// Story 6.4 Boundary #10.
    #[test]
    fn probe_returns_basic_on_non_lhm_body() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(NON_LHM_BODY.to_string()));
        let (sv, _dir) = supervisor_in_tempdir(mock);
        assert_eq!(sv.probe(DEFAULT_OHM_PORT), ProviderTier::Basic);
    }

    /// T-10 timeout → Basic (probe must not hang).
    #[test]
    fn probe_returns_basic_on_timeout() {
        let mut mock = MockFakeClient::new();
        mock.expect_get().returning(|_| Err(OhmError::Timeout));
        let (sv, _dir) = supervisor_in_tempdir(mock);
        assert_eq!(sv.probe(DEFAULT_OHM_PORT), ProviderTier::Basic);
    }

    // ==========================================================
    // is_lhm_signature — pure function (Boundary #10)
    // ==========================================================

    /// LHM signature recognized: PascalCase keys + children array.
    #[test]
    fn signature_recognizes_pascalcase() {
        assert!(is_lhm_signature(LHM_SIGNATURE_BODY));
    }

    /// LHM signature recognized: camelCase keys (forward-compat).
    #[test]
    fn signature_recognizes_camelcase() {
        let body = r#"[{ "id": "/", "text": "root", "children": [] }]"#;
        assert!(is_lhm_signature(body));
    }

    /// Non-LHM HTML body rejected.
    #[test]
    fn signature_rejects_html() {
        assert!(!is_lhm_signature(NON_LHM_BODY));
    }

    /// Empty body rejected.
    #[test]
    fn signature_rejects_empty() {
        assert!(!is_lhm_signature(""));
    }

    /// JSON object (not array) rejected — LHM is always a top-level array.
    #[test]
    fn signature_rejects_top_level_object() {
        let body = r#"{ "text": "root", "children": [] }"#;
        assert!(!is_lhm_signature(body));
    }

    /// Array with empty first element rejected (no Text/Children).
    #[test]
    fn signature_rejects_empty_first_element() {
        let body = r"[{}]";
        assert!(!is_lhm_signature(body));
    }

    // ==========================================================
    // Boundary #11 — config patch (the #1 gotcha)
    // ==========================================================

    /// Story 6.4 Boundary #11 (the #1 gotcha). When the config file does NOT
    /// exist, `patch_lhm_config` creates it with BOTH keys
    /// (`runWebServerMenuItem=true` AND `listenerPort=<port>`). Cited:
    /// Story 6.4 Boundary #11.
    #[test]
    fn config_patch_creates_file_with_both_keys_when_absent() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.exe.config");
        patch_lhm_config(&path, 17_129).expect("patch");
        let content = fs::read_to_string(&path).expect("read");
        assert!(
            content.contains(r#"key="runWebServerMenuItem" value="true""#),
            "must set runWebServerMenuItem=true (#1 gotcha), got:\n{content}"
        );
        assert!(
            content.contains(r#"key="listenerPort" value="17129""#),
            "must set listenerPort=17129, got:\n{content}"
        );
    }

    /// Story 6.4 Boundary #11 extension. When the config file EXISTS but has
    /// no `<appSettings>` (the actual bundled state), the patcher injects an
    /// `<appSettings>` block with both keys WITHOUT destroying existing
    /// `<startup>`/`<runtime>` sections.
    #[test]
    fn config_patch_injects_appsettings_into_existing_file() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.exe.config");
        // Mirror the actual bundled config (startup + runtime, no appSettings).
        fs::write(
            &path,
            r#"<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <startup>
    <supportedRuntime version="v4.0" sku=".NETFramework,Version=v4.7.2" />
  </startup>
  <runtime></runtime>
</configuration>"#,
        )
        .expect("write");

        patch_lhm_config(&path, 17_127).expect("patch");

        let content = fs::read_to_string(&path).expect("read");
        // Existing sections preserved.
        assert!(
            content.contains("<startup>"),
            "startup must survive: {content}"
        );
        assert!(
            content.contains("<runtime>"),
            "runtime must survive: {content}"
        );
        // Both keys injected.
        assert!(
            content.contains(r#"key="runWebServerMenuItem" value="true""#),
            "must set runWebServerMenuItem=true: {content}"
        );
        assert!(
            content.contains(r#"key="listenerPort" value="17127""#),
            "must set listenerPort=17127: {content}"
        );
    }

    /// When the config already has `<appSettings>` with the keys at wrong
    /// values, the patcher updates them in-place (idempotent-ish).
    #[test]
    fn config_patch_updates_existing_keys_in_place() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.exe.config");
        fs::write(
            &path,
            r#"<?xml version="1.0"?>
<configuration>
  <appSettings>
    <add key="runWebServerMenuItem" value="false" />
    <add key="listenerPort" value="8085" />
  </appSettings>
</configuration>"#,
        )
        .expect("write");

        patch_lhm_config(&path, 17_130).expect("patch");

        let content = fs::read_to_string(&path).expect("read");
        assert!(
            content.contains(r#"key="runWebServerMenuItem" value="true""#),
            "must flip to true: {content}"
        );
        assert!(
            content.contains(r#"key="listenerPort" value="17130""#),
            "must update to 17130: {content}"
        );
        // Old values must be gone.
        assert!(
            !content.contains(r#"value="false""#),
            "stale false must be replaced: {content}"
        );
        assert!(
            !content.contains(r#"value="8085""#),
            "stale 8085 must be replaced: {content}"
        );
    }

    /// Idempotency: patching twice produces the same result.
    #[test]
    fn config_patch_is_idempotent() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.exe.config");
        patch_lhm_config(&path, 17_127).expect("patch 1");
        let after_first = fs::read_to_string(&path).expect("read 1");
        patch_lhm_config(&path, 17_127).expect("patch 2");
        let after_second = fs::read_to_string(&path).expect("read 2");
        assert_eq!(after_first, after_second, "patch must be idempotent");
    }

    // ==========================================================
    // Boundary #9 — port fallback (T-45)
    // ==========================================================

    /// Story 6.4 Boundary #9. Port 17127 occupied by a non-LHM service (mock
    /// returns non-LHM body) → `pick_free_port` returns 17128. Cited:
    /// Story 6.4 Boundary #9, T-45.
    #[test]
    fn pick_free_port_falls_back_when_17127_occupied() {
        let mut mock = MockFakeClient::new();
        // 17127 returns non-LHM (occupied); 17128 returns connection-refused (free).
        mock.expect_get().returning(|url| {
            if url.contains(":17127") {
                Ok(NON_LHM_BODY.to_string())
            } else {
                Err(OhmError::HttpFailed("connection refused".to_string()))
            }
        });
        let port = pick_free_port(&mock).expect("free port");
        assert_eq!(port, 17_128, "must fall back to 17128 when 17127 occupied");
    }

    /// T-45: if 17127 is free (connection-refused), pick it (don't skip).
    #[test]
    fn pick_free_port_prefers_17127_when_free() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let port = pick_free_port(&mock).expect("free port");
        assert_eq!(port, 17_127);
    }

    /// T-45: if all 10 candidates (17127-17137) are occupied by non-LHM
    /// services → `Err` (Full unavailable). Cited: "out of fallback chain".
    #[test]
    fn pick_free_port_errors_when_all_occupied() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Ok(NON_LHM_BODY.to_string()));
        let result = pick_free_port(&mock);
        assert!(result.is_err(), "must error when all ports occupied");
    }

    /// T-45: a port already running LHM (LHM signature body) is NOT "free" —
    /// it means LHM is already running. `pick_free_port` returns that port
    /// (caller can use it directly without relaunching). This is the
    /// "already-running LHM" path (AD-8 step 1).
    #[test]
    fn pick_free_port_returns_lhm_port_if_already_running() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(LHM_SIGNATURE_BODY.to_string()));
        let port = pick_free_port(&mock).expect("port");
        assert_eq!(port, 17_127, "first LHM-detected port is the pick");
    }

    // ==========================================================
    // Boundary #2/#5/#6/#8 — launch_elevated error paths (mock FFI)
    // ==========================================================

    /// Story 6.4 Boundary #5. LHM binary missing → `launch_elevated` returns
    /// `Err` with a clear message. Cited: Story 6.4 Boundary #5.
    #[test]
    fn launch_elevated_errors_when_lhm_binary_missing() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (mut sv, _dir) = supervisor_in_tempdir(mock);
        // The TempDir has no LibreHardwareMonitor.exe → must error.
        let result = sv.launch_elevated();
        assert!(result.is_err(), "missing binary must error");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.to_lowercase().contains("librehardwaremonitor")
                || msg.to_lowercase().contains("lhm")
                || msg.to_lowercase().contains("exe")
                || msg.to_lowercase().contains("not found")
                || msg.to_lowercase().contains("missing"),
            "error must mention LHM/exe/missing: {msg}"
        );
    }

    /// `is_shellexecute_error`: HINSTANCE values ≤32 are errors.
    #[test]
    fn shellexecute_error_decoding_threshold() {
        assert!(is_shellexecute_error(0)); // OOM-ish
        assert!(is_shellexecute_error(5)); // SE_ERR_ACCESSDENIED
        assert!(is_shellexecute_error(32)); // boundary
        assert!(!is_shellexecute_error(33)); // success
        assert!(!is_shellexecute_error(42)); // success
    }

    // ==========================================================
    // Boundary #4 — shutdown ownership (G10)
    // ==========================================================

    /// Story 6.4 Boundary #4. When sidebar did NOT launch LHM,
    /// `shutdown()` is a no-op (user-started LHM left running). Cited:
    /// Story 6.4 Boundary #4, G10.
    #[test]
    fn shutdown_is_noop_when_sidebar_did_not_launch() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (mut sv, _dir) = supervisor_in_tempdir(mock);
        assert!(!sv.sidebar_launched(), "fresh supervisor did not launch");
        // shutdown must be Ok and a no-op.
        sv.shutdown().expect("shutdown no-op");
        assert!(!sv.sidebar_launched(), "still did not launch");
    }

    // ==========================================================
    // Tier-change broadcast (T-38)
    // ==========================================================

    /// T-38: the tier-change callback fires on `broadcast_tier_change`. The
    /// real monitor task (Story 7.4) wires this to a tokio broadcast channel.
    #[test]
    fn tier_change_callback_fires() {
        let received: Arc<Mutex<Vec<ProviderTier>>> = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let cb: TierChangeCallback = Box::new(move |tier| {
            received_clone.lock().expect("lock").push(tier);
        });
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (mut sv, _dir) = supervisor_in_tempdir(mock);
        sv.set_tier_change_broadcaster(Some(cb));
        // Simulate the monitor task firing a transition.
        broadcast_tier_change(sv.tier_tx.as_ref(), ProviderTier::Basic);
        let got = received.lock().expect("lock").clone();
        assert_eq!(got, vec![ProviderTier::Basic]);
    }

    // ==========================================================
    // is_child_alive — no child → false
    // ==========================================================

    /// A fresh supervisor with no child launched → `is_child_alive` is false.
    #[test]
    fn is_child_alive_false_on_fresh_supervisor() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (sv, _dir) = supervisor_in_tempdir(mock);
        assert!(!sv.is_child_alive());
    }

    // ==========================================================
    // #[ignore] integration tests — real FFI + real UAC + real LHM
    // ==========================================================

    /// Real ShellExecuteW + Job Object smoke against bundled LHM. Needs UAC
    /// consent (will prompt) + real LHM binary. Cited: Story 6.4 sdd-verify.
    #[test]
    #[ignore = "needs real UAC + real LHM binary (sdd-verify manual smoke, Story 6.4)"]
    fn launch_elevated_real_lhm_integration() {
        use sidebar_adapter_ohm::http::RealHttpClient;
        let exe_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("parent")
            .parent()
            .expect("grandparent")
            .join("resources");
        let exe = exe_dir.join("LibreHardwareMonitor.exe");
        if !exe.exists() {
            eprintln!("skipped: {exe:?} not present");
            return;
        }
        let mut sv = OhmSupervisor::new(RealHttpClient::new(), exe_dir);
        let port = sv.launch_elevated().expect("launch");
        assert!((PORT_RANGE_START..=PORT_RANGE_END).contains(&port));
        // Cleanup.
        let _ = sv.shutdown();
    }

    /// G10 host-crash simulation: drop the supervisor without shutdown, then
    /// verify the Job Object handle closure reaps the child. Manual smoke —
    /// the kernel reaps when the last handle closes (on process exit this is
    /// automatic). Cited: Story 6.4 Boundary #7, G10.
    #[test]
    #[ignore = "manual G10 verification (sdd-verify, Story 6.4 Boundary #7)"]
    fn job_object_reaps_on_drop() {
        // Placeholder — real verification is via the integration smoke +
        // post-test tasklist inspection confirming no orphan LHM.
    }

    // ----- write helper used nowhere but keeps clippy's dead_code quiet
    // about the `Write` import if a future refactor drops fs::write. -----
    #[allow(dead_code)]
    fn _write_helper(p: &Path, b: &str) -> std::io::Result<()> {
        let mut f = fs::File::create(p)?;
        f.write_all(b.as_bytes())?;
        Ok(())
    }
}
