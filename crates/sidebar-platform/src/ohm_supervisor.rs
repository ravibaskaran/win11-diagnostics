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
use std::time::{Duration, Instant};

// `HttpClient` + `OhmError` are used by `OhmSupervisor<C: HttpClient>` in
// production; `HTTP_TIMEOUT_MS` is consumed by the M9 wait_for_probe fix
// below. `DEFAULT_OHM_PORT` is only referenced in tests.
#[cfg(test)]
use sidebar_adapter_ohm::http::DEFAULT_OHM_PORT;
use sidebar_adapter_ohm::http::{HttpClient, OhmError, HTTP_TIMEOUT_MS};
use sidebar_domain::error::{Error, Result};
use sidebar_sensor::descriptor::ProviderTier;

use tracing::{debug, info, warn};

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{TerminateProcess, WaitForSingleObject, INFINITE};
#[cfg(windows)]
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// T-11: 5s launch timeout — from `ShellExecuteW("runas")` return to first
/// successful HTTP probe on the chosen port. Cited: nfr-thresholds.md T-11.
pub const LAUNCH_TIMEOUT_MS: u64 = 5_000;

/// T-45: the first candidate port in the LHM HTTP fallback chain.
/// `OhmSupervisor` probes 17127 first; on collision it walks 17128..17137.
pub const PORT_RANGE_START: u16 = 17_127;

/// T-45: the last candidate port (inclusive). The range
/// `17127..=17137` is 11 candidates total (the 17127 default plus 10
/// fallbacks). If all occupied → Full mode unavailable.
pub const PORT_RANGE_END: u16 = 17_137;

// M10 — compile-time assertion that the port fallback chain is exactly 11
// candidates (the 17127 default + 10 fallbacks per T-45). Catches an
// accidental `..` vs `..=` off-by-one regression.
const _: () = assert!(PORT_RANGE_END - PORT_RANGE_START + 1 == 11);

/// T-11: re-probe interval during the launch wait. We poll the HTTP endpoint
/// every 200ms. Note: with the M9 pre-probe deadline check, the worst-case
/// attempt count within the 5s budget is `5000 / (HTTP_TIMEOUT_MS + interval)`
/// = `5000 / (500 + 200)` ≈ 7 attempts (NOT 25 — the previous "≈25 attempts"
/// comment assumed zero probe latency). Cited: M9.
const LAUNCH_REPROBE_INTERVAL_MS: u64 = 200;

/// `ShellExecuteW` returns an HINSTANCE; Win32 docs define a return value
/// `<= 32` as an error code. Cited: Story 6.4 Technical Context.
const SHELLEXECUTE_ERROR_THRESHOLD: i32 = 32;

/// LHM config key: enables the HTTP web server (OFF by default in v0.9.6).
const CONFIG_KEY_WEB_SERVER: &str = "runWebServerMenuItem";

/// LHM config key: the TCP port the HTTP server binds.
const CONFIG_KEY_LISTENER_PORT: &str = "listenerPort";

#[cfg(windows)]
struct OwnedHandleGuard {
    handle: Option<HANDLE>,
}

#[cfg(windows)]
impl OwnedHandleGuard {
    fn new(handle: HANDLE) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    fn as_handle(&self) -> HANDLE {
        self.handle.expect("owned handle guard is populated")
    }

    fn into_handle(mut self) -> HANDLE {
        self.handle.take().expect("owned handle guard is populated")
    }

    fn terminate_and_reap(&self) {
        let handle = self.as_handle();
        // SAFETY: handle came from ShellExecuteExW and remains owned by this
        // guard. TerminateProcess + INFINITE wait ensures no orphan survives
        // a failed post-launch ownership setup.
        let _ = unsafe { TerminateProcess(handle, 1) };
        // SAFETY: the same owned process handle is waited synchronously until
        // its termination is observable before Drop closes it.
        let _ = unsafe { WaitForSingleObject(handle, INFINITE) };
    }
}

#[cfg(windows)]
impl Drop for OwnedHandleGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: the guard owns this handle and closes it at most once.
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

trait ChildTermination {
    fn terminate_and_reap(&mut self);
}

fn cleanup_failed_post_launch_setup<C: ChildTermination>(child: &mut C) {
    child.terminate_and_reap();
}

