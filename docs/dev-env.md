# Development Environment — sidebar

**Status:** Inventoried 2026-07-07, **fully provisioned 2026-07-08** (LHM v0.9.6 HTTP migration + all tools downloaded), **end-to-end certified 2026-07-08** (toolchain proven to compile + link a real `windows = 0.62` crate; LHM HTTP-endpoint behavior verified). `scripts/verify-dev-env.ps1` reports 16/16 green (15 OK + 1 expected NVIDIA-absent WARN) on this machine.

**Design goals:** (1) isolated from the system where possible, (2) relocatable — moving `D:\dev\sidebar\` to another Win11 machine should work after one activation step, (3) minimal redundant downloads of tools already installed.

---

## 0. End-to-end certification (2026-07-08)

Beyond the 16 `verify-dev-env.ps1` checks (which confirm presence + version), the dev env was certified end-to-end:

| Certification | Result | Notes |
|---|---|---|
| **Rust toolchain compiles a real crate** | ✅ PASS | Built a minimal `smoketest` crate using `windows = 0.62` with the `Win32_System_SystemInformation` feature; `cargo build --release` succeeded; `GetTickCount64()` FFI call returned `589356734`. This proves the MSVC linker + Windows SDK + windows-crate FFI path works. |
| **cargo subcommands run** | ✅ PASS | cargo-deny 0.19.9, cargo-audit 0.22.2, cargo-llvm-cov 0.8.7, cargo-nextest 0.9.140 all invoke cleanly. |
| **CI tools run** | ✅ PASS | actionlint 1.7.12, wingetcreate 1.12.8.0 verified. |
| **SQLite runs** | ✅ PASS | sqlite3 3.53.3 — ready to debug `bandwidth.db`. |
| **Bundled LHM v0.9.6 binary launches** | ✅ PASS | Process starts, runs .NET 10, no crash. |
| **LHM HTTP endpoint reachable on port 17127** | ⚠️ **REQUIRES CONFIG WRITE FIRST** | See §0.1 below — LHM's HTTP server is OFF by default. |
| **`scripts/verify-dev-env.ps1 -Json`** | ✅ PASS | 15 OK + 1 WARN, exit 0, machine-readable JSON well-formed. |
| **`scripts/env.ps1` no persistent mutation** | ✅ PASS | Verified zero persistent PATH changes at User + Machine scope after multiple invocations. |
| **Git ↔ GitHub sync** | ✅ PASS | Local HEAD = remote HEAD after each push. |

### 0.1 LHM HTTP-endpoint finding (critical for Story 6.4)

LHM v0.9.6's HTTP server (`/data.json`, `/metrics`) is **OFF by default**. Verified from source:
- `MainForm.cs`: `_runWebServer = new UserOption("runWebServerMenuItem", false, ...)` — starts false.
- `MainForm.cs`: `Server = new HttpServer(..., _settings.GetValue("listenerPort", 8085), ...)` — default port 8085.

**Implication for Story 6.4 (`OhmSupervisor`):** before launching `LibreHardwareMonitor.exe`, sidebar MUST patch the LHM config file (`resources/LibreHardwareMonitor.exe.config`) to set:
- `runWebServerMenuItem = true` (enables the HTTP server)
- `listenerPort = <chosen>` (default 17127 per T-45)

Without `runWebServerMenuItem=true`, LHM starts cleanly but listens on **zero ports** — sidebar's probe gets connection-refused and incorrectly concludes "Full mode unavailable." This is the #1 integration gotcha and is now documented in Story 6.4's Technical Context as a Verified-fact note, with a dedicated test case (#11) asserting the config-write includes both keys.

**Verified LHM config file location:** `Path.ChangeExtension(Application.ExecutablePath, ".config")` per `MainForm.cs:75` — i.e. `resources/LibreHardwareMonitor.exe.config`, alongside the exe. (NOT `%LOCALAPPDATA%`; LHM uses local-dir config.)

The config file format is XML (LHM's `PersistentSettings` class). sidebar will load the shipped `.exe.config`, mutate the two keys, and write back before launch.

---

## 1. Machine inventory (as inventoried 2026-07-07; toolchain confirmed 2026-07-08)

### 1.1 Hardware (this machine)
| Component | Detected | Project relevance |
|---|---|---|
| **CPU** | AMD Ryzen AI 7 350 (8+8 cores) @ 2.00 GHz | ⚠️ Reference hardware T-31 specifies Intel i5-1240P. See §6 below — T-31 has been generalized. |
| **GPU** | AMD Radeon 860M iGPU (459 MiB) | ⚠️ **NO NVIDIA GPU.** Story 3.2 (nvml-wrapper) cannot run on this machine. Only Story 3.6 (LHM HTTP bridge) covers AMD GPUs locally. NVIDIA testing requires a different machine or CI runner with NVIDIA hardware. |
| **RAM** | 24 GB (23.29 GiB visible) | Exceeds NFR-4 baseline. |
| **Storage** | C: 951 GB (291 used), D: 466 GB external exFAT, G: 951 GB FAT32 | Project lives on D:. |
| **Battery** | L24B3PK2, 98% | Story 3.3 testable locally. |
| **Network** | Wi-Fi (Realtek 8922AE WiFi 7), Bluetooth PAN, plus hidden adapters | Story 3.5 + Epic 5 (bandwidth) testable locally. Wi-Fi 5/Wi-Fi 4 show as "Not Present" — LUID-stability tests (T-24) must account for adapter appearance/disappearance. |
| **OS** | Windows 11 Pro Education 25H2 build 26200.x | NFR-5 covers 24H2 + 25H2. This machine is the 25H2 variant. |

### 1.2 Software installed (no action needed)

| Component | Version | Location | Notes |
|---|---|---|---|
| **Windows PowerShell** | 5.1.26100.8737 | System | Always present on Win11. |
| **PowerShell 7** | **7.6.3** | `C:\Program Files\PowerShell\7\pwsh.exe` | ✅ User already has pwsh 7. NOT on `PATH` for bash (a `~/bin/pwsh` shim shadows it); scripts should call the full path or invoke from pwsh-aware shells. |
| **Git for Windows** | 2.55.0.windows.2 | `/mingw64/bin/git` (via Git Bash) | ✅ Use this; no portable copy needed. |
| **winget** | v1.29.40-preview | `WindowsApps` | ✅ Available. |
| **.NET Framework 4.7.2+** | system component | Always on Win11 | ✅ Required by bundled OHM. |
| **VS Build Tools 2026 (VS 18)** | MSVC 14.51.36231 | `C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\` | ✅ **The linker + compiler Rust needs.** Verified: minimal `rustc` MSVC link succeeds. |
| **VS Build Tools 2022** | (shell installed, MSVC folder empty) | `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\` | Present but no MSVC toolset under it — VS 18 is the active one. |
| **Windows SDK** | 10.0.26100.0 | `C:\Program Files (x86)\Windows Kits\10\` | ✅ Win11 24H2 SDK. Provides import libs for the `windows` crate FFI. |
| **Rust toolchain** | **1.96.1** (active, was 1.94 — bumped 2026-07-08 per user) | `C:\Users\Ravi Baskaran\.cargo\`, `C:\Users\Ravi Baskaran\.rustup\` | ✅ Exceeds MSRV 1.95 (forced by sysinfo 0.39.3). |
| **rustup** | 1.29.0 | as above | ✅ |
| **rustc components** | clippy, rustfmt, miri, rust-analyzer, rust-docs, **llvm-tools** (added 2026-07-08) | as above | ✅ `llvm-tools` required by `cargo-llvm-cov` (T-43). |
| **cargo-binstall** | present | `~/.cargo/bin/cargo-binstall.exe` | ✅ Use this to fetch the missing subcommands. |
| **cargo-update** | present | `cargo-install-update.exe` | ✅ Useful for keeping subcommands current. |
| **scoop** | 0.5.3 | `C:\Users\Ravi Baskaran\scoop\` | ✅ **Preferred portable installer.** Many remaining tools install via scoop. |
| **scoop apps** | 7zip, bat, btop, btop-lhm, fd, fzf, jq, ripgrep, starship, fastfetch, imagemagick, marksman, ffmpeg, etc. | scoop apps dir | ✅ Excellent baseline. `btop-lhm` already bundles `LibreHardwareMonitorLib.dll` (the lib, not the GUI app — see §3.4). |
| **gh CLI** | 2.96.0 | `C:\Program Files\GitHub CLI\` | ✅ For winget-pkgs PR submission (Story 9.2). |
| **Python** | 3.14 | `C:\Program Files\Python314\` | Available for the PROGRESS.md parser (Story 11.4). |
| **Node.js** | present | `C:\Program Files\nodejs\` | Not required by sidebar; available. |
| **VS Code** | present | `C:\Program Files\Microsoft VS Code\` | Optional editor. |

### 1.3 Software installed during provisioning (2026-07-08) — all complete

The user performed the two system-level actions (Rust bump + llvm-tools component). The remaining project-local tools were downloaded into `D:\dev\sidebar\tools\` and `resources\` via `scripts/fetch_ohm.ps1`, `cargo binstall`, scoop, and direct GitHub release downloads.

| Component | Version | Location | Status |
|---|---|---|---|
| **Rust toolchain** | 1.96.1 | `~/.cargo`, `~/.rustup` | ✅ User bumped (was 1.94) |
| **rustup component: llvm-tools** | (matches toolchain) | as above | ✅ User added |
| **cargo-deny** | 0.19.9 | `D:\dev\sidebar\tools\cargo-bin\cargo-deny.exe` | ✅ Downloaded via cargo-binstall |
| **cargo-audit** | 0.22.2 | `D:\dev\sidebar\tools\cargo-bin\cargo-audit.exe` | ✅ Downloaded via cargo-binstall |
| **cargo-llvm-cov** | 0.8.7 | `D:\dev\sidebar\tools\cargo-bin\cargo-llvm-cov.exe` | ✅ Downloaded via cargo-binstall |
| **cargo-nextest** | 0.9.140 | `D:\dev\sidebar\tools\cargo-bin\cargo-nextest.exe` | ✅ Downloaded via cargo-binstall |
| **actionlint** | 1.7.12 | `D:\dev\sidebar\tools\ci\actionlint.exe` | ✅ Downloaded via scoop, copied to tools/ |
| **wingetcreate** | 1.12.8.0 | `D:\dev\sidebar\tools\ci\wingetcreate.exe` | ✅ Direct download from microsoft/winget-create releases (not in scoop main bucket) |
| **sqlite3** | 3.53.3 | `D:\dev\sidebar\tools\sqlite\sqlite3.exe` | ✅ Downloaded via scoop, copied to tools/ |
| **LibreHardwareMonitor** | v0.9.6 (.NET 10 build) | `D:\dev\sidebar\resources\LibreHardwareMonitor.exe` + 28 supporting DLLs + LICENSE | ✅ Downloaded via `scripts/fetch_ohm.ps1` |
| **LHM SHA-256 pin** | fe216a48...1ba22 | `D:\dev\sidebar\resources\ohm.sha256` | ✅ Committed pin + verified by `fetch_ohm.ps1 -CheckOnly` |
| **LHM LICENSE** | MPL-2.0 | `D:\dev\sidebar\resources\LibreHardwareMonitor.LICENSE.txt` | ✅ Fetched from LHM master |
| **SignPath CLI** / **dotnet sign** | — | — | N/A — CI-only per architecture §11.2; not installed locally |
| **.NET SDK 8+** | — | — | N/A — only needed for local signing, which we don't do |

**Verification:** `scripts/verify-dev-env.ps1` reports 16/16 checks (15 OK + expected NVIDIA-absent warning) on this machine.

---

## 2. What lives where (current state, post-provisioning 2026-07-08)

```
D:\dev\sidebar\
├── docs/                          ← design + backlog
│   └── backlog/                   ← README, guardrails, nfr-thresholds, tdd-fixtures,
│                                    regression-harness, PROGRESS, epics-and-stories
├── tools/                         ← ✅ populated 2026-07-08 — relocatable dev tooling
│   ├── cargo-bin/                 ← cargo subcommands (via cargo-binstall)
│   │   ├── cargo-deny.exe         (0.19.9, 8.9 MB)
│   │   ├── cargo-audit.exe        (0.22.2, 15.3 MB)
│   │   ├── cargo-llvm-cov.exe     (0.8.7, 4.3 MB)
│   │   └── cargo-nextest.exe      (0.9.140, 19.6 MB)
│   ├── ci/                        ← CI / release tools
│   │   ├── actionlint.exe         (1.7.12, 6.4 MB)
│   │   └── wingetcreate.exe       (1.12.8.0, 38.7 MB)
│   └── sqlite/                    ← sqlite3.exe for debugging bandwidth.db
│       └── sqlite3.exe            (3.53.3, 4.0 MB)
├── resources/                     ← ✅ populated 2026-07-08 — LHM v0.9.6 runtime
│   ├── LibreHardwareMonitor.exe   (4.5 MB, v0.9.6 .NET 10 build — HTTP endpoint)
│   ├── LibreHardwareMonitor.exe.config
│   ├── LibreHardwareMonitorLib.dll + .pdb + .xml
│   ├── *.dll                      (28 supporting .NET 10 DLLs — Aga.Controls, OxyPlot, etc.)
│   ├── {de,es,fr,it,ja,pl,ru,sv,tr,zh-CN,zh-Hant}/  (localized resources)
│   ├── ohm.sha256                 ← SHA-256 pin (fe216a48...1ba22)
│   └── LibreHardwareMonitor.LICENSE.txt   ← MPL-2.0
├── scripts/                       ← ✅ populated 2026-07-08 — Story 0.7 deliverables
│   ├── env.ps1                    ← activates the dev env (PATH prepend + prereq checks)
│   ├── verify-dev-env.ps1         ← full 15-point verification (CI pre-flight gate)
│   └── fetch_ohm.ps1              ← idempotent LHM download + hash-verify (Story 6.5)
├── .gitignore                     ← ✅ added 2026-07-08 (excludes bulky tools/ + resources/ binaries)
└── (future: crates/, Cargo.toml, etc. once Story 0.1 merges)
```

**Total relocatable footprint:** ~120 MB under `D:\dev\sidebar\` (tools/ ~97 MB, resources/ ~23 MB, scripts/ + docs/ ~0.5 MB). Git-ignored except for the small text pins (`ohm.sha256`, `LICENSE.txt`).

### Not under `D:\dev\sidebar\` (intentionally — already on the system)

These were already installed and relocating them would break other projects:
- **PowerShell 7** → `C:\Program Files\PowerShell\7\pwsh.exe`
- **Git for Windows** → `/mingw64/bin/git` (system Git Bash)
- **Rust toolchain** → `~/.cargo` + `~/.rustup` (the standard locations; relocating loses rust-analyzer integration with VS Code etc.)
- **VS Build Tools + Windows SDK** → system install (required by Rust's MSVC target globally; cannot usefully be folder-relocated without breaking the rustc linker discovery)
- **scoop** → `~/scoop` (the user's established portable-app manager — installing into our own folder would duplicate scoop's purpose)
- **gh CLI** → `C:\Program Files\GitHub CLI\`

### Activation

`scripts/env.ps1` (run once per PowerShell session, or dot-source from `$PROFILE`):

```powershell
# D:\dev\sidebar\scripts\env.ps1 — activate the sidebar dev environment
$sidebarRoot = Split-Path $PSScriptRoot
$env:PATH = "$sidebarRoot\tools\cargo-bin;$sidebarRoot\tools\ci;$sidebarRoot\tools\sqlite;$env:PATH"
# Rust toolchain lives at the system default (~/.cargo, ~/.rustup) — already on PATH.
# MSVC linker + Windows SDK discovered by rustc via vsdevshell.bat / registry — already on PATH via Build Tools.
Write-Host "sidebar dev env activated. Tools prefix: $sidebarRoot\tools" -ForegroundColor Green
```

Move `D:\dev\sidebar\` to another Win11 machine with the same system prerequisites (Rust ≥1.95, MSVC Build Tools, PowerShell 7) → run `scripts/env.ps1` → everything works.

---

## 3. Installation log (completed 2026-07-08)

For posterity, the actions taken during provisioning. Future contributors on a fresh machine follow the same sequence.

### 3.1 User actions (system-level, completed 2026-07-08)
```pwsh
rustup update stable     # 1.94 → 1.96.1
rustup default stable
rustup component add llvm-tools    # for cargo-llvm-cov
```

### 3.2 Swarm actions (project-local, completed 2026-07-08)
```pwsh
# Cargo subcommands — installed to a temp C: location first because cargo-binstall
# cannot atomically rename across the C:→D: drive boundary, then copied to tools/.
# Workaround: `CARGO_HOME=/tmp/cargo-tmp cargo binstall --no-confirm <pkgs>` then `cp`.
cargo binstall --no-confirm cargo-deny cargo-audit cargo-llvm-cov cargo-nextest
cp /tmp/cargo-tmp/bin/cargo-*.exe D:\dev\sidebar\tools\cargo-bin\

