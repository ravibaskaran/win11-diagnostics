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

    /// G16: production HTTP requests must target loopback only.
    #[error("HTTP URL rejected by loopback-only G16 policy: {0}")]
    RejectedUrl(String),
}

/// Validate the G16 loopback-only exception before any HTTP request is sent.
/// Accepts `http://127.0.0.0/8` and `http://[::1]` authorities, with optional
/// ports and paths; hostnames, remote IPv4 addresses, and non-HTTP schemes are
/// rejected without DNS resolution.
pub fn validate_loopback_url(url: &str) -> Result<(), OhmError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| OhmError::RejectedUrl("scheme must be http".to_string()))?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(OhmError::RejectedUrl(
            "missing or credentialed authority".to_string(),
        ));
    }

    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return Err(OhmError::RejectedUrl("unterminated IPv6 host".to_string()));
        };
        if &bracketed[..end] != "::1" {
            return Err(OhmError::RejectedUrl("IPv6 host is not ::1".to_string()));
        }
        let suffix = &bracketed[end + 1..];
        if !suffix.is_empty() {
            let port = suffix
                .strip_prefix(':')
                .ok_or_else(|| OhmError::RejectedUrl("invalid IPv6 authority".to_string()))?;
            port.parse::<u16>()
                .map_err(|_| OhmError::RejectedUrl("invalid port".to_string()))?;
        }
        return Ok(());
    }

    if authority.matches(':').count() > 1 {
        return Err(OhmError::RejectedUrl(
            "IPv6 hosts must use [::1] notation".to_string(),
        ));
    }
    let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
    let parsed = host
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| OhmError::RejectedUrl("host must be a loopback IP".to_string()))?;
    if !parsed.is_loopback() {
        return Err(OhmError::RejectedUrl(
            "IPv4 host is not in 127.0.0.0/8".to_string(),
        ));
    }
    if !port.is_empty() {
        port.parse::<u16>()
            .map_err(|_| OhmError::RejectedUrl("invalid port".to_string()))?;
    }
    Ok(())
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
            // G16: validate only the initial URL and never follow a 3xx
            // Location that could escape the loopback-only boundary.
            .redirects(0)
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
        validate_loopback_url(url)?;
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

#[cfg(test)]
mod tests {
    use super::validate_loopback_url;
    use super::{HttpClient, OhmError, RealHttpClient};

    #[test]
    fn loopback_validator_accepts_ipv4_loopback_range() {
        assert!(validate_loopback_url("http://127.0.0.1:17127/data.json").is_ok());
        assert!(validate_loopback_url("http://127.255.255.254:8080/data.json").is_ok());
    }

    #[test]
    fn loopback_validator_accepts_ipv6_loopback() {
        assert!(validate_loopback_url("http://[::1]:17127/data.json").is_ok());
    }

    #[test]
    fn loopback_validator_rejects_hostnames_and_remote_ips() {
        assert!(validate_loopback_url("http://localhost:17127/data.json").is_err());
        assert!(validate_loopback_url("http://192.168.1.10:17127/data.json").is_err());
        assert!(validate_loopback_url("https://127.0.0.1:17127/data.json").is_err());
    }

    #[test]
    fn real_client_rejects_non_loopback_before_transport() {
        let error = RealHttpClient::new()
            .get("http://localhost:17127/data.json")
            .expect_err("G16 must reject before ureq transport");
        assert!(matches!(error, OhmError::RejectedUrl(_)));
    }

    #[test]
    fn real_client_does_not_follow_redirects_to_remote_hosts() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local redirect fixture");
        let address = listener.local_addr().expect("local fixture address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept redirect request");
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("configure redirect fixture timeout");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1:9/data.json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write redirect response");
        });

        let result = RealHttpClient::new().get(&format!("http://{address}/data.json"));
        server.join().expect("redirect fixture thread");
        assert!(
            matches!(result, Ok(ref body) if body.is_empty())
                || matches!(result, Err(OhmError::HttpFailed(ref message)) if message.contains("HTTP 302")),
            "redirect must not be followed to remote host, got {result:?}"
        );
    }
}