#[cfg(windows)]
impl ChildTermination for OwnedHandleGuard {
    fn terminate_and_reap(&mut self) {
        Self::terminate_and_reap(self);
    }
}

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
/// - `child_handle` — `HANDLE` (as `isize`) to the launched child, or `None`.
/// - `job_handle` — Job Object HANDLE (as `isize`) wrapping the child (G10).
/// - `sidebar_launched` — `true` iff sidebar invoked `ShellExecuteW` (G10
///   ownership: user-started LHM is left running on shutdown).
/// - `resolved_port` — the port the supervisor launched LHM on (for
///   re-probe + adapter wiring).
/// - `tier_tx` — optional tier-change broadcaster (T-38).
///
/// `HANDLE`s are stored as `isize` (the inner value of `HANDLE(isize)`) so
/// the struct is `cfg(unix)`-compatible for compile tests. The FFI is gated
/// `#[cfg(windows)]`; on non-Windows the launch/alive/shutdown methods return
/// `Err` (sidebar only ships on Win11, but the crate must `cargo check`
/// cross-platform for the workspace-shape test).
pub struct OhmSupervisor<C: HttpClient> {
    client: C,
    lhm_exe: PathBuf,
    lhm_config: PathBuf,
    /// Path to `LibreHardwareMonitor.config` (LHM's PersistentSettings file).
    /// LHM's `PersistentSettings` reads from THIS file (not the .exe.config),
    /// so `runWebServerMenuItem` and `listenerPort` MUST be patched here too.
    lhm_user_config: PathBuf,
    child_handle: Option<isize>,
    job_handle: Option<isize>,
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
        let lhm_user_config = dir.join("LibreHardwareMonitor.config");
        Self {
            client,
            lhm_exe,
            lhm_config,
            lhm_user_config,
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

    /// Borrow the underlying HTTP client. Exposed so the launch-time tier
    /// probe (Story 7.3) can classify WHY a port returned Basic (connection
    /// refused vs non-LHM body vs timeout) when composing the user-facing
    /// hint — `probe()` collapses all failures to `Basic` for the tier
    /// decision, but the hint needs the reason. The client is read-only via
    /// `HttpClient::get(&self, ...)`, so borrowing it does not compromise
    /// the supervisor's lifecycle ownership. Cited: Story 7.3.
    #[must_use]
    pub fn client(&self) -> &C {
        &self.client
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
        let url = format!("http://127.0.0.1:{port}/data.json");
        match self.client.get(&url) {
            Ok(body) => {
                if is_lhm_signature(&body) {
                    ProviderTier::Full
                } else {
                    // Boundary #10 — something answered but it's not LHM.
                    debug!(
                        port,
                        "probe: non-LHM body on port, treating as Basic (occupied)"
                    );
                    ProviderTier::Basic
                }
            }
            Err(OhmError::HttpFailed(reason)) => {
                debug!(port, %reason, "probe: connection failed → Basic");
                ProviderTier::Basic
            }
            Err(OhmError::Timeout) => {
                debug!(port, "probe: T-10 timeout → Basic");
                ProviderTier::Basic
            }
            Err(OhmError::NotJson(reason)) => {
                debug!(port, %reason, "probe: non-JSON body → Basic");
                ProviderTier::Basic
            }
            Err(OhmError::Parse(reason)) => {
                debug!(port, %reason, "probe: JSON parse failure → Basic");
                ProviderTier::Basic
            }
            Err(OhmError::RejectedUrl(reason)) => {
                warn!(port, %reason, "probe: URL rejected by G16 loopback policy");
                ProviderTier::Basic
            }
        }
    }

    /// Launch LHM elevated. Three steps (AD-8 step 2):
    /// 1. Pick a free port per T-45 (probe 17127..17137).
    /// 2. Patch the LHM config file (`runWebServerMenuItem` + `listenerPort`).
    /// 3. `ShellExecuteW("runas", SW_HIDE)` + wait T-11 (5s) for HTTP probe.
    ///
    /// Returns the resolved port on success.
    ///
    /// # Errors
    /// - [`Error::Platform`] if the LHM binary is missing (Boundary #5).
    /// - [`Error::Platform`] if all ports in the T-45 chain are occupied
    ///   by non-LHM services (Boundary #9 "out of fallback chain").
    /// - [`Error::Platform`] if `ShellExecuteW` returns an error (≤32)
    ///   (Boundary #2, #6 — UAC declined).
    /// - [`Error::Platform`] if the launch timeout T-11 elapses without a
    ///   successful HTTP probe (Boundary #8).
    ///
    /// Cited: Story 6.4 Boundary #1, #5, #6, #8, #9, #11. T-11, T-45, G10.
    pub fn launch_elevated(&mut self) -> Result<u16> {
        // H5 — re-entry guard. If sidebar already launched LHM in this
        // supervisor's lifetime, refuse to launch again — blindly
        // overwriting `child_handle`/`job_handle` would leak the first
        // pair (no CloseHandle) until process exit, leaving an orphan
        // elevated LHM during sidebar's lifetime. G10 contract.
        if self.sidebar_launched {
            return Err(Error::Platform(
                "launch_elevated called twice without shutdown — refusing to leak prior child/job handle (G10)".into(),
            ));
        }

        // T-45 — pick a free port (handles already-running LHM + fallback).
        let port = match pick_free_port(&self.client)? {
            PortPick::AlreadyRunning(already_port) => {
                // H4 / AD-8 step 1 — LHM is already answering. Reuse the
                // port WITHOUT relaunching, WITHOUT setting
                // `sidebar_launched` (G10 — sidebar does not own the
                // user's LHM and MUST NOT kill it on shutdown). Skip the
                // config patch + ShellExecuteW + Job Object entirely.
                self.resolved_port = Some(already_port);
                debug!(
                    port = already_port,
                    "launch_elevated: LHM already running — reusing port, no relaunch (AD-8 step 1)"
                );
                return Ok(already_port);
            }
            PortPick::Free(free_port) => {
                // Boundary #5 — only a new launch requires the bundled
                // executable. An already-running LHM was handled above and
                // must be reusable even when this installation does not ship
                // its own executable.
                if !self.lhm_exe.exists() {
                    return Err(Error::Platform(format!(
                        "LibreHardwareMonitor.exe not found at {} — cannot launch Full mode",
                        self.lhm_exe.display()
                    )));
                }
                self.resolved_port = Some(free_port);
                free_port
            }
        };

        // Boundary #11 — patch BOTH config files before launch (the #1 gotcha).
        // 1. LibreHardwareMonitor.exe.config — .NET runtime config (startup, assembly redirects).
        // 2. LibreHardwareMonitor.config — LHM PersistentSettings file (where the web server
        //    actually reads runWebServerMenuItem + listenerPort at startup).
        // Without patching the user config, LHM starts but its HTTP server stays OFF because
        // PersistentSettings defaults runWebServerMenuItem to false when the key is absent.
        patch_lhm_config(&self.lhm_config, port)?;
        patch_lhm_user_config(&self.lhm_user_config, port)?;

        // Launch + Job Object wrap. The ShellExecuteW + Job Object wiring is
        // windows-only; the non-windows path returns Err (sidebar ships on
        // Win11 only, but the crate must compile cross-platform for the
        // workspace-shape test).
        #[cfg(windows)]
        {
            let child_handle = self.shellexecute_runas(&port)?;
            let mut child_guard = OwnedHandleGuard::new(child_handle);
            // G10 — wrap in Job Object so the kernel reaps the elevated child
            // if the sidebar host crashes.
            let job_handle = match Self::create_and_assign_job(child_guard.as_handle()) {
                Ok(job_handle) => job_handle,
                Err(error) => {
                    cleanup_failed_post_launch_setup(&mut child_guard);
                    return Err(error);
                }
            };
            let child_handle = child_guard.into_handle();
            // Stash handles as isize so the struct is unix-compilable for the
            // workspace-shape test (HANDLE wraps *mut c_void on Windows).
            self.child_handle = Some(child_handle.0 as isize);
            self.job_handle = Some(job_handle.0 as isize);
            self.sidebar_launched = true;

            // T-11 — wait up to 5s for the HTTP probe to succeed.
            if !self.wait_for_probe(port)? {
                // Launch timed out — LHM started but didn't answer HTTP in
                // budget. Leave the child running (user may want to inspect
                // it); mark sidebar_launched so shutdown can clean it up.
                warn!(port, "LHM launch timed out (T-11={LAUNCH_TIMEOUT_MS}ms)");
                return Err(Error::Platform(format!(
                    "LHM launch timed out: no HTTP response on port {port} within {LAUNCH_TIMEOUT_MS}ms (T-11)"
                )));
            }
            info!(
                port,
                "LHM launched elevated, HTTP probe confirmed Full tier"
            );
        }

        Ok(port)
    }

    /// `true` iff the child handle is still open and the process is running.
    /// Used by the monitor task to detect LHM crash (Boundary #3, #7).
    ///
    /// Cited: Story 6.4 Boundary #3. G10.
    #[must_use]
    pub fn is_child_alive(&self) -> bool {
        let Some(raw) = self.child_handle else {
            return false;
        };
        #[cfg(windows)]
        {
            // Reconstruct the HANDLE from its stored isize form.
            let handle = HANDLE(raw as *mut core::ffi::c_void);
            // SAFETY: `raw` came from a valid process HANDLE we own (stash on
            // launch_elevated via ShellExecuteExW hProcess); the handle remains
            // valid until CloseHandle (called in shutdown / Drop).
            // WaitForSingleObject with timeout 0 is non-blocking and returns
            // WAIT_TIMEOUT (0x102) if the process is still running.
            let result = unsafe { WaitForSingleObject(handle, 0) };
            // 0x102 = WAIT_TIMEOUT (still running); 0 = WAIT_OBJECT_0 (signaled = exited).
            // WaitForSingleObject returns WAIT_EVENT (a u32 newtype); compare
            // against WAIT_EVENT(0x102) directly.
            result == windows::Win32::Foundation::WAIT_EVENT(0x0000_0102)
        }
        #[cfg(not(windows))]
        {
            let _ = raw;
            false
        }
    }

    /// Terminate the child **only if sidebar launched it** (G10). User-started
    /// LHM is left running. Closes the Job Object handle (which also kills
    /// the child if `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is set).
    ///
    /// # Errors
    /// Returns [`Error::Platform`] on handle-closure failure (logged but
    /// propagated so the caller can decide).
    ///
    /// Cited: Story 6.4 Boundary #4. G10. T-39 (shutdown hierarchy).
    pub fn shutdown(&mut self) -> Result<()> {
        self.shutdown_with_budget(Duration::from_millis(1_500))
    }

    /// Shut down a sidebar-owned child within the caller's remaining budget.
    /// The Windows waits share one absolute deadline, so the post-terminate
    /// re-wait cannot add another full timeout after the first wait expires.
    pub fn shutdown_with_budget(&mut self, budget: Duration) -> Result<()> {
        if !self.sidebar_launched {
            // G10 — user-started LHM is left running.
            debug!("shutdown: sidebar did not launch LHM — leaving child running (G10)");
            return Ok(());
        }
        // Close the Job Object handle. With JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        // set (see create_and_assign_job), closing it terminates the child.
        // M8 — wait on the child to synchronize per T-39's staged hierarchy
        // (the caller has a 1500ms OHM-teardown window). Without the wait,
        // closing the job handle returns immediately and the caller has no
        // observable confirmation that LHM is actually dead before the next
        // shutdown phase runs. The G10 orphan contract still holds (kernel
        // completes the reap post-exit), but staged shutdown per T-39 is now
        // observable.
        let deadline = Instant::now() + budget;
        #[cfg(windows)]
        {
            if let Some(job_raw) = self.job_handle.take() {
                // SAFETY: `job_raw` is a valid Job Object HANDLE we created;
                // closing it is safe and triggers kernel reap of all
                // processes in the job (G10).
                let job = HANDLE(job_raw as *mut core::ffi::c_void);
                // SAFETY: see above — CloseHandle on an owned job handle.
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
            }
            if let Some(child_raw) = self.child_handle.take() {
                let child = HANDLE(child_raw as *mut core::ffi::c_void);
                // M8 — wait only until the caller's absolute deadline for
                // the child to actually exit. On WAIT_TIMEOUT, fall back to
                // TerminateProcess + re-wait with the remaining budget. The
                // waits are bounded so a wedged child cannot stall shutdown
                // past T-39.
                //
                // SAFETY: `child` is a valid process HANDLE we own (from
                // ShellExecuteExW hProcess). WaitForSingleObject with a
                // finite timeout is non-blocking past the timeout and
                // returns WAIT_OBJECT_0 (signaled = exited) or
                // WAIT_TIMEOUT. TerminateProcess forces exit.
                let wait = unsafe {
                    windows::Win32::System::Threading::WaitForSingleObject(
                        child,
                        remaining_wait_millis(deadline),
                    )
                };
                if wait == windows::Win32::Foundation::WAIT_EVENT(0x0000_0102) {
                    // WAIT_TIMEOUT — child didn't exit; force-kill.
                    // SAFETY: `child` is a valid process HANDLE we own;
                    // TerminateProcess is the documented force-kill API and
                    // is safe to call on a handle with PROCESS_TERMINATE
                    // access (ShellExecuteExW hProcess grants it).
                    let _ = unsafe {
                        windows::Win32::System::Threading::TerminateProcess(
                            child, 1, // exit code
                        )
                    };
                    // Re-wait briefly so CloseHandle runs after exit, not
                    // mid-termination (cleaner kernel accounting).
                    // SAFETY: same `child` HANDLE; the re-wait is bounded by the
                    // same absolute shutdown deadline.
                    let _ = unsafe {
                        windows::Win32::System::Threading::WaitForSingleObject(
                            child,
                            remaining_wait_millis(deadline),
                        )
                    };
                }
                // SAFETY: child_raw is the HANDLE from ShellExecuteExW's
                // hProcess. Closing it after the job has killed the process
                // (or TerminateProcess forced exit) is the documented cleanup.
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(child) };
            }
        }
        self.sidebar_launched = false;
        // Broadcast Full → Basic (T-38). The monitor task in Story 7.4 owns
        // coalescing; here we just fire the transition.
        broadcast_tier_change(self.tier_tx.as_ref(), ProviderTier::Basic);
        info!("shutdown: terminated sidebar-launched LHM (G10)");
        Ok(())
    }