# CI tools via scoop (actionlint + sqlite are in the main bucket).
scoop install actionlint sqlite
cp ~\scoop\apps\actionlint\current\actionlint.exe D:\dev\sidebar\tools\ci\
cp ~\scoop\apps\sqlite\current\sqlite3.exe        D:\dev\sidebar\tools\sqlite\

# winget-create is NOT in scoop main — direct download from GitHub releases.
curl -sL "https://github.com/microsoft/winget-create/releases/download/v1.12.8.0/wingetcreate.exe" `
    -o D:\dev\sidebar\tools\ci\wingetcreate.exe

# LibreHardwareMonitor v0.9.6 (.NET 10 build — the HTTP-endpoint build, NOT the legacy WMI build).
# Done via scripts/fetch_ohm.ps1 (idempotent — re-running is safe).
.\scripts\fetch_ohm.ps1
# Offline deterministic check (no network, validates the committed pin + license).
.\scripts\fetch_ohm.ps1 -CheckOnly
```

### 3.3 SignPath / dotnet sign — deliberately NOT installed locally

Per architecture.md §11.2, code signing is **CI-only** (runs in GitHub Actions via the SignPath GitHub Action on release tags). The local dev machine does not need SignPath CLI, `dotnet sign`, or .NET SDK 8+. Local builds are unsigned — correct.

