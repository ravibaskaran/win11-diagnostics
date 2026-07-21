# Code-Signing Policy — sidebar

## Current status: UNSIGNED

**v0.1.0 ships unsigned.** Releases built by
[`.github/workflows/release.yml`](../.github/workflows/release.yml) are not
code-signed. Windows SmartScreen will show "Windows protected your PC" on the
installer's first launch — users click **More info → Run anyway**. The
[`README.md`](../README.md) install section documents this.

This is the authoritative policy referenced by:
- `SECURITY.md` (user-facing trust statement)
- `README.md` (download + SmartScreen instructions)
- `.github/workflows/release.yml` (release pipeline)
- `docs/privacy-policy.md` (data-handling statement; SignPath Foundation
  requires a public privacy policy page as a submission prerequisite)

## Plan: signed releases via SignPath Foundation

The project is in the process of applying to the
[SignPath Foundation](https://signpath.org/) for free code-signing for
open-source projects. Once approved:

1. The `sign:` job (currently stripped from `release.yml`) will be re-added:
   it submits `sidebar.exe`, `sidebar-monitor-svc.exe`,
   `sidebar-monitor-host.exe`, and `sidebar-setup.exe` to SignPath for
   signing, then publishes the signed artifacts to the GitHub Release.
2. A `release-approver` GitHub Environment with MFA-enabled reviewers will
   gate the sign + publish steps.
3. The winget manifest will be submitted with a stable, signed release URL +
   SHA-256 (both require the first signed release to publish first).
4. SmartScreen reputation will build over time as users install the signed
   builds; the warning will eventually disappear for verified binaries.

Until then: **all releases are unsigned**. Users verify integrity via the
SHA-256 checksums published in each release body (computed by `release.yml`
for `sidebar.exe`, `sidebar-setup-<version>.exe`, and
`sidebar-portable-<version>.zip`).

## Trust boundary

- **Host binary (`sidebar-app.exe` / `sidebar.exe`)**: MIT-licensed. Currently
  unsigned. Will be signed by SignPath Foundation in CI once approved.
- **Sensor host binary (`sidebar-monitor-host.exe`)**: MIT-licensed (the C#
  source is in `resources/sidebar-monitor-host/`), runs elevated, loads
  `LibreHardwareMonitorLib.dll`. Currently unsigned. Will be signed.
- **Service binary (`sidebar-monitor-svc.exe`)**: MIT-licensed, intended to
  run as `LocalSystem`. Currently unsigned. Service registration is disabled
  in the installer (see `installer/sidebar.iss` `[Run]` section); the binary
  ships but is not registered. Will be signed when the named-pipe consumer
  lands + the service is re-enabled.
- **Installer (`sidebar-setup.exe`)**: Inno Setup output. Currently unsigned.
  Will be signed by SignPath Foundation in CI.
- **Bundled LHM library (`LibreHardwareMonitorLib.dll`)**: MPL-2.0, loaded by
  `sidebar-monitor-host.exe`. We verify the SHA-256 pin
  (`resources/ohm.sha256`) on every CI run via `fetch_ohm.ps1 -CheckOnly`
  (Story 6.5). We do NOT re-sign the bundled library.
- **Bundled LHM GUI (`LibreHardwareMonitor.exe`)**: MPL-2.0, upstream-signed.
  Retained only for the portable fallback path.
- **Distribution channels**: GitHub Releases (installer EXE + portable ZIP) +
  winget (deferred — `InstallerType: inno`, will be submitted after the first
  signed release). No direct download from any other source.

## Hash verification

`resources/ohm.sha256` pins the bundled LHM binary:

```
fe216a48a48a6048156a133a39b960437d781a4c9214e5abc0f26f666f61ba22  LibreHardwareMonitor.exe
```

The `lhm-hash` CI check (Story 6.5) runs `fetch_ohm.ps1 -CheckOnly` on every
PR + push to main. A hash mismatch fails the build immediately.

Release artifacts (installer + portable ZIP + the sidebar.exe inside) get
their SHA-256 checksums computed by `release.yml` and listed in each release's
body. Users verifying a download should compare against those published
checksums.

## Edge cases

- **LHM hash mismatch on release**: CI fails fast. The release tag is NOT
  cut until the hash matches the committed pin.
- **LHM upstream 404 (retired release)**: `fetch_ohm.ps1` emits an actionable
  error pointing to the pinned release URL. The maintainer must explicitly
  bump the pin.
- **SignPath rejection / downtime once wired**: `release.yml` will keep the
  workflow observable by producing an explicitly labelled unsigned **draft**
  with a prominent warning. Maintainers must not promote that draft or submit
  it to winget as a signed release. Users are NEVER silently given an unsigned
  binary labeled as signed.