    // ----- windows-only FFI helpers (ShellExecuteExW + Job Object) -----

    /// Launch LHM elevated via `ShellExecuteExW(verb="runas")` with
    /// `SEE_MASK_NOCLOSEPROCESS` so we get the child HANDLE back (needed for
    /// the Job Object wrap). SW_HIDE keeps the LHM console window hidden — we
    /// talk to its HTTP server, never to its GUI.
    ///
    /// On UAC decline (user clicked "No"), `ShellExecuteExW` returns an error
    /// via the `windows::core::Error` (`ERROR_CANCELLED`). We surface that as
    /// `Error::Platform` so the caller falls back to Basic tier (Boundary #2,
    /// #6).
    #[cfg(windows)]
    fn shellexecute_runas(&self, port: &u16) -> Result<HANDLE> {
        use std::os::windows::ffi::OsStrExt;

        // Build NUL-terminated UTF-16 wide strings. `encode_wide` yields the
        // UTF-16 encoding; chaining `once(0)` adds the NUL terminator required
        // by PCWSTR.
        let exe_wide: Vec<u16> = std::ffi::OsStr::new(&self.lhm_exe)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // "runas" is the documented verb that triggers UAC. encode_wide lives
        // on OsStr, so wrap the str in OsStr::new. The trailing NUL terminator
        // is required by PCWSTR.
        let verb_wide: Vec<u16> = std::ffi::OsStr::new("runas")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let params = format!("--port {port}");
        let params_wide: Vec<u16> = std::ffi::OsStr::new(&params)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut info = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
                .expect("SHELLEXECUTEINFOW fits in u32"),
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb_wide.as_ptr()),
            lpFile: PCWSTR(exe_wide.as_ptr()),
            lpParameters: PCWSTR(params_wide.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };

