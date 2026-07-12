# Backlog — sidebar-v1 (Audited, Hardened, Pass 4 + Dev-Env Complete)

**Status:** Four-pass audit complete + dev environment inventoried on the primary machine. Pass 1: structural & cold-start. Pass 2: NFR/security/guardrail. Pass 3: correctness & completeness vs PRD/architecture. Pass 4: cumulative regression harness + story-to-story wiring. **Post-pass-4: dev-env inventory + tarpaulin→llvm-cov correction + Story 0.7 added**. Production-ready for swarm ingestion.

**Source of truth for the downstream Agentic Coding Swarm.** Derived from
`docs/PRD.md` (v2) and `docs/architecture.md` (v2). Every delivery story is
TDD-contract-bound, NFR-threshold-cited, layer-classified, dependency-wired,
and regression-gated. Epic 12 records post-Epic-8 parity and integration
closure work.

## Files

| File | Purpose | Read order |
|---|---|---|
| `README.md` | This index. | 1 |
| `guardrails.md` | Cross-cutting rules G1–G27. | 2 — ingest as system prompt |
| `nfr-thresholds.md` | Single source of truth for every numeric NFR boundary (T-1–T-45, incl. LHM HTTP port T-45, dev-env prerequisites T-44, coverage tool T-43, generalized reference HW T-31). | 3 — reference |
| `tdd-fixtures.md` | Setup/teardown patterns F1–F14. | 4 — reference |
| `regression-harness.md` | Test layer model (L0–L4), regression contract (8-point DoD), story wiring metadata schema, critical path, swarm loop. | 5 — reference |
| `PROGRESS.md` | Auto-updated story tracker (Story 11.4). Swarm reads this at task-startup. | — runtime |
| `epics-and-stories.md` | 13 Epics / 68 Stories (60 delivery rows including INT + 8 parity/closure) with hardened SDD schema + Wiring blocks. | 6 — per-story pickup |

**Companion (outside `backlog/`):** `docs/dev-env.md` — dev environment inventory + setup guide (authoritative for what's installed, what's missing, who downloads what).

## At-a-glance

- **Epics:** 13 (including Epic 12 parity/closure).
- **Stories:** 68 (60 current delivery rows including INT + 8 Epic 12 parity/closure stories).
- **Post-pass-4 deltas:** New `docs/dev-env.md` (machine inventory + relocatable folder plan). T-31 generalized (reference hardware no longer pinned to Intel i5-1240P; any modern 8+ core CPU with per-machine calibration). T-43 added (`cargo-llvm-cov`, replacing `cargo-tarpaulin` which is Linux-only). T-44 added (dev-env prerequisites as a threshold). Story 0.7 added (env.ps1 + verify-dev-env.ps1 + fetch_ohm.ps1). Story 3.2 annotated with local-NVIDIA-unavailable caveat. PRD §11 + architecture §12 added (Development Environment sections). Tarpaulin→llvm-cov fix applied to Story 11.2, Story 10.1.
- **Marquee features (unchanged):** per-NIC network throughput + monthly bandwidth tracking; NFR-8 human-readable formatting; zero-cost SignPath distribution.
- **Defining constraint (unchanged):** CPU package temp requires bundled OHM subprocess.
- **Pass 4 marquee addition:** Zero-regression cumulative testing — every story's PR proves Stories 1..N-1 still pass.

## Sequencing (dependency graph)

```
Epic 0 (Foundation) — 6 stories, all blocking.
   │
   ▼
Epic 1 (Domain) — pure types & logic.
   │
   ▼
Epic 2 (Sensor framework) — keystone.
   │
   ├──────────────┐
   ▼              ▼
Epic 3         Epic 4
(Adapters)    (Persistence)
   │              │
   │              ▼
   │           Epic 5 (Bandwidth Accountant)
   │              │
   ▼              ▼
Epic 6 (Platform) — incl. 6.5 (OHM acquire), 6.6 (hotkey/monitor/theme)
   │
   ▼
Epic 7 (Wiring) — incl. 7.4 (event channel), 7.5 (shutdown)
   │
   ▼
Epic 8 (GUI) — incl. 8.6 (theme), 8.7 (sparkline), 8.8 (alert), 8.9 (dnd), 8.10 (wizard)
   │
   ▼
Epic 9 (Release) — incl. 9.3 (auto-update, optional/v1.1)
   ║
Epic 10 (Verify)
   ║
Epic 11 (Regression Harness) — 11.1–11.4; 11.1/11.2 merge-eligible as early as after 0.2
```

