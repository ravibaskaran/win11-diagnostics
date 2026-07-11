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

## Trust boundary

- **Host binary (`sidebar-app.exe`)**: MIT-licensed, signed by SignPath
  Foundation in CI. No self-signing; no unsigned distribution channel.
- **Bundled LHM (`LibreHardwareMonitor.exe`)**: MPL-2.0, upstream-signed by
  the LibreHardwareMonitor maintainers. We re-verify the SHA-256 pin
  (`fe216a48...1ba22`) at every CI run. We do NOT re-sign the bundled LHM.
- **User distribution channels**: GitHub Releases + winget only. No direct
  download from any other source.

## SignPath Foundation submission

**Status: pending external submission (HITL).** SignPath Foundation requires:
1. OSI-approved license verification (host MIT, bundled MPL-2.0 — both qualify).
2. Public repo (`github.com/ravibaskaran/win11-diagnostics`).
3. MFA-enabled approvers.
4. Project slug: `sidebar`; signing policy slug: `release`.

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

- **SignPath rejection / downtime**: Story 9.2's `release.yml` falls back to
  unsigned GitHub Releases + winget with a prominent "unsigned" warning.
  Users are NEVER silently given an unsigned binary labeled as signed.
- **LHM hash mismatch on release**: CI fails fast. The release tag is NOT
  cut until the hash matches the committed pin.
- **LHM upstream 404 (retired release)**: `fetch_ohm.ps1` emits an
  actionable error pointing to the pinned release URL. The maintainer must
  explicitly bump the pin (R7, HITL).
