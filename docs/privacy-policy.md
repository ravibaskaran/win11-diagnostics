# Privacy Policy — sidebar (win11-diagnostics)

> **Effective date:** 2026-07-13 · **Version:** 1.0

sidebar is a **local-only desktop application**. It does not collect,
transmit, sell, share, or otherwise disclose any personal data, telemetry,
usage statistics, hardware readings, or identifiers to the maintainer, to
any third party, or to any network service.

This document is the privacy policy referenced by [`SECURITY.md`](../SECURITY.md),
the [`README.md`](../README.md) download instructions, and the
[`code-signing-policy`](../signpath/code-signing-policy.md) submitted to the
[SignPath Foundation](https://signpath.org/).

---

## 1. Summary

sidebar has **zero runtime network egress**. The only network request the
application can ever issue is an **optional loopback HTTP GET** to a
bundled [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
subprocess on `http://127.0.0.1:<port>/data.json` (the IPv4 loopback
address, the host's own machine). This request is made **only** when the
user explicitly opts into **Full mode** by clicking the status pill and
accepting the Windows UAC prompt. In **Basic mode** (the default) no
network request of any kind is made.

This zero-egress posture is an architectural invariant, enforced at the
source level: the URL is validated against a literal-loopback check
before any transport (`http.rs::validate_loopback_url`), and the HTTP
client is constructed with redirects disabled. A 60-second `netstat`
snapshot diff is part of the smoke checklist (`verify/smoke-checklist.ps1`,
guardrails.md G16) and confirms no outbound sockets are opened during a
run.

---

## 2. Data we do NOT collect

sidebar does **not**:

- Collect telemetry, analytics, usage statistics, or crash reports.
- Auto-update over the network (the auto-update skeleton is hard-OFF in
  v1.0 — `updater::should_check` always returns `false`).
- Phone home for license checks, feature flags, A/B tests, or
  entitlement.
- Send hardware sensor readings, processes, network counters, or
  bandwidth totals anywhere off the host.
- Require, transmit, or store any account, identity, email, IP address,
  MAC address, machine-id, Windows advertising-id, or hardware
  fingerprint.
- Open any inbound port. The bundled LibreHardwareMonitor subprocess
  binds to `127.0.0.1` only (the IPv4 loopback), never to external
  interfaces.

---

## 3. Data the application stores locally

All persistent state lives under `%APPDATA%\sidebar\` (Windows) on the
user's own machine:

| Path | Contents | Purpose |
|---|---|---|
| `%APPDATA%\sidebar\config.toml` | Plain-text TOML | User preferences: poll interval, theme, docked edge, billing-cycle start day, hotkey, monitor id, metric order. |
| `%APPDATA%\sidebar\bandwidth.db` | SQLite (WAL) | Monthly per-network-adapter bandwidth totals. Cumulative RX/TX counters per cycle, current + previous. |

No other file is written. No registry key outside the standard
`HKCU\Control Panel\Desktop` (read-only theme tracking) or
`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize`
(read-only dark/light detection) is read or written.

The application does **not** use any cookies, browser storage, or
advertising SDK.

---

## 4. What happens when Full mode / UAC is enabled

When the user clicks the **BASIC** status pill and accepts the Windows
UAC prompt, sidebar:

1. Patches the bundled LibreHardwareMonitor config (enables its
   read-only HTTP API; the API serves `127.0.0.1` only).
2. Launches the bundled LibreHardwareMonitor as an elevated, hidden
   subprocess via `ShellExecuteExW("runas")`.
3. Wraps that subprocess in a Win32 **Job Object** with
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so the kernel reaps it if
   sidebar crashes (no orphan elevated processes).
4. Issues `GET http://127.0.0.1:<port>/data.json` (500ms timeout,
   redirects disabled, loopback-validated) to read CPU package
   temperature, fan speeds, voltages, and motherboard sensors.
5. Renders those readings in the sidebar.

**The HTTP connection is loopback-only.** No sensor reading, identifier,
or payload is transmitted off the host. The LibreHardwareMonitor process
listens on `127.0.0.1` and rejects external interfaces by configuration.

When the user clicks the pill again to switch back to Basic, or when
sidebar exits cleanly, the supervisor sends a shutdown signal to the
LibreHardwareMonitor subprocess **that sidebar launched**. If
LibreHardwareMonitor was already running before sidebar started
(user-started), sidebar does not kill it (guardrails.md G10 — ownership
rule).

---

## 5. Third-party components

sidebar bundles or links the following third-party components. Each
component's own privacy policy (if any) governs that component; sidebar
does not invoke any network feature of any component.

| Component | License | Purpose | Privacy |
|---|---|---|---|
| [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) (bundled binary) | MPL-2.0 | Hardware sensors (CPU temp, fans, voltages) | [Upstream policy](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor#privacy) — LHM is local-only; sidebar verifies the SHA-256 of the bundled binary against `resources/ohm.sha256` before launch. |
| [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/eframe) | MIT | Immediate-mode GUI | No network code. |
| [sysinfo](https://github.com/GuillaumeGomez/sysinfo) | MIT | CPU / RAM / process / disk counters | No network code. |
| [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) | MIT/Apache-2.0 | NVIDIA GPU readings (when present) | No network code. |
| [starship-battery](https://github.com/starship/rust-battery) | MIT/Apache-2.0 | Battery state | No network code. |
| [rusqlite](https://github.com/rusqlite/rusqlite) (bundled SQLite) | MIT | Local bandwidth database | No network code. |
| [ureq](https://github.com/algesten/ureq) | MIT/Apache-2.0 | HTTP client (loopback only) | Used **only** for `http://127.0.0.1/...`; redirects disabled. |
| [windows](https://github.com/microsoft/windows-rs) (Win32 bindings) | MIT/Apache-2.0 | AppBar, DPI, hotkey, DWM, Job Object, GetIfTable2 | No network code. |

The full dependency list with licenses is audited by `cargo deny` on
every PR (`deny.toml`, guardrails.md G3/G18). Only licenses on the T-32
allowlist (MIT, Apache-2.0, MPL-2.0, BSD-3-Clause, ISC, Zlib,
Unicode-DFS-2016, CC0-1.0) are permitted.

---

## 6. Data transfer

sidebar does **not transfer any data** off the user's machine, including
to the maintainer. There is no server-side component. Source-code
contributions (issues, pull requests) submitted to the public GitHub
repository are governed by GitHub's own policies, not this document.

---

## 7. Contact / report an issue

- **Bug reports and feature requests:** please use the public
  [GitHub issue tracker](https://github.com/ravibaskaran/win11-diagnostics/issues).
- **Security reports:** please do **not** open a public issue. Follow
  the private-disclosure procedure in [`SECURITY.md`](../SECURITY.md).
- **Privacy questions about this policy:** open an issue with the
  `privacy` label, or contact the maintainer via the GitHub profile.

---

## 8. Changes to this policy

Material changes to this policy will be made by pull request, recorded
in the git history of this file, and dated in the header. The
application's behavior is the source of truth: if a future version
introduces a new network egress (it will not, under the v1 guardrails),
this policy will be updated to describe it before the release ships, and
the change will be flagged in the release notes.

---

## 9. Effective date

**2026-07-13.** This policy applies to all builds of sidebar
(win11-diagnostics) v1.x. Earlier development builds (pre-v1.0.0 tags)
are covered by the same zero-egress invariant, but this policy document
was not published until the date above.
