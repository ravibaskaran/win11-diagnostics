# win11-diagnostics

> *Glanceable system health, calmly.* A Windows 11 desktop sidebar overlay showing live hardware telemetry — CPU/GPU temps, clocks, utilization, fan speeds, voltages, power draw; memory and VRAM; per-drive storage and throughput; per-network-adapter throughput; per-process top-N resource consumers; battery; and **monthly bandwidth consumption tracking per network interface**.

A ground-up Rust clone of the user-facing experience of [SidebarDiagnostics](https://github.com/ArcadeRenegade/SidebarDiagnostics) (C#/.NET/WPF + LibreHardwareMonitor), rebuilt natively for Windows 11 with a strict lightweight mandate and a two-tier sensor model that degrades gracefully when elevated privileges are unavailable.

**Status:** Epic 0–8 implementation is present in the workspace (48/68 tracked
stories merged, including integration wiring). Workspace tests currently pass
528/528 with 11 ignored hardware/UAC/capture smokes; Epic 10.1 verification is
ready to start. Raw `cargo audit` reports only the documented transitive
`quick-xml`/`ttf-parser` advisory exceptions (see `deny.toml`). Known launch/UI wiring gaps are tracked in `docs/PRD.md` §12,
`docs/architecture.md` §14, and Story 12.8.

## Honest framing

This product is **Rust-native except for CPU package temperature and a small set of low-level hardware sensors**, which require a bundled [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor) (LHM) subprocess. We do not claim "pure Rust" anywhere. The LHM bundling is a deliberate, research-validated design decision (see `docs/architecture.md` AD-2), not a fallback.

## What's in this repo

```
docs/
├── PRD.md                 Product requirements (NFRs, telemetry matrix, two-tier model, risks)
├── architecture.md        Architecture decisions (AD-1..AD-14), data flow, crate layout, trait sketches
├── grants.md              Open-source credits/grants strategy + zero-cost distribution analysis
├── dev-env.md             Development environment setup guide + machine inventory
└── backlog/
    ├── README.md             Backlog index (4-pass audit complete)
├── epics-and-stories.md  13 Epics / 68 Stories (including parity/closure), with wiring metadata
    ├── guardrails.md         27 cross-cutting rules (G1..G27) + HITL action matrix
    ├── nfr-thresholds.md     45 NFR thresholds (T-1..T-45) — single source of truth
    ├── tdd-fixtures.md       14 test-fixture patterns (F-1..F-14)
    ├── regression-harness.md Test layer model (L0..L4) + 8-point Definition of Done
    └── PROGRESS.md           Auto-updated story tracker (read by the agentic swarm)
scripts/
├── env.ps1                Session-scoped dev-env activation (no system mutation)
├── verify-dev-env.ps1     16-point verification gate (CI pre-flight)
└── fetch_ohm.ps1          Idempotent LHM binary download + SHA-256 verify
resources/
├── ohm.sha256             SHA-256 pin for the bundled LHM binary
├── LibreHardwareMonitor.exe          (provisioned locally; release packaging pending)
└── LibreHardwareMonitor.LICENSE.txt  MPL-2.0 (redistribution terms)
```

## Quick start

```pwsh
git clone https://github.com/ravibaskaran/win11-diagnostics.git
cd win11-diagnostics

# Verify your machine has the system prerequisites (Rust >=1.95, MSVC Build Tools,
# llvm-tools rustup component, PowerShell 7+, Git). 15-16 checks.
.\scripts\verify-dev-env.ps1

# Provision the project-local tooling (cargo subcommands, CI tools, LHM binary).
# Follows docs/dev-env.md §3.2 + scripts/fetch_ohm.ps1.

# Activate the dev env in your current PowerShell session (PATH only — no system mutation):
. .\scripts\env.ps1
```

See [`docs/dev-env.md`](docs/dev-env.md) for the full setup guide, including the minimal system prerequisites (Rust, MSVC Build Tools, PowerShell 7, Git) and the project-local relocatable tooling under `tools/`.

## Distribution

Zero-cost-first: SignPath Foundation (free OSS code signing) + GitHub Releases + winget + optional Microsoft Store (free Partner Center onboarding). Total annual cost: $0. See `docs/architecture.md` §11, `docs/grants.md`, and [`signpath/code-signing-policy.md`](signpath/code-signing-policy.md) for the full analysis + signing policy.

## License

The host workspace is **MIT** (`LICENSE`). The bundled
`LibreHardwareMonitor.exe` and its license remain MPL-2.0.

## Documentation

- [PRD](docs/PRD.md) — what we're building and why
- [Architecture](docs/architecture.md) — how it's structured
- [Dev Environment](docs/dev-env.md) — how to set up a contributor machine
- [Backlog](docs/backlog/README.md) — the audited story breakdown for the agentic swarm
- [Grants Strategy](docs/grants.md) — open-source credits + zero-cost distribution
