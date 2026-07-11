# Regression Harness & Story Wiring — sidebar-v1 (Audit Pass 4)

**Purpose:** Guarantee zero regressions as stories accumulate. Every story's PR merges ONLY when (a) its own new tests pass AND (b) all prior stories' tests still pass. Stories are explicitly wired so the swarm knows the deterministic next pickup.

Cross-references: `guardrails.md` G25/G26/G27, `nfr-thresholds.md` T-40/T-41, `tdd-fixtures.md` F14.

---

## 1. Test Layer Model

Every test in the workspace declares exactly one layer via a `#[TestCase]`-style convention (module path). The harness runs them in strict order; a failure at layer N aborts layers N+1.

| Layer | Folder / suffix | What it tests | Runner | Budget (T-40) |
|---|---|---|---|---|
| **L0 — Unit** | `crates/*/src/**.rs` inline `#[cfg(test)]` | Pure functions, type-level proofs, mock-based trait behavior. NO OS calls. | `cargo test --lib` per crate | 60 s total |
| **L1 — Integration** | `crates/*/tests/*.rs` + `#[cfg(target_os="windows")]` | Real OS calls (LHM loopback HTTP, GetIfTable2, NVML, PDH, AppBar, capture affinity) on Windows CI. Hardware/UAC-dep tests `#[ignore]`. | `cargo test --test '*'` on `windows-latest` | 60 s total (excl. ignored) |
| **L2 — UI snapshot** | `crates/sidebar-app/tests/snapshots/*.rs` + `insta` files | egui_kittest-rendered panels vs committed snapshots. | `cargo test --test ui_snapshots` | 30 s total |
| **L3 — Bench (NFR)** | `benches/poll_cost.rs` + `benches/nfr_cold_start.rs` + `benches/nfr_rss.rs` | CPU%, cold-start, RSS thresholds. | `cargo bench --bench '*'` on Windows CI | 600 s total |
| **L4 — Smoke (manual)** | `verify/smoke-checklist.md` | Transparency, dock edges, multi-monitor, UAC flow. NOT in CI. | Human + scriptable subset | Human-time |

**Hard rule (G25):** A PR is NOT mergeable until L0+L1+L2+L3 all pass. L4 is gated at release time (Story 9.2).

---

## 2. The Regression Contract

For every story N, the merge gate is:

```
Story N "done" ≡
    (1) Story N's new tests (declared in its TDD contract) pass at their declared layer(s)
AND (2) ALL tests from Stories 1 .. N-1 still pass (full L0–L3 matrix)
AND (3) Coverage delta for the touched crate(s) is ≥ 0 (no coverage regression)
AND (4) `cargo clippy --workspace -- -D warnings` clean
AND (5) `cargo fmt --check` clean
AND (6) `cargo deny check bans licenses advisories sources` clean
AND (7) `cargo audit` clean (zero unmuted advisories)
AND (8) HITL gates per G11/G19 are cleared (labels removed)
```

This is enforced by CI (`ci.yml` Story 0.2 extended) — the workflow runs the FULL matrix on every PR, not just the touched crate. See `nfr-thresholds.md` T-40 for per-layer budgets; T-41 for the aggregate budget.

**Why "all prior tests" and not "all touched-crate tests":** Adapter crates depend on `sidebar-domain` and `sidebar-sensor`. A change to a pure-domain type can break an adapter's integration test silently if the adapter isn't re-run. The full matrix is the only honest check.

---

## 3. Story Wiring Metadata

Every story in `epics-and-stories.md` carries a wiring block (audit pass 4 addition):

```markdown
- **Wiring:**
  - **Layer:** unit | integration | ui | smoke | bench | (multiple allowed)
  - **Depends-On:** [list of story IDs that MUST merge before this one]
  - **Blocks:** [list of story IDs that cannot start until this one merges]
  - **Next:** [the single deterministic story to pick up after this one, IF the swarm is following the critical path]
  - **Parallel-With:** [stories that may run concurrently after dependencies are met]
  - **Definition of Done (DoD):** the 8-point contract above, specialized per story
```

The swarm uses `Next:` to compute the critical path. `Parallel-With` enables concurrent execution where the dependency graph allows (see §5).

---

## 4. Critical Path (Deterministic Swarm Pickup)

The critical path is the longest dependency chain. The swarm picks the next story as follows:

```
At any moment, the swarm's "ready set" = stories where ALL `Depends-On` entries have merged.

If the swarm is single-threaded (one story at a time):
    pick the ready-set story with the lowest (Epic, Story) number.

If the swarm is multi-threaded (concurrent agents):
    pick all ready-set stories whose `Parallel-With` allows co-execution,
    respecting G17 generation bounds (max 3 concurrent agents).
```

