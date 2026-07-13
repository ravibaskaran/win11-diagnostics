# win11-diagnostics

> *Glanceable system health, calmly.*

A lightweight, always-on desktop sidebar for Windows 11 that shows live
hardware telemetry — CPU/GPU temperatures, clocks, utilization, fan speeds,
voltages, power draw; memory and VRAM; per-drive storage and throughput;
per-network-adapter throughput; and **monthly bandwidth consumption tracking
per network interface**.

A ground-up Rust reimagining of the user-facing experience of
[SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics),
rebuilt natively for Windows 11 with a strict lightweight mandate and a
two-tier sensor model that degrades gracefully when elevated privileges are
unavailable.

---

## Why Rust?

SidebarDiagnostics (the original) is a C#/.NET/WPF application. This project
rebuilds the same user experience in Rust for three reasons:

1. **Memory safety without a garbage collector.** The sidebar runs 24/7 as a
   background overlay. Rust's ownership model guarantees no use-after-free,
   no null dereferences, no buffer overflows — classes of bugs that plague
   long-running C# apps that depend on Win32 interop and native libraries.

2. **Smaller, faster binaries.** Rust compiles to a single static binary
   with no runtime dependency on .NET, the CLR, or a JIT. The current local
   x64 release artifact is roughly 11.6 MB with LTO + symbol stripping. The
   application targets a sub-500 ms first frame; actual startup varies by
   Windows machine and disk state.

3. **Predictable resource usage.** Rust's zero-cost abstractions and lack of
   a garbage collector mean RSS stays flat. There's no GC pause, no JIT warmup,
   no surprise memory spikes from runtime-managed heaps.