        // SAFETY: SHELLEXECUTEINFOW is zero-initialized beyond cbSize/fMask
        // (Default impl via `Win32_System_Registry`); the wide-string pointers
        // are NUL-terminated (we chained a 0 for verb/exe/params); `fMask`
        // includes SEE_MASK_NOCLOSEPROCESS so hProcess is populated in the
        // out-param and ownership transfers to us (we CloseHandle it in
        // shutdown). The struct lives on this stack frame for the duration of
        // the call. Win32 docs guarantee the call returns before the struct is
        // touched again.
        let ok = unsafe { ShellExecuteExW(std::ptr::addr_of_mut!(info)) };
        ok.map_err(|e| {
            Error::Platform(format!(
                "ShellExecuteExW(runas) failed: {e} — UAC declined or LHM binary inaccessible"
            ))
        })?;
        let hprocess = info.hProcess;
        if hprocess.is_invalid() {
            // Defensive — shouldn't happen on Ok return, but the Win32 docs
            // allow hProcess=NULL when no process was launched.
            return Err(Error::Platform(
                "ShellExecuteExW returned Ok but hProcess is NULL".to_string(),
            ));
        }
        Ok(hprocess)
    }

    /// Create an anonymous Job Object with
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then assign `child` to it. With
    /// this flag, when the *last* handle to the job is closed (including
    /// kernel handle-table cleanup on host process exit), the kernel
    /// terminates every process in the job. This is the G10 contract: no
    /// orphan elevated LHM survives a sidebar crash.
    #[cfg(windows)]
    fn create_and_assign_job(child: HANDLE) -> Result<HANDLE> {
        // SAFETY: CreateJobObjectW with both args `None` creates an anonymous
        // job with default security; returns a HANDLE we own. `None` name →
        // no global-namespace collision possible. The handle must be closed
        // (done in shutdown / Drop).
        let job = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| Error::Platform(format!("CreateJobObjectW failed: {e}")))?;
        let job_guard = OwnedHandleGuard::new(job);

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let info_ptr: *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::ptr::addr_of!(info);
        let info_len = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .expect("JOBOBJECT_EXTENDED_LIMIT_INFORMATION fits in u32");
        // SAFETY: `JobObjectExtendedLimitInformation` is the documented info
        // class; the struct is value-initialized above. We pass a pointer to
        // it + its byte size. The buffer lives on this stack frame for the
        // duration of the call. The call only reads `info` (no out-param).
        let ok = unsafe {
            windows::Win32::System::JobObjects::SetInformationJobObject(
                job_guard.as_handle(),
                JobObjectExtendedLimitInformation,
                info_ptr.cast::<core::ffi::c_void>(),
                info_len,
            )
        };
        ok.map_err(|e| Error::Platform(format!("SetInformationJobObject failed: {e}")))?;

        // SAFETY: `child` is a valid process HANDLE from ShellExecuteExW;
        // `job` is a valid job HANDLE we just created and configured.
        // AssignProcessToJobObject adds the child to the job — from now on
        // the child's lifetime is bound to the job (G10). The child handle
        // remains valid (we still own it for is_child_alive polling).
        let ok = unsafe { AssignProcessToJobObject(job_guard.as_handle(), child) };
        ok.map_err(|e| Error::Platform(format!("AssignProcessToJobObject failed: {e}")))?;

        Ok(job_guard.into_handle())
    }

    #[cfg(windows)]
    fn wait_for_probe(&self, port: u16) -> Result<bool> {
        let deadline = Instant::now() + Duration::from_millis(LAUNCH_TIMEOUT_MS);
        let interval = Duration::from_millis(LAUNCH_REPROBE_INTERVAL_MS);
        // Upper bound on a single probe's wall-clock cost (T-10 = 500ms).
        // We check `now + probe_budget >= deadline` BEFORE each probe so a
        // probe started near the deadline cannot overshoot T-11 by a full
        // probe timeout (~500ms). The original post-probe check could
        // reach ~5500ms in the worst case (probe at t=4999ms returns at
        // t≈5499ms, then the deadline check fires) — T-11 is 5000ms hard.
        let probe_budget = Duration::from_millis(HTTP_TIMEOUT_MS);
        loop {
            // Pre-flight: if a probe would overshoot the deadline, stop.
            // `checked_add` guards against Instant overflow (theoretically
            // impossible with a 5s deadline but defensive).
            let next_probe_end = Instant::now().checked_add(probe_budget);
            if next_probe_end.is_none_or(|end| end >= deadline) {
                return Ok(false);
            }
            if self.probe(port) == ProviderTier::Full {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(interval);
        }
    }
}