## Swarm ingestion protocol (updated pass 4)

1. **Load `guardrails.md` into the system prompt** (all 27 rules + HITL matrix + regression gate).
2. **Load `nfr-thresholds.md`, `tdd-fixtures.md`, `regression-harness.md` as reference** — every test cites a `T-*` threshold, an `F-*` fixture, and declares a Layer.
3. **Read `PROGRESS.md`** (Story 11.4 auto-generates this on every merge) to identify the ready set.
4. **Pick the critical-path story** (lowest `Epic.Story` number in the ready set) from `epics-and-stories.md`. Respect each story's `Depends-On` / `Blocks` / `Next` wiring.
5. **Run the FULL regression matrix locally** before opening a PR (G25). If any prior test fails, surface a regression-blocker instead.
6. **Respect HITL gates** in `guardrails.md` G11/G19 — they cannot be auto-merged.
7. **Cross-reference** `docs/PRD.md` and `docs/architecture.md` for any ambiguity.

## Audit changelog

- **Pass 1 (structural):** 4 bootstrap stories (0.3–0.6). Exhaustive match replacing count test. `CycleStartDay` invariant. Adapter state-containers. SAFETY contracts for all `unsafe`. LHM HTTP fixture contract. SQLite test-vs-prod Connection. Exact `Clock` signature. Job-Object orphan prevention + `ShellExecuteW` error decoding. SignPath slug env vars. `tdd-fixtures.md` reference.
- **Pass 2 (security/NFR):** `nfr-thresholds.md` single-source-of-truth (T-1–T-33). HITL action-permission matrix (G19). Generation-loop bounds (G17). Network-egress allowlist (G16). Panic-safety rules (G15). Resource-bounds rules (G14). Cold-start idempotency (G13). Supply-chain automation (G18). Executable NFR-3/NFR-4 tests. Capped SQLite busy-retry + LHM HTTP timeout enforcement.
- **Pass 3 (correctness & completeness):** Cross-walked every PRD §3 UX row, §5 two-tier detail, §6 NFR, §8 risk, and architecture §6 threading/events/AD-* against the backlog. Added 11 stories (6.5, 6.6, 7.4, 7.5, 8.6, 8.7, 8.8, 8.9, 8.10, 9.3). Expanded 1.5/3.6/4.2/6.1/6.4. New T-34..T-39, F12/F13, G23/G24.
- **Pass 4 (regression harness & story wiring):** New Epic 11 (4 stories). New `regression-harness.md` (layer model L0–L4, 8-point DoD, wiring schema, critical path, swarm loop). Every story now carries a `Wiring:` block. New guardrails G25 (cumulative regression — full matrix per PR), G26 (coverage non-regression — T-42 floor), G27 (wiring discipline — `Depends-On`/`Blocks`/`Next` are the source of truth). New thresholds T-40 (per-layer budgets), T-41 (aggregate ≤750s), T-42 (coverage delta ≥0%). New fixture F14 (regression triple-layer test). HITL matrix expanded: +2 story gates (11.1, 11.4), +6 action gates (layer model, regression contract, snapshot acceptance, progress schema, coverage target).
- **Post-pass-4 (dev environment inventory):** Inventoried the primary dev machine (LAPTOP-PLN56DNU, AMD Ryzen AI 7 350, 24 GB, Win11 25H2, AMD Radeon 860M iGPU — no NVIDIA). Created `docs/dev-env.md` documenting installed/missing software, the relocatable `tools/` folder structure, the activation script (`scripts/env.ps1`), and who-downloads-what split (system prereqs = user; project tooling = swarm via cargo-binstall + scoop; signing = CI-only). New Story 0.7 (env.ps1 + verify-dev-env.ps1 + fetch_ohm.ps1). **Corrected `cargo-tarpaulin` → `cargo-llvm-cov`** everywhere (Story 11.2, Story 10.1) — tarpaulin is Linux-only. Generalized T-31 reference hardware (was Intel i5-1240P-specific; now any modern 8+ core CPU with per-machine calibration). New T-43 (coverage tool), T-44 (dev-env prerequisites). PRD §11 + architecture §12 added. Story 3.2 annotated: NVML integration tests are `#[ignore]`'d on this machine (no NVIDIA); AMD GPU coverage via Story 3.6 (OHM) only.
