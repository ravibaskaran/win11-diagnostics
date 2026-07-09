//! HTTP client abstraction for the OHM adapter.
//!
//! LibreHardwareMonitor exposes a small HTTP server on `127.0.0.1:<port>`
//! (default 17127, T-45). The adapter `GET`s `/data.json` on every tick and
//! parses the resulting JSON sensor tree (see [`crate::lhm_model`]).
//!
//! ## Why a trait (not a concrete `ureq::Agent`)
//!
//! Unit tests must NOT hit the network. Abstracting behind [`HttpClient`] lets
//! `mockall` generate a `MockHttpClient` that returns canned JSON for the
//! contract tests (Story 3.6 Happy Path + Boundaries #1-#4). The production
//! wiring uses [`RealHttpClient`] which wraps a `ureq::Agent` configured with
//! the T-10 500ms timeout.
//!
//! ## T-10 timeout (HITL)
//!
//! The 500ms timeout is the contract: localhost roundtrips are sub-millisecond
//! normally, but if LHM is hung or the socket accept queue is stalled we must
//! not block the poller's tick budget. We expose this as a `const` so it can
//! be cited from the docs + gated by HITL (guardrail G11).
//!
//! ## Cited
//!
//! - Story 3.6 TDD contract (Happy Path #2 — mock HttpClient; Boundary #1
//!   connection-refused; Boundary #2 500ms timeout; Boundary #3 non-LHM
//!   service returns HTML 404)
//! - architecture.md AD-2 (revised) + AD-7 (revised)
//! - nfr-thresholds.md T-10 (500ms HTTP timeout — HITL), T-45 (port 17127)

use std::time::Duration;

/// T-10: 500ms HTTP timeout. Localhost HTTP is sub-millisecond normally, so
/// this is generous — but it bounds a stuck LHM subprocess from blocking the
/// poller. HITL (G11) — do not change without architect sign-off.
pub const HTTP_TIMEOUT_MS: u64 = 500;

/// T-45: LHM's default HTTP port. Configurable via [`RealHttpClient::new`]
/// / [`OhmAdapterGeneric`] constructor; the actual resolved port is supplied
/// by `OhmSupervisor` (Story 6.4). Hardcoded here as the default.
pub const DEFAULT_OHM_PORT: u16 = 17127;

/// Errors from an LHM HTTP fetch + parse. Each variant maps to a Story 3.6
/// boundary contract: connection-refused (#1), timeout (#2), non-JSON body
/// (#3), and JSON-shape violation (#4 / parse failures on the sensor tree).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OhmError {
    /// Boundary #1 (connection refused / network-layer failure). LHM is not
    /// running, or the port is wrong.
    #[error("HTTP request failed: {0}")]
    HttpFailed(String),

    /// Boundary #2 (T-10 timeout). The 500ms budget elapsed with no response.
    #[error("HTTP timeout after {HTTP_TIMEOUT_MS}ms (T-10)")]
    Timeout,

    /// Boundary #3 (non-LHM service on the port returned HTML — e.g. 404
    /// page). The HTTP request succeeded but the body is not JSON.
    #[error("response body is not valid JSON: {0}")]
    NotJson(String),

    /// Boundary #4 (JSON shape mismatch). The body parsed as JSON but did not
    /// match the `LhmNode` tree contract.
    #[error("JSON parse error: {0}")]
    Parse(String),
}

/// Abstraction over the LHM HTTP data source. The production impl wraps a
/// `ureq::Agent`; tests substitute a mock (via `mockall`) that returns canned
/// `/data.json` payloads.
///
/// Implementations need not be `Send + Sync` themselves — the adapter wraps
/// the client in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait HttpClient {
    /// Issue `GET <url>` and return the response body as a string.
    ///
    /// Errors are mapped to [`OhmError`] so the adapter can decide (empty
    /// readings + `debug!`/`warn!`) without inspecting `ureq::Error`'s shape.
    fn get(&self, url: &str) -> Result<String, OhmError>;
}

/// Production HTTP client wrapping a `ureq::Agent` with the T-10 timeout.
///
/// One agent is reused for the adapter's lifetime (ureq pools connections +
/// caches DNS — though for `127.0.0.1` neither matters, reusing the agent
/// avoids per-tick allocator churn).
#[derive(Debug, Clone)]
pub struct RealHttpClient {
    agent: ureq::Agent,
}

impl RealHttpClient {
    /// Construct a client. The T-10 500ms timeout is baked in; it is not
    /// overridable here (HITL-guarded). The port is NOT stored — the URL
    /// (including port) is supplied by the adapter on each `get` call.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
            .build();
        Self { agent }
    }
}

impl Default for RealHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for RealHttpClient {
    fn get(&self, url: &str) -> Result<String, OhmError> {
        // `ureq` distinguishes timeout from other transport errors via the
        // `Error::Timeout` variant (2.x); map accordingly so the adapter can
        // log a targeted message per Boundary #2.
        match self.agent.get(url).call() {
            Ok(resp) => resp
                .into_string()
                .map_err(|e| OhmError::HttpFailed(format!("read body: {e}"))),
            Err(ureq::Error::Status(code, _resp)) => {
                // Non-2xx status. LHM normally returns 200 with JSON on
                // `/data.json`; a 404 here almost certainly means a different
                // service is on this port (HTML response — Boundary #3).
                Err(OhmError::HttpFailed(format!("HTTP {code}")))
            }
            Err(ureq::Error::Transport(t)) => {
                if t.kind() == ureq::ErrorKind::ConnectionFailed {
                    // ECONNREFUSED — LHM is not running. Boundary #1.
                    Err(OhmError::HttpFailed("connection refused".to_string()))
                } else if is_timeout_transport(&t) {
                    // Boundary #2 (T-10). ureq 2.12 surfaces timeouts as
                    // ErrorKind::Io with an inner io::ErrorKind::TimedOut;
                    // we walk the source chain to detect it.
                    Err(OhmError::Timeout)
                } else {
                    Err(OhmError::HttpFailed(format!("{t}")))
                }
            }
        }
    }
}

/// Inspect a `ureq::Transport`'s source chain for a `std::io::Error` with
/// kind `TimedOut`. ureq 2.12 does NOT expose a dedicated `TimedOut`
/// `ErrorKind` — agent timeouts propagate as `ErrorKind::Io` wrapping an
/// `io::Error(TimedOut)`. Returns `true` if such a chain is found.
fn is_timeout_transport(t: &ureq::Transport) -> bool {
    let Some(src) = std::error::Error::source(t) else {
        return false;
    };
    if let Some(io_err) = src.downcast_ref::<std::io::Error>() {
        // Direct io::Error.
        if io_err.kind() == std::io::ErrorKind::TimedOut
            || io_err.kind() == std::io::ErrorKind::WouldBlock
        {
            return true;
        }
        // Some ureq versions nest the timeout one level deeper.
        if let Some(inner) = std::error::Error::source(io_err) {
            if let Some(inner_io) = inner.downcast_ref::<std::io::Error>() {
                if inner_io.kind() == std::io::ErrorKind::TimedOut {
                    return true;
                }
            }
        }
    }
    false
}