impl<C: HttpClient> Drop for OhmSupervisor<C> {
    /// G10 safety net: if `shutdown` was never called (e.g. sidebar crashed
    /// cleanly via panic), close the Job Object handle on drop so the kernel
    /// reaps the elevated child. This is the documented G10 contract.
    fn drop(&mut self) {
        // Close handles without broadcasting (the app layer may be mid-tear-
        // down; broadcasting from Drop is unsafe).
        #[cfg(windows)]
        {
            if let Some(job_raw) = self.job_handle.take() {
                // SAFETY: job_raw is a valid Job Object HANDLE; CloseHandle
                // triggers kernel reap of all assigned processes (G10).
                let job = HANDLE(job_raw as *mut core::ffi::c_void);
                // SAFETY: see above — owned handle, last close on drop.
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
            }
            if let Some(child_raw) = self.child_handle.take() {
                let child = HANDLE(child_raw as *mut core::ffi::c_void);
                // SAFETY: owned process handle, last close on drop.
                let _ = unsafe { windows::Win32::Foundation::CloseHandle(child) };
            }
        }
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
    // Parse as a generic JSON value to avoid coupling to the LhmNode schema
    // (this module intentionally does NOT depend on sidebar-adapter-ohm's
    // lhm_model — it only needs the signature shape).
    let Ok(parsed): serde_json::Result<serde_json::Value> = serde_json::from_str(body) else {
        return false;
    };
    let Some(arr) = parsed.as_array() else {
        return false; // top-level must be an array
    };
    let Some(first) = arr.first() else {
        return false; // empty array is not a signature
    };
    let Some(obj) = first.as_object() else {
        return false; // first element must be an object
    };
    // LHM v0.9.x emits PascalCase (Text, Children). We tolerate camelCase
    // (text, children) for forward-compat. Require BOTH a text-ish AND a
    // children-ish key (the two universal LHM root-node fields).
    let has_text = obj.contains_key("Text") || obj.contains_key("text");
    let has_children = obj.contains_key("Children") || obj.contains_key("children");
    has_text && has_children
}

/// Convert the time remaining before an absolute shutdown deadline to the
/// millisecond timeout expected by Win32. Saturation keeps the conversion
/// safe even if callers pass a very distant deadline.
fn remaining_wait_millis(deadline: Instant) -> u32 {
    u32::try_from(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(u32::MAX)),
    )
    .unwrap_or(u32::MAX)
}

/// The outcome of [`pick_free_port`] — distinguishes the
/// "LHM-already-running" case (caller MUST NOT relaunch, MUST NOT mark
/// itself as the launcher per G10) from the "free port" case (caller
/// relaunches LHM and owns the child).
///
/// Cited: AD-8 step 1, G10 ownership, Story 6.4 H4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortPick {
    /// The port is free (connection refused). Caller launches LHM on it.
    Free(u16),
    /// LHM is already answering on this port with a signature body. Caller
    /// reuses the port WITHOUT relaunching and WITHOUT setting
    /// `sidebar_launched` (G10 — sidebar does not own the user's LHM).
    AlreadyRunning(u16),
}

/// Walk the T-45 port fallback chain (17127..=17137) and return the first port
/// that is NOT occupied by a non-LHM service. Classification per port:
/// - LHM signature body → [`PortPick::AlreadyRunning`] (LHM already running
///   — AD-8 step 1; caller reuses the port WITHOUT relaunching).
/// - connection-refused/timeout → port is free → [`PortPick::Free`].
/// - non-LHM body (HTML etc.) → port occupied by a foreign service → skip.
///
/// Returns `Ok(PortPick)` on the first usable port, or `Err` if all 11
/// candidates are occupied by non-LHM services (Boundary #9 "out of
/// fallback chain" → the caller keeps Basic tier).
///
/// Cited: Story 6.4 Boundary #9. T-45.
pub fn pick_free_port<C: HttpClient>(client: &C) -> Result<PortPick> {
    for port in PORT_RANGE_START..=PORT_RANGE_END {
        let url = format!("http://127.0.0.1:{port}/data.json");
        match client.get(&url) {
            Ok(body) => {
                if is_lhm_signature(&body) {
                    // LHM already running here — reuse the port, no relaunch.
                    debug!(port, "pick_free_port: LHM already running on this port");
                    return Ok(PortPick::AlreadyRunning(port));
                }
                // Non-LHM body — occupied by a foreign service. Skip.
                debug!(
                    port,
                    "pick_free_port: occupied by non-LHM service, skipping (T-45 fallback)"
                );
            }
            Err(OhmError::HttpFailed(_)) => {
                // Connection refused — port is free.
                debug!(port, "pick_free_port: port free (connection refused)");
                return Ok(PortPick::Free(port));
            }
            Err(OhmError::Timeout) => {
                // T-10 timeout — ambiguous; treat as occupied (something is
                // listening but not answering). Skip to be safe.
                debug!(port, "pick_free_port: T-10 timeout, treating as occupied");
            }
            Err(OhmError::NotJson(_) | OhmError::Parse(_)) => {
                // Foreign service returned a non-JSON body. Skip.
                debug!(port, "pick_free_port: non-JSON body, treating as occupied");
            }
            Err(OhmError::RejectedUrl(reason)) => {
                warn!(port, %reason, "pick_free_port: URL rejected by G16 loopback policy");
            }
        }
    }
    Err(Error::Platform(format!(
        "all ports {PORT_RANGE_START}-{PORT_RANGE_END} occupied by non-LHM services (T-45 fallback chain exhausted)"
    )))
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
    // The canonical appSettings block we want present after patching.
    let app_settings_block = build_app_settings_block(port);

    if !config_path.exists() {
        // Case (a) — create the file from scratch with both keys.
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<configuration>\n{app_settings_block}</configuration>\n"
        );
        std::fs::write(config_path, content).map_err(|e| {
            Error::Platform(format!(
                "failed to write LHM config {}: {e}",
                config_path.display()
            ))
        })?;
        return Ok(());
    }

    let original = std::fs::read_to_string(config_path).map_err(|e| {
        Error::Platform(format!(
            "failed to read LHM config {}: {e}",
            config_path.display()
        ))
    })?;

    let patched = patch_app_settings(&original, &app_settings_block);
    if patched == original {
        // No change needed (already correct). Still write to be idempotent
        // (no-op write is harmless).
        return Ok(());
    }
    std::fs::write(config_path, patched).map_err(|e| {
        Error::Platform(format!(
            "failed to write patched LHM config {}: {e}",
            config_path.display()
        ))
    })?;
    Ok(())
}