### 3.4 Already covered by scoop baseline

`scoop list` already provides: ripgrep (search), fd (find), fzf (fuzzy), bat (cat), jq (JSON), 7zip (archives), marksman (LSP for markdown), btop-lhm (ships `LibreHardwareMonitorLib.dll` — not reused; our architecture uses the subprocess + HTTP model, not the in-process lib). No action.

---

## 4. Verification — current state (16/16 checks; expected NVIDIA warning)

```pwsh
.\scripts\verify-dev-env.ps1
```

Last run on this machine (LAPTOP-PLN56DNU, 2026-07-08): **all 15 checks passed.**

The script verifies, in order:
1. System prerequisites: Rust ≥1.95, `llvm-tools` rustup component, MSVC linker reachable, Git, PowerShell 7+.
2. Project-local cargo subcommands: cargo-deny, cargo-audit, cargo-llvm-cov, cargo-nextest (under `tools\cargo-bin\`).
3. CI tools: actionlint, wingetcreate (under `tools\ci\`).
4. SQLite: sqlite3.exe under `tools\sqlite\`.
5. Bundled LHM: LibreHardwareMonitor.exe present, SHA-256 matches `ohm.sha256` pin, LICENSE file present.

Exits 0 on success, 1 on any failure. The CI regression gate (Story 11.2) calls this script as a pre-flight check before running the test matrix.

---

## 5. Compatibility with the relocatable-folder goal

**Honest assessment:** The original goal — "move `D:\dev\sidebar\` to another machine and everything works" — is **partially achievable**, with one caveat:

✅ **Achievable:** All project-specific tooling (cargo subcommands, actionlint, winget-create, sqlite, OHM binary, the Rust source itself) lives under `D:\dev\sidebar\` and is relocatable.

⚠️ **Not achievable without prerequisites:** The destination machine must already have:
1. **Rust 1.95+** (system install via rustup — relocating it requires env-var gymnastics that break VS Code integration)
2. **MSVC Build Tools** (system install — the Windows SDK is licensed/distributed as a system component)
3. **PowerShell 7** (system install)
4. **Git** (system install)

These four are the "system prerequisites" that `scripts/env.ps1`'s header documents. The script verifies them on activation. Moving to a fresh machine means: install those four system components, then copy `D:\dev\sidebar\`, then run `env.ps1`. The dev tooling (cargo subcommands, OHM binary) does NOT need re-downloading — it's already in the folder.

This is the **minimum system footprint** consistent with a Rust-on-Windows project that uses the `windows` crate. There is no portable workaround for the MSVC linker requirement.

---

## 6. Backlog / PRD / architecture corrections driven by this inventory

This inventory surfaced four correctness issues in the existing design docs. All applied during this provisioning cycle.

### 6.1 Reference hardware (T-31) — generalized
PRD/architecture specified "Intel i5-1240P / 16 GB / Win11 24H2" as THE reference. This machine is AMD Ryzen AI 7 350 / 24 GB / Win11 25H2. **T-31 generalized** to "any modern 8+ core CPU, ≥16 GB RAM, Win11 24H2 or 25H2" with per-machine calibration constants for the NFR-1 bench. See updated `nfr-thresholds.md`.

### 6.2 GPU coverage reality (R5)
This machine has **no NVIDIA GPU**. The backlog's Story 3.2 (nvml-wrapper) is annotated: locally untestable on this machine; CI runner with NVIDIA hardware required (or defer to a future hardware upgrade). R5 already documents this gap. AMD GPU coverage is via Story 3.6 (LHM Full mode) only.

### 6.3 Coverage tool: tarpaulin → llvm-cov
Story 11.2, Story 10.1, and T-42 specified `cargo-tarpaulin`. Tarpaulin is Linux-only (uses ptrace) and does not run on Windows at all. **Corrected to `cargo-llvm-cov`** everywhere (T-43). The `llvm-tools` rustup component (§3.1) is the prerequisite.

### 6.4 PowerShell 7 not on bash PATH
The `~/bin/pwsh` shim shadows the real `C:\Program Files\PowerShell\7\pwsh.exe` when called from bash. PowerShell scripts invoked from CI or from this orchestrator must use the full path or be invoked from a pwsh-launched shell. Documented in §1.2.

### 6.5 LHM WMI → HTTP migration (AD-2 + AD-7 revised, Stories 3.6/6.4/7.3 updated) — found during provisioning
The original architecture assumed LHM publishes sensors to the `root\LibreHardwareMonitor` WMI namespace. **LHM dropped WMI output in v0.9.5 (Jan 2026)** because .NET 10 removed WMI provider support. The maintainer confirmed in [issue #2143](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/2143): *"Yes, this is removed as .NET 10 doesn't support it anymore. Either integrate the library directly or use the HTTP endpoint."*

The replacement is LHM's **HTTP endpoint**: `GET http://127.0.0.1:<port>/data.json` returns a JSON sensor tree. Verified from `LibreHardwareMonitor.Windows.Forms/Utilities/HttpServer.cs` on master.

**Decision (Option B, per user 2026-07-08):** Pin LHM **v0.9.6** (Feb 2026 stable; first release with Ryzen AI 300-series support AND the HTTP endpoint), switch the integration from WMI to HTTP, and pick port **17127** (verified free + not in any Windows excluded range on this machine).

**Changes applied:**
- Architecture **AD-2**: `wmi` crate → `ureq` (sync HTTP). Bundled binary is `LibreHardwareMonitor.exe` v0.9.6, not `OpenHardwareMonitor.exe`.
- Architecture **AD-7**: WMI namespace probe → HTTP `/data.json` probe with JSON-signature discrimination.
- Architecture **AD-8**: `OhmSupervisor` writes `runWebServerMenuItem=true` and lowercase `listenerPort` into `LibreHardwareMonitor.exe.config` before launching.
- **Story 3.6**: rewrite from WMI/WQL to `ureq` GET + `serde_json` parse of `/data.json` (test fixture: saved `lhm_data.json`).
- **Story 6.4**: rewrite probe to HTTP, add port-write-to-LHM-config step, add port-fallback test cases.
- **Story 7.3**: tier probe becomes HTTP reachability.
- **Story 1.5**: config gains `[ohm] http_port = 17127` + `[ohm] enabled = false`.
- **Story 0.1**: workspace deps swap `wmi = 0.18.4` for `ureq = 2.12` + `serde_json = 1`.
- **T-10**: renamed from "WMI namespace probe timeout" to "LHM HTTP probe timeout" (still 500ms).
- **T-45 (new)**: LHM HTTP port = 17127 default, with fallback chain 17128..17137 if occupied.
- **Resources**: LHM v0.9.6 `LibreHardwareMonitor.zip` (the .NET 10 build) downloaded + SHA-256-pinned (fe216a48...1ba22) + MPL-2.0 LICENSE fetched.

**Why this matters:** v0.9.4 (the last WMI build, Nov 2024) predates AMD Ryzen AI 300-series sensor support — unacceptable for this machine's CPU. v0.9.6 + HTTP is both future-proof and correct for the project's reference hardware.

---

## 7. Provisioning summary — all actions complete (2026-07-08)

| # | Action | Owner | Status |
|---|---|---|---|
| 1 | Bump Rust to 1.95+ | User | ✅ Done (went to 1.96.1) |
| 2 | Add `llvm-tools` rustup component | User | ✅ Done |
| 3 | Pick LHM version + integration mode | User | ✅ Done (Option B: v0.9.6 + HTTP, port 17127) |
| 4 | Download project-local tooling + LHM | Swarm | ✅ Done (cargo subcommands, CI tools, sqlite, LHM v0.9.6) |
| 5 | Write `scripts/env.ps1` + `verify-dev-env.ps1` + `fetch_ohm.ps1` | Swarm | ✅ Done (Story 0.7 deliverable) |
| 6 | Full verification: 15/15 green | Swarm | ✅ Done |

**The dev environment is ready for Story 0.1 (workspace skeleton) to start.** Run `.\scripts\env.ps1` in any new PowerShell session to activate, then proceed with the critical path per `docs/backlog/regression-harness.md` §4.
