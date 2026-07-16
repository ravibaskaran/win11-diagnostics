# Code-Signing Policy — sidebar-v1 (Story 9.1)

## Overview

`sidebar.exe` is code-signed via the [SignPath Foundation](https://signpath.org/)
in CI at release time. The bundled `LibreHardwareMonitor.exe` (MPL-2.0) is
hash-pinned via `resources/ohm.sha256` and verified on every CI run via
`fetch_ohm.ps1 -CheckOnly` (Story 6.5).

This document is the authoritative policy referenced by:
- `SECURITY.md` (user-facing trust statement)
- `README.md` (download instructions)
- `.github/workflows/release.yml` (Story 9.2 — the signing CI job)
- `docs/backlog/guardrails.md` G18 (supply-chain automation)
- `docs/privacy-policy.md` (data-handling statement required by SignPath)

## Trust boundary

- **Host binary (`sidebar-app.exe`)**: MIT-licensed, signed by SignPath
  Foundation in CI. No self-signing. If SignPath is unavailable, CI may create
  an explicitly labelled **draft-only** unsigned artifact for maintainer
  review; it must not be treated as a signed public release.
- **Sensor host binary (`sidebar-monitor-host.exe`, Story 15.1)**: MIT-licensed
  (the C# source is in the repo), runs elevated, loads LibreHardwareMonitorLib.dll.
  Signed by SignPath Foundation in CI. Hash-pinned like the LHM binary.
- **Service binary (`sidebar-monitor-svc.exe`, Story 16.1)**: MIT-licensed, runs
  as `LocalSystem`, owns the sensor host. Signed by SignPath Foundation in CI.
  This is the highest-trust binary in the product — a LocalSystem service
  requires the strictest review (G11/G19 HITL on every change).
- **Installer (`sidebar-setup.exe`, Story 16.3)**: The Inno Setup output EXE,
  signed by SignPath Foundation in CI. It is the user-facing trust entry point.
- **Bundled LHM library (`LibreHardwareMonitorLib.dll`)**: MPL-2.0, loaded by
  `sidebar-monitor-host.exe`. We re-verify the SHA-256 pin at every CI run.
  We do NOT re-sign the bundled library. (Note: Epic 15 replaces the LHM GUI
  binary with the library directly; the `LibreHardwareMonitor.exe` trust
  boundary entry becomes historical once Story 15.3 deletes the HTTP path.)
- **Bundled LHM GUI (`LibreHardwareMonitor.exe`, DEPRECATED by Epic 15)**:
  MPL-2.0, upstream-signed. Retained only for the portable fallback path until
  Story 15.3 removes the HTTP dependency entirely.
- **User distribution channels**: GitHub Releases (installer EXE) + winget
  (`InstallerType: inno`). No direct download from any other source.

## SignPath Foundation submission

**Status: pending external submission (HITL).** SignPath Foundation requires:
1. OSI-approved license verification (host MIT, bundled MPL-2.0 — both qualify).
2. Public repo (`github.com/ravibaskaran/win11-diagnostics`).
3. Public **code-signing policy** page reachable from the repo homepage
   (this document; surfaced in the GitHub Releases body via
   `.github/workflows/release.yml`).
4. Public **privacy policy** page reachable from the repo homepage
   (`docs/privacy-policy.md`, linked from `README.md` and `SECURITY.md`).
5. MFA-enabled approvers.
6. Project slug: `sidebar`; signing policy slug: `release`.

The submission is a manual HITL action (G11/G19). Once approved, the
`SIGNPATH_API_TOKEN` secret + `release-approver` GitHub Environment are
provisioned by the maintainer, and Story 9.2's `release.yml` can complete
the signed-release pipeline.

## Hash verification

`resources/ohm.sha256` pins the bundled LHM binary:

```
fe216a48a48a6048156a133a39b960437d781a4c9214e5abc0f26f666f61ba22  LibreHardwareMonitor.exe
```

The `lhm-hash` CI job (Story 6.5) runs `fetch_ohm.ps1 -CheckOnly` on every
PR + push to main. A hash mismatch fails the build immediately.

## Edge cases

- **SignPath rejection / downtime**: Story 9.2's `release.yml` keeps the
  workflow observable by producing an explicitly labelled unsigned **draft**
  with a prominent warning. Maintainers must not promote that draft or submit
  it to winget as a signed release. Users are NEVER silently given an unsigned
  binary labeled as signed.
- **LHM hash mismatch on release**: CI fails fast. The release tag is NOT
  cut until the hash matches the committed pin.
- **LHM upstream 404 (retired release)**: `fetch_ohm.ps1` emits an
  actionable error pointing to the pinned release URL. The maintainer must
  explicitly bump the pin (R7, HITL).