**Critical path (longest chain):**
```
0.1 → 0.2 → 0.3 → 0.4 → 0.5 → 0.6
  → 1.1 → 1.2 → 1.3 → 1.4 → 1.5 → 1.6
  → 2.1 → 2.2 → 2.3
  → 3.1 → 3.2 → 3.2b → 3.3 → 3.4 → 3.5 → 3.6      ‖  4.1 → 4.2 → 4.3
  → 5.1 → 5.2 → 5.3
  → 6.1 → 6.2 → 6.3 → 6.4 → 6.5 → 6.6
  → 7.1 → 7.2 → 7.3 → 7.4 → 7.5
  → 8.1 → 8.2 → 8.3 → 8.4 → 8.5 → 8.6 → 8.7 → 8.8 → 8.9 → 8.10
  → 9.1 → 9.2 → 9.3
  → 10.1 → 10.2
  → 11.1 → 11.2 → 11.3 → 11.4   (parallel-eligible from 0.2 onward)
```

Total critical-path length: 47 stories (parallelizable bursts reduce wall-clock).

---

## 5. Parallel Execution Plan

| Burst | Stories eligible to run concurrently | Reason |
|---|---|---|
| After Epic 2 | 3.1, 3.2, 3.3, 3.4, 3.5, 3.6 ‖ 4.1, 4.2, 4.3 | Each adapter is its own crate; persistence is independent. |
| After Epic 5 | 6.1, 6.2, 6.3, 6.6 | All platform-layer crates; 6.4 depends on 6.1–6.3. |
| After Epic 7 | 8.6, 8.7, 8.8, 8.9 | GUI panels independent after AppState (8.1) exists. |
| After Epic 0.2 | 11.1, 11.2, 11.3, 11.4 | Regression harness stories can develop alongside everything else once CI exists. |

---

## 6. Regression Harness Components

### 6.1 — Test runner matrix
- `cargo test --workspace --lib` (L0 unit) — 60s budget.
- `cargo test --workspace --tests` (L1 integration) — 60s budget, Windows-only.
- `cargo test --test ui_snapshots` (L2 UI) — 30s budget.
- `cargo bench --bench poll_cost` + `nfr_cold_start` + `nfr_rss` (L3 bench) — 600s budget.
- `verify/smoke-runner.ps1` (L4 scriptable smoke) — manual + release gate.

### 6.2 — Coverage tracking
- `cargo llvm-cov --workspace --lcov --output-path coverage/lcov.info` on every PR. (NOT `cargo tarpaulin` — Linux-only; see T-43.)
- Coverage delta computed vs `main` branch baseline.
- Coverage delta MUST be ≥ 0 for touched crates (G26).
- Coverage report uploaded as a CI artifact for HITL review.

### 6.3 — UI snapshot management
- `insta` crate (MIT/Apache-2.0, T-32-allowed) for L2 snapshots.
- New snapshots reviewed via `cargo insta review` (HITL gate per G19).
- Snapshots live in `crates/sidebar-app/tests/snapshots/`.
- A snapshot diff (intentional change) requires the PR description to call out "snapshot update" with a screenshot/justification.

### 6.4 — Story progress tracker
- `docs/backlog/PROGRESS.md` (auto-generated by Story 11.4) — a table of every story × its merge status.
- Updated by a CI job on every merge to `main`.
- The swarm reads `PROGRESS.md` at task-startup to know what's done.
- Format:
  ```markdown
  | Story | Status | Merged-At | PR | Layer Coverage |
  |---|---|---|---|---|
  | 0.1 | merged | 2026-07-08T10:23Z | #12 | L0:100% L1:n/a |
  | 0.2 | merged | 2026-07-08T11:45Z | #15 | L0:100% L1:n/a |
  | 0.3 | in-progress | — | #18 | — |
  | 0.4 | blocked-on-hitl | — | #19 | L0:100% |
  | 0.5 | pending | — | — | — |
  ```
- "blocked-on-hitl" status surfaces stories waiting on human review (G11/G19).

### 6.5 — Regression dashboard (CI artifact)
- On every PR, a `regression-report.md` is generated summarizing:
  - Which L0/L1/L2/L3 tests ran, passed, failed.
  - Coverage delta per crate.
  - Snapshot diffs (count + names).
  - HITL gates still blocking.
- This artifact is the orchestrator's verification view.

---

## 7. The Swarm Loop

The agentic coding swarm follows this exact loop per story:

```
1. Read PROGRESS.md → identify the ready set (Depends-On all merged).
2. Pick the critical-path story (lowest Epic.Story number in ready set).
3. Read the story's Wiring + TDD contract.
4. RED phase: write the failing tests at the declared Layer(s).
5. Commit RED ("test(story-X.Y): RED — <fixture>").
6. GREEN phase: write the implementation.
7. Commit GREEN ("feat(story-X.Y): <one-line>").
8. Run FULL regression matrix locally (L0–L3).
   - If any prior test fails → STOP, surface as regression blocker.
9. Open PR with `requires-hitl-*` labels per G19.
10. CI runs the same matrix; orchestrator reads regression-report.md.
11. HITL review per G11/G19.
12. Merge → PROGRESS.md auto-updates → swarm picks next from ready set.
```

The loop terminates when PROGRESS.md shows all 58 stories (54 existing + 4 from Epic 11) as `merged`.
