# installer/

This folder holds the **Inno Setup source** for the sidebar Windows installer
plus the README shipped inside the portable ZIP.

## Contents

| File | Purpose |
|---|---|
| `sidebar.iss` | Inno Setup script. Builds `sidebar-setup.exe`. |
| `portable/README.txt` | README bundled inside the portable ZIP (`sidebar-portable-<version>.zip`). |
| `winget/manifest.yaml` | winget submission manifest. Deferred — submit only after the first stable release is published. |

## Build the installer locally

Requires [Inno Setup 6](https://jrsoftware.org/isdl.php) (`ISCC.exe` on PATH,
or at the default `%ProgramFiles(x86)%\Inno Setup 6\` location).

```pwsh
# From the repo root — build the release binary + fetch the LHM runtime first
.\scripts\fetch_ohm.ps1
cargo build --release --target x86_64-pc-windows-msvc

# Compile the installer
ISCC.exe installer\sidebar.iss
```

**Output**: `dist\sidebar-setup.exe` (≈12 MB). The `dist/` folder is gitignored —
the binary is a build artifact, not source.

To override the version stamped into the installer's Add/Remove Programs entry:

```pwsh
ISCC.exe /DAppVersion=0.2.0 installer\sidebar.iss
```

Without `/D`, the literal `AppVersion` in `sidebar.iss` (`0.1.0`) is used.

## Release pipeline

The release workflow (`.github/workflows/release.yml`) invokes `ISCC.exe` on
this script as part of the build stage. The resulting `sidebar-setup.exe` is
attached to the GitHub Release as `sidebar-setup-<version>.exe`. See the
workflow file for the full build → stage → publish flow.

## What the installer does

- Installs to `C:\Program Files\Sidebar` (per-machine).
- Bundles `sidebar-app.exe`, the LHM runtime (DLLs, config, locales), and the
  service/host binaries.
- Creates a Start Menu shortcut (and an optional desktop shortcut).
- Registers an Add/Remove Programs entry with uninstall support.
- **Service registration is currently disabled** (commented out in
  `sidebar.iss` `[Run]` section). The app uses the HTTP-to-LHM path which
  works without the service. The service binaries are bundled so they're
  ready when the named-pipe consumer lands.

## Portable ZIP

The portable ZIP is built by `release.yml` from `staging/` (sidebar.exe + LHM
runtime) with `portable/README.txt` copied in as `README.txt`. It is NOT built
by `sidebar.iss` — the Inno Setup script only produces the installer EXE.
