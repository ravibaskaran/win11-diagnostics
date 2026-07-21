sidebar — Portable Edition

This ZIP contains the portable edition of sidebar. It runs WITHOUT an
installer and WITHOUT admin rights for Basic mode.

USAGE:
  1. Extract the ZIP to any folder.
  2. Run sidebar.exe.
  3. Basic mode works immediately (CPU, RAM, disk, network, battery).

FULL MODE (temperature, fans, voltages):
  Click the gray BASIC pill in the sidebar header. Windows will show a
  UAC prompt each time you enable Full mode (this is expected — the
  hardware monitor needs elevated access).

  NOTE: Unlike the installer edition, the portable edition does NOT
  install a Windows Service. This means UAC recurs on every Full-mode
  launch. For set-and-forget elevation, use the installer edition
  (sidebar-setup.exe) from the GitHub Releases page.

PRIVACY:
  No telemetry. No network egress. All readings stay on your machine.

CREDITS:
  - LibreHardwareMonitor (MPL-2.0) — hardware sensor library
  - SidebarDiagnostics — the original C# app that inspired this project

LICENSE: MIT