The trade-off: CPU package temperature, fan speeds, and motherboard voltages
require low-level hardware sensor access that Rust cannot do alone on Windows.
These sensors are read through a bundled [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
subprocess (MPL-2.0). The sidebar itself remains pure Rust; the LHM binary
is a pinned, hash-verified external dependency that runs with elevated
privileges only when the user explicitly requests Full mode.

## Why Windows 11 only?

The sidebar uses Windows 11-specific APIs that don't exist on Windows 10 or
earlier:

- **Per-Monitor DPI Awareness v2** — crisp text on 4K displays at any scaling
- **`WDA_EXCLUDEFROMCAPTURE`** — the sidebar can hide itself from screen
  capture (streamers, screen-sharers) at the OS level
- **`SHAppBarMessage`** — native taskbar docking; other windows snap around
  the sidebar instead of overlapping it
- **`SetProcessDpiAwarenessContext`** — per-window DPI, not per-process

Porting to Windows 10 would require fallback paths for each of these, which
isn't planned for v1.

---

## How to use it

### Basic mode (no admin required)

The sidebar starts in **Basic mode** by default. No elevation, no UAC prompt,
no administrator privileges. You see:

- **CPU** — utilization, frequency, power draw (via `sysinfo`)
- **Memory** — used / free / total RAM (via `sysinfo`)
- **GPU** — NVIDIA utilization/temperature (via NVML, if an NVIDIA GPU is present)
- **Battery** — charge level, charging/discharging state (via `starship-battery`)
- **Disk** — per-drive capacity + R/W throughput (via PDH counters)
- **Network** — per-NIC RX/TX throughput + **monthly bandwidth tracking** (via `GetIfTable2`)
- **Per-process** — top-N CPU/RAM consumers

Basic mode is designed to be **lightweight**: no more than 0.5% average CPU
per sensor source, with memory bounded toward the approximately 80 MiB RSS
target. These limits are checked by automated tests and the Win11 smoke
checklist; results vary with hardware and enabled providers.

### Full mode (opt-in, requires UAC)

If you need **CPU package temperature, fan speeds, or motherboard voltages**,
click the gray **BASIC** pill in the sidebar header. Windows will show a UAC
prompt. Accept it, and the sidebar:

1. Patches the bundled LibreHardwareMonitor config (enables its HTTP API)
2. Launches LHM as an elevated, hidden subprocess (via `ShellExecuteW("runas")`)
3. Wraps LHM in a **Job Object** so it's automatically killed if the sidebar
   crashes (no orphan processes)
4. Probes `http://127.0.0.1:17127/data.json` to confirm LHM is responding
5. Switches to Full mode — the status pill turns green, and temperature/fan/
   voltage readings appear alongside the existing metrics

If the bundled LHM build does not start its Web Server automatically, open
LHM's **View → Web Server** menu once, then click the status pill again. Until
the loopback probe succeeds, the sidebar deliberately remains in Basic mode.

**Privacy:** Full mode makes a **loopback-only** HTTP connection to
`127.0.0.1`. The sidebar has **zero runtime network egress** in any mode
(verified via `netstat` snapshot diff). No telemetry, no auto-update checks,
no cloud sync — nothing leaves your machine.

If the sidebar is closed normally, it sends a signal to LHM to shut down
cleanly. If the sidebar crashes, the Job Object ensures LHM is reaped by the
kernel — no orphan processes survive.

### Global hotkey

Press **Ctrl+Shift+S** to toggle **click-through** mode. When enabled, mouse
clicks pass through the sidebar to windows behind it — useful when the sidebar
is covering something you need to interact with. Press again to toggle off.

### First-run wizard

On first launch, a wizard appears:
1. Choose the docked edge (left/right/top/bottom — right is default)
2. Choose the target monitor (primary by default)
3. Set your billing-cycle start day (for monthly bandwidth tracking)
4. Choose light/dark/system theme

After completing or skipping the wizard, the sidebar appears immediately.
Settings can be changed later via the gear icon (⚙) in the header.

### Bandwidth tracking

The sidebar tracks **cumulative bytes per network interface** per billing
cycle. Your cycle start day is set in the first-run wizard (default: 1st of
each month). The sidebar shows:

- Current cycle: total RX/TX per adapter
- Days until reset
- Previous cycle history (retention: current + previous)

Data is persisted in a local SQLite database (`%APPDATA%\sidebar\bandwidth.db`).
If the sidebar is restarted mid-cycle, accumulated totals are **rehydrated**
from the database — no data loss on restart.

---

## Configuration

The sidebar reads its configuration from `%APPDATA%\sidebar\config.toml`.
Key settings:

| Setting | Default | Range | Description |
|---|---|---|---|
| `poll_interval_seconds` | 10 | 1–60 | How often sensors refresh |
| `[display] temp_unit` | Celsius | Celsius/Fahrenheit | Temperature unit |
| `[display] decimal_base` | true | true/false | Decimal (GB=10⁹) vs binary (GiB=2³⁰) |
| `[display] raw_values` | false | true/false | Show Hz/bytes/bps instead of GHz/GB/Mbps |
| `[display] force_opaque` | false | true/false | Disable transparency (for GPU compatibility) |
| `[display] hide_from_capture` | false | true/false | Hide sidebar from screen capture (OBS, Teams, etc.) |
| `[process] top_n` | 5 | 1–50 | Number of top processes to show |
| `[graph] window` | 60 | 10–600 | Sparkline history window (samples) |
| `[bandwidth] cycle_start_day` | Day(1) | Day(1–28) / LastDayOfMonth | Billing cycle start |
| `[theme] mode` | Dark | Dark/Light/System | Theme preference |
| `[theme] accent` | #4CAF50 | #RRGGBB | Accent color |
| `[dock] edge` | Right | Left/Right/Top/Bottom | Docked edge |
| `[dock] monitor_id` | primary | DeviceID or "primary" | Target monitor |
| `[hotkeys] click_through` | Ctrl+Shift+S | Modifiers+Key | Click-through toggle hotkey |

---

## Download

Pre-built binaries will be available on the [GitHub Releases](https://github.com/ravibaskaran/win11-diagnostics/releases)
page once code signing is set up. Until then, you can build from source.

## Build from source

**Prerequisites:**
- [Rust](https://rustup.rs/) 1.95+ (MSRV enforced by `sysinfo` 0.39.3)
- MSVC Build Tools 2022+ (Visual Studio Installer → "Desktop development with C++")
- PowerShell 7+ (`winget install Microsoft.PowerShell`)
- Git

```pwsh
git clone https://github.com/ravibaskaran/win11-diagnostics.git
cd win11-diagnostics

# Download + hash-verify the bundled LHM binary
.\scripts\fetch_ohm.ps1

# Build the release binary
cargo build --release --target x86_64-pc-windows-msvc

# Keep the complete LHM runtime beside the sidebar executable for Full mode
Copy-Item .\resources\* .\target\x86_64-pc-windows-msvc\release\ -Recurse -Force

# Run it
.\target\x86_64-pc-windows-msvc\release\sidebar-app.exe
```

---

## Troubleshooting

| Symptom | What to do |
|---|---|
| The sidebar opens in **BASIC** mode | This is the safe default. Click the status pill only when you want the extra LHM sensors and accept the UAC prompt. |
| Full mode does not start | Confirm that the bundled LHM files were downloaded by `fetch_ohm.ps1`, then retry. A firewall or an incompatible running LHM instance can prevent the loopback probe from succeeding. |
| A monitor or edge is wrong | Open settings, select the target monitor and dock edge again, then restart after a Windows display-topology change. |
| Bandwidth totals look empty | Generate traffic, wait for the next poll, and confirm `%APPDATA%\\sidebar\\bandwidth.db` is writable. |
| The UI is too large or too small | Check Windows display scaling. The application uses per-monitor DPI awareness. |

The application has no telemetry, account system, cloud sync, or background
update service. It reads local sensors only; the optional LHM bridge is
loopback-only.

---

## License

The host workspace is **MIT** ([`LICENSE`](LICENSE)).

The bundled `LibreHardwareMonitor.exe` and its license remain **MPL-2.0**
([`resources/LibreHardwareMonitor.LICENSE.txt`](resources/LibreHardwareMonitor.LICENSE.txt)).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the development workflow.

## Acknowledgments

- [SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics) —
  the original C#/.NET sidebar that inspired this project
- [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) —
  the open-source hardware monitoring library (MPL-2.0)
- [egui](https://github.com/emilk/egui) — the immediate-mode Rust GUI framework
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo) — cross-platform system
  information crate
