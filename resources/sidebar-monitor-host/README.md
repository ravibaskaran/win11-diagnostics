# sidebar-monitor-host build instructions

## Prerequisites
- .NET Framework 4.7.2 SDK (or `csc.exe` from the Windows SDK / Visual Studio Build Tools)
- `LibreHardwareMonitorLib.dll` in this directory (copied from `resources/`)

## Build (manual, no .csproj needed)
```pwsh
# Option A: using csc directly (lightest — no SDK install needed)
csc Program.cs `
    -r:../LibreHardwareMonitorLib.dll `
    -out:sidebar-monitor-host.exe `
    -target:exe

# Option B: using dotnet build (if .NET SDK is installed)
dotnet build sidebar-monitor-host.csproj -c Release
```

## Build (CI / build script)
`scripts/build_host.ps1` automates this:
```pwsh
pwsh scripts/build_host.ps1
```

## Output
`sidebar-monitor-host.exe` — a standalone .NET console app (~20 KB) that
loads `LibreHardwareMonitorLib.dll`, opens the `Computer`, and emits JSON
sensor frames to stdout every 1 second.

## Hash pin
After building, hash-pin the EXE:
```pwsh
(Get-FileHash sidebar-monitor-host.exe -Algorithm SHA256).Hash | Out-File host.sha256
```

The Rust `OhmSupervisor` will verify this hash before launching the host,
same as the LHM binary pin (`resources/ohm.sha256`).

## License
MIT — same as the sidebar workspace. The C# source is in this repo for
audit (G11/G19). `LibreHardwareMonitorLib.dll` is MPL-2.0 (bundled).

## Cited
Story 15.1, guardrails.md G10 + G16.
