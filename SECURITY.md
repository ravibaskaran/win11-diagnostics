# Security Policy

## Supported Versions

sidebar (win11-diagnostics) is in active development. The `main` branch
reflects the current development version; tagged releases (`vX.Y.Z`) are
the supported versions once we ship v1.0.0.

| Version | Supported          |
| ------- | ------------------ |
| main    | ✅ active dev      |
| < 1.0.0 | ❌ pre-release     |
| ≥ 1.0.0 | ✅ (once shipped)  |

## Reporting a Vulnerability

**DO NOT open a public GitHub issue for security vulnerabilities.**

Instead, please report vulnerabilities privately:

1. Use GitHub's **"Report a vulnerability"** feature on the
   [Security advisories tab](https://github.com/ravibaskaran/win11-diagnostics/security/advisories/new).
   This keeps the report private to the maintainer until a fix is published.
2. Alternatively, email the maintainer directly (see the GitHub profile for
   contact info).

Please include:
- A description of the vulnerability and its impact.
- Steps to reproduce (proof-of-concept if possible).
- Affected versions (commit hash or tag).
- Suggested fix if you have one.

### Response timeline

- **Acknowledgement:** within 48 hours.
- **Initial assessment:** within 7 days.
- **Fix or mitigation:** target 30 days for high-severity, 90 days for low.
- **Public disclosure:** coordinated with the reporter after a fix ships.

## Security posture

sidebar is a **local-only desktop application** with **zero runtime network
egress** (guardrails.md G16). It does not:
- Send telemetry or analytics.
- Auto-update over the network.
- Phone home for license checks or feature flags.
- Open inbound ports (the bundled LibreHardwareMonitor subprocess binds to
  `127.0.0.1` only — `localhost`, never external interfaces).

Configuration and bandwidth-tracking state live under
`%APPDATA%\sidebar\` (config.toml + bandwidth.db). No data leaves the
machine.

## Threat model

- **Bundled LibreHardwareMonitor (LHM):** LHM runs elevated (admin) when
  Full mode is active. It is redistributed verbatim from the upstream LHM
  releases (MPL-2.0). sidebar verifies LHM's SHA-256 against a pinned hash
  (`resources/ohm.sha256`) before launch — a tampered LHM binary is
  rejected (Story 6.5).
- **Code signing:** sidebar.exe is signed via SignPath Foundation in CI
  (Story 9.1). Local dev builds are unsigned. Downloads should come from
  the GitHub Releases page or winget, never from third-party mirrors.
- **Supply chain:** every dependency is checked against the T-32 license
  allowlist and the RUSTSEC advisory database on every PR (Story 0.2/0.3).
  `Cargo.lock` is committed; reproducible builds are best-effort enforced.

## Acknowledgements

(To be populated as reports come in. We will credit reporters unless they
prefer to remain anonymous.)