/// Build the canonical `<appSettings>` block with BOTH required keys set to
/// the correct values for the chosen port. The block uses a 2-space indent
/// convention so it reads cleanly inside `<configuration>`. The indent is
/// included at the START of the block (before `<appSettings>`) AND before
/// each `<add>` line; callers must NOT prepend additional indent (the
/// patcher handles inserting after `<configuration>` + newline).
fn build_app_settings_block(port: u16) -> String {
    format!(
        "  <appSettings>\n    \
<add key=\"{CONFIG_KEY_WEB_SERVER}\" value=\"true\" />\n    \
<add key=\"{CONFIG_KEY_LISTENER_PORT}\" value=\"{port}\" />\n  \
</appSettings>\n"
    )
}

/// Patch an existing config string: if `<appSettings>` is absent, inject the
/// canonical block after `<configuration>`; if present, replace the existing
/// block with the canonical one (handles both "keys present with wrong
/// values" and "keys missing"). Preserves all other sections (`<startup>`,
/// `<runtime>`). Idempotent: a second patch on an already-canonical file is
/// a no-op (the canonical block's leading/trailing whitespace is reproduced
/// exactly).
fn patch_app_settings(original: &str, canonical_block: &str) -> String {
    if let Some(start) = original.find("<appSettings>") {
        // Cases (c)/(d) — appSettings present. Locate the closing
        // `</appSettings>` by searching from `start` (NOT from `end`, which
        // is not yet bound). Replace the block wholesale with the canonical
        // one. To keep idempotency, we also consume any leading whitespace
        // (indent) before `<appSettings>` in the original so the canonical
        // block's own leading indent replaces it cleanly.
        let block_start = original[..start].rfind('\n').map_or(0, |i| i + 1);
        let end_marker = "</appSettings>";
        let end = original[start..]
            .find(end_marker)
            .map_or(original.len(), |i| start + i + end_marker.len());
        // Consume any trailing newline so we don't double-up.
        let after_end = if original[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        let before = &original[..block_start];
        let after = &original[after_end..];
        format!("{before}{canonical_block}{after}")
    } else {
        // Case (b) — no appSettings. Inject after `<configuration>`.
        if let Some(cfg_end) = original.find("<configuration>") {
            let insert_at = cfg_end + "<configuration>".len();
            let (before, after) = original.split_at(insert_at);
            format!("{before}\n{canonical_block}{after}")
        } else {
            // No `<configuration>` root — wrap the canonical block ourselves.
            format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<configuration>\n{canonical_block}</configuration>\n"
            )
        }
    }
}

/// Patch LHM's PersistentSettings file (`LibreHardwareMonitor.config`).
/// Unlike the .exe.config (which is small + managed by us), this file is
/// 100KB+ with sensor plot data that MUST be preserved. We surgically
/// update/add the two web server keys within the existing `<appSettings>`
/// section without touching any other entries.
///
/// If the file doesn't exist yet (first launch), we create it with just the
/// two required keys. LHM will populate the rest on its first run.
fn patch_lhm_user_config(config_path: &Path, port: u16) -> Result<()> {
    if !config_path.exists() {
        // First launch: create minimal config with both keys.
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <configuration>\n  <appSettings>\n    \
             <add key=\"{CONFIG_KEY_WEB_SERVER}\" value=\"true\" />\n    \
             <add key=\"{CONFIG_KEY_LISTENER_PORT}\" value=\"{port}\" />\n  \
             </appSettings>\n</configuration>\n"
        );
        std::fs::write(config_path, content).map_err(|e| {
            Error::Platform(format!(
                "failed to write LHM user config {}: {e}",
                config_path.display()
            ))
        })?;
        return Ok(());
    }

    let original = std::fs::read_to_string(config_path).map_err(|e| {
        Error::Platform(format!(
            "failed to read LHM user config {}: {e}",
            config_path.display()
        ))
    })?;

    // Surgically update/add keys without touching the rest of the file.
    let mut patched = original.clone();

    // Update or add runWebServerMenuItem.
    patched = update_app_setting_key(&patched, CONFIG_KEY_WEB_SERVER, "true");
    // Update or add listenerPort.
    patched = update_app_setting_key(&patched, CONFIG_KEY_LISTENER_PORT, &port.to_string());

    if patched != original {
        std::fs::write(config_path, patched).map_err(|e| {
            Error::Platform(format!(
                "failed to write patched LHM user config {}: {e}",
                config_path.display()
            ))
        })?;
    }
    Ok(())
}

/// Update or add a single `<add key="..." value="..." />` entry within an
/// existing `<appSettings>` section. If the key already exists, its value is
/// replaced in-place. If it doesn't exist, it's inserted right after the
/// opening `<appSettings>` tag.
fn update_app_setting_key(xml: &str, key: &str, value: &str) -> String {
    // Pattern: <add key="KEY" value="OLD_VALUE" />
    let search = format!(r#"key="{key}""#);
    if let Some(key_pos) = xml.find(&search) {
        // Key exists — find the enclosing <add ... /> and replace the value.
        // Walk backwards to find "<add " before this key.
        let add_start = xml[..key_pos].rfind("<add ").map_or(0, |i| i);
        // Walk forward to find " />" after this key.
        let add_end_rel = xml[key_pos..]
            .find("/>")
            .map_or(xml.len(), |i| key_pos + i + 2);
        let old_entry = &xml[add_start..add_end_rel];
        let new_entry = format!(r#"<add key="{key}" value="{value}" />"#);
        xml.replace(old_entry, &new_entry)
    } else if let Some(appsettings_open) = xml.find("<appSettings>") {
        // Key doesn't exist but appSettings section does — insert after opening tag.
        let insert_at = appsettings_open + "<appSettings>".len();
        let (before, after) = xml.split_at(insert_at);
        format!("{before}\n    <add key=\"{key}\" value=\"{value}\" />{after}")
    } else {
        // No appSettings section at all — this shouldn't happen for the user
        // config (LHM always creates one), but handle it gracefully.
        xml.to_string()
    }
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
    // LHM user-config patching (LibreHardwareMonitor.config)
    // ==========================================================

    /// patch_lhm_user_config creates the file if it doesn't exist.
    #[test]
    fn user_config_patch_creates_file_when_absent() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.config");
        patch_lhm_user_config(&path, 17_127).expect("patch");
        let content = fs::read_to_string(&path).expect("read");
        assert!(
            content.contains(r#"key="runWebServerMenuItem" value="true""#),
            "must set runWebServerMenuItem=true"
        );
        assert!(
            content.contains(r#"key="listenerPort" value="17127""#),
            "must set listenerPort=17127"
        );
    }

    /// patch_lhm_user_config preserves existing sensor data and only updates the web server keys.
    #[test]
    fn user_config_patch_preserves_existing_data_and_updates_keys() {
        let dir = TempDir::new().expect("TempDir");
        let path = dir.path().join("LibreHardwareMonitor.config");
        // Simulate a real LHM user config with sensor data + old web server settings.
        let existing = r#"<?xml version="1.0"?>
<configuration>
  <appSettings>
    <add key="listenerPort" value="8085" />
    <add key="/amdcpu/0/load/0/plot" value="false" />
    <add key="/amdcpu/0/load/1/plot" value="false" />
    <add key="startMinimized" value="false" />
  </appSettings>
</configuration>"#;
        fs::write(&path, existing).expect("write");

        patch_lhm_user_config(&path, 17_127).expect("patch");
        let patched = fs::read_to_string(&path).expect("read");

        // Web server keys must be correct.
        assert!(
            patched.contains(r#"key="runWebServerMenuItem" value="true""#),
            "runWebServerMenuItem must be added"
        );
        assert!(
            patched.contains(r#"key="listenerPort" value="17127""#),
            "listenerPort must be updated to 17127"
        );
        assert!(
            !patched.contains("8085"),
            "old listenerPort 8085 must be replaced"
        );
        // Sensor data must be preserved.
        assert!(
            patched.contains("/amdcpu/0/load/0/plot"),
            "sensor plot data must survive"
        );
        assert!(
            patched.contains("/amdcpu/0/load/1/plot"),
            "sensor plot data must survive"
        );
        assert!(
            patched.contains("startMinimized"),
            "other settings must survive"
        );
    }

    /// update_app_setting_key replaces value when key exists.
    #[test]
    fn update_app_setting_key_replaces_existing() {
        let xml = r#"<appSettings><add key="foo" value="old" /></appSettings>"#;
        let patched = update_app_setting_key(xml, "foo", "new");
        assert!(patched.contains(r#"value="new""#));
        assert!(!patched.contains(r#"value="old""#));
    }

    /// update_app_setting_key inserts when key doesn't exist.
    #[test]
    fn update_app_setting_key_inserts_new() {
        let xml = r#"<appSettings><add key="other" value="1" /></appSettings>"#;
        let patched = update_app_setting_key(xml, "foo", "bar");
        assert!(patched.contains(r#"key="foo" value="bar""#));
        assert!(
            patched.contains(r#"key="other" value="1""#),
            "existing keys must survive"
        );
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
        let pick = pick_free_port(&mock).expect("free port");
        assert_eq!(
            pick,
            PortPick::Free(17_128),
            "must fall back to Free(17128) when 17127 occupied"
        );
    }

    /// T-45: if 17127 is free (connection-refused), pick it (don't skip).
    #[test]
    fn pick_free_port_prefers_17127_when_free() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let pick = pick_free_port(&mock).expect("free port");
        assert_eq!(pick, PortPick::Free(17_127));
    }

    /// T-45: if all 11 candidates (17127-17137) are occupied by non-LHM
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
    /// as `AlreadyRunning` (caller reuses WITHOUT relaunching). This is the
    /// "already-running LHM" path (AD-8 step 1, H4).
    #[test]
    fn pick_free_port_returns_already_running_when_lhm_signature() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(LHM_SIGNATURE_BODY.to_string()));
        let pick = pick_free_port(&mock).expect("port");
        assert_eq!(
            pick,
            PortPick::AlreadyRunning(17_127),
            "LHM-detected port is AlreadyRunning"
        );
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

    // ==========================================================
    // Boundary #11 (H4) — already-running LHM: launch_elevated MUST
    // skip relaunch (no spurious UAC, sidebar_launched stays false).
    // ==========================================================

    /// Cited: AD-8 step 1 ("if LHM already running, use it, do not
    /// relaunch"), G10 (kill on shutdown only if sidebar-launched).
    ///
    /// RED: `launch_elevated` cannot distinguish "LHM already running"
    /// from "free port" — `pick_free_port` returned `Ok(port)` for both
    /// before H4. So when LHM is already answering on 17127,
    /// `launch_elevated` unconditionally patched the config + called
    /// `ShellExecuteW("runas")` + set `sidebar_launched=true`. The
    /// consequences: a redundant UAC prompt, the new child failing to
    /// bind 17127 (silent failure), and sidebar believing it owns a
    /// process it doesn't really own.
    ///
    /// This test stages an LHM-shaped probe body on 17127 with the LHM
    /// exe present in the tempdir (so the binary-existence check passes)
    /// and asserts `launch_elevated` returns `Ok(17127)` WITHOUT setting
    /// `sidebar_launched`, WITHOUT touching `child_handle`/`job_handle`.
    #[test]
    fn launch_elevated_skips_relaunch_when_lhm_already_running() {
        let mut mock = MockFakeClient::new();
        // 17127 answers with an LHM signature body — LHM is already up.
        mock.expect_get()
            .returning(move |_| Ok(LHM_SIGNATURE_BODY.to_string()));
        let (mut sv, dir) = supervisor_in_tempdir(mock);

        // Stage a stub LHM exe so the binary-existence early-return does
        // NOT fire (we want to reach the pick_free_port branch). The stub
        // is an empty file; launch_elevated never actually executes it
        // because the already-running path must short-circuit first.
        let exe = dir.path().join("LibreHardwareMonitor.exe");
        std::fs::write(&exe, b"stub").expect("write stub exe");

        let result = sv.launch_elevated();
        assert!(result.is_ok(), "must return Ok(port), got {result:?}");
        assert_eq!(result.unwrap(), 17_127, "resolved port is 17127");
        // H4 core assertions — sidebar did NOT take ownership.
        assert!(
            !sv.sidebar_launched(),
            "AD-8 step 1: sidebar MUST NOT mark itself as the launcher when LHM is already running"
        );
        assert!(
            sv.child_handle.is_none(),
            "no child handle when reusing already-running LHM"
        );
        assert!(
            sv.job_handle.is_none(),
            "no job handle when reusing already-running LHM"
        );
        assert_eq!(sv.resolved_port(), Some(17_127));
    }

    /// AD-8 step 1 must not require the bundled executable when an existing
    /// LHM instance is already answering. Reuse is specifically the path for
    /// installations where the user owns LHM independently of this binary.
    #[test]
    fn launch_elevated_reuses_running_lhm_without_bundled_exe() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(move |_| Ok(LHM_SIGNATURE_BODY.to_string()));
        let (mut sv, _dir) = supervisor_in_tempdir(mock);

        // Do not create LibreHardwareMonitor.exe: probing must happen before
        // the bundled-executable check on the already-running path.
        let result = sv.launch_elevated();
        assert_eq!(result.expect("reuse must succeed"), 17_127);
        assert!(!sv.sidebar_launched());
        assert!(sv.child_handle.is_none());
        assert!(sv.job_handle.is_none());
        assert_eq!(sv.resolved_port(), Some(17_127));
        assert!(
            !sv.lhm_config.exists(),
            "already-running reuse must not patch a launch config"
        );
    }

    // ==========================================================
    // Boundary #12 (H5) — launch_elevated re-entry guard
    // ==========================================================

    /// Cited: G10 (no orphan elevated LHM during sidebar's lifetime).
    ///
    /// RED: `launch_elevated` blindly overwrites `child_handle`/`job_handle`
    /// on retry without `CloseHandle`. A retry after a T-11 timeout leaks
    /// the first child+job handle until process exit.
    ///
    /// We simulate a previously-launched supervisor by manually setting
    /// `sidebar_launched=true` (the production post-launch state) and
    /// assert a second `launch_elevated` returns `Err` without invoking
    /// `shellexecute_runas` again.
    #[test]
    fn launch_elevated_reentry_returns_err_without_leaking_handles() {
        let mut mock = MockFakeClient::new();
        mock.expect_get()
            .returning(|_| Err(OhmError::HttpFailed("connection refused".to_string())));
        let (mut sv, dir) = supervisor_in_tempdir(mock);

        // Stage LHM exe so the early-return doesn't fire.
        let exe = dir.path().join("LibreHardwareMonitor.exe");
        std::fs::write(&exe, b"stub").expect("write stub exe");

        // Simulate a prior successful launch (the H5 re-entry state). We
        // can't easily exercise the real `shellexecute_runas` path in a
        // unit test (it triggers UAC), so we mark the state directly.
        sv.sidebar_launched = true;
        sv.child_handle = Some(0xDEAD_BEEF_isize); // sentinel — never a real handle
        sv.job_handle = Some(0xCAFE_F00D_isize);

        let result = sv.launch_elevated();
        assert!(
            result.is_err(),
            "H5: re-entry MUST return Err (got {result:?})"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.to_lowercase().contains("already")
                || msg.to_lowercase().contains("launched")
                || msg.to_lowercase().contains("re-entry")
                || msg.to_lowercase().contains("twice")
                || msg.to_lowercase().contains("leak"),
            "H5 error must mention already/launched/re-entry/twice/leak: {msg}"
        );
        // State is unchanged — the sentinel handles are still there (we
        // did NOT overwrite them, did NOT leak them).
        assert!(sv.sidebar_launched, "state unchanged");
        assert_eq!(sv.child_handle, Some(0xDEAD_BEEF_isize));
        assert_eq!(sv.job_handle, Some(0xCAFE_F00D_isize));
    }

    #[test]
    fn remaining_wait_millis_never_exceeds_deadline_budget() {
        let deadline = Instant::now() + Duration::from_millis(25);
        assert!(remaining_wait_millis(deadline) <= 25);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(remaining_wait_millis(deadline), 0);
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

    #[allow(clippy::struct_excessive_bools)]
    #[derive(Debug, Default)]
    struct FakeLaunchResources {
        cleanup_flags: u8,
        child_started: bool,
        job_created: bool,
        job_configured: bool,
        job_assigned: bool,
    }

    impl FakeLaunchResources {
        fn child_terminated(&self) -> bool {
            self.cleanup_flags & 0b0001 != 0
        }
        fn child_reaped(&self) -> bool {
            self.cleanup_flags & 0b0010 != 0
        }
        fn child_closed(&self) -> bool {
            self.cleanup_flags & 0b0100 != 0
        }
        fn job_closed(&self) -> bool {
            self.cleanup_flags & 0b1000 != 0
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum SetupFailure {
        Create,
        Configure,
        Assign,
    }

    fn simulate_job_setup_failure(
        stage: SetupFailure,
        resources: &mut FakeLaunchResources,
    ) -> std::result::Result<(), SetupFailure> {
        resources.child_started = true;
        if stage == SetupFailure::Create {
            cleanup_failed_post_launch_setup(resources);
            return Err(stage);
        }

        resources.job_created = true;
        if stage == SetupFailure::Configure {
            cleanup_failed_post_launch_setup(resources);
            return Err(stage);
        }

        resources.job_configured = true;
        if stage == SetupFailure::Assign {
            cleanup_failed_post_launch_setup(resources);
            return Err(stage);
        }

        resources.job_assigned = true;
        Ok(())
    }

    impl ChildTermination for FakeLaunchResources {
        fn terminate_and_reap(&mut self) {
            self.cleanup_flags = 0b1111;
        }
    }

    #[test]
    fn post_shell_execute_setup_failures_terminate_reap_and_close_handles() {
        for stage in [
            SetupFailure::Create,
            SetupFailure::Configure,
            SetupFailure::Assign,
        ] {
            let mut resources = FakeLaunchResources::default();
            let error = simulate_job_setup_failure(stage, &mut resources)
                .expect_err("each configured stage must fail in this seam");
            assert_eq!(error, stage, "the seam must report the failing stage");
            assert!(resources.child_started, "child launched before {stage:?}");
            assert_eq!(
                resources.job_created,
                !matches!(stage, SetupFailure::Create),
                "job creation state must match {stage:?}"
            );
            assert_eq!(
                resources.job_configured,
                matches!(stage, SetupFailure::Assign),
                "job configuration state must match {stage:?}"
            );
            assert!(
                !resources.job_assigned,
                "failed setup must not commit the job"
            );
            assert!(
                resources.child_terminated(),
                "child terminated for {stage:?}"
            );
            assert!(resources.child_reaped(), "child reaped for {stage:?}");
            assert!(
                resources.child_closed(),
                "child handle closed for {stage:?}"
            );
            assert!(resources.job_closed(), "job handle closed for {stage:?}");
        }
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
