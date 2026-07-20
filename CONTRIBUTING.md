# Contributing to sidebar (win11-diagnostics)

Thanks for your interest in contributing! This project uses a spec-driven
development (SDD) workflow with strict TDD. Before contributing, please read:

## Required reading

1. [`docs/PRD.md`](docs/PRD.md) — what we're building and why.
2. [`docs/architecture.md`](docs/architecture.md) — how it's structured.
3. [`docs/backlog/README.md`](docs/backlog/README.md) — the audited backlog.
4. [`docs/backlog/guardrails.md`](docs/backlog/guardrails.md) — the hard
   rules (G1..G27) every contribution must follow.
5. [`docs/architecture.md` §14](docs/architecture.md#14-current-implementation-state-and-known-gaps)
   — current integration gaps; do not claim the status-pill, live bandwidth
   view, or OHM child monitor is wired until Story 12.8 lands.

## Getting started

```pwsh
git clone https://github.com/ravibaskaran/win11-diagnostics.git
cd win11-diagnostics

# Verify your machine has all prerequisites.
.\scripts\verify-dev-env.ps1

# Activate the dev env in your session (PATH only — no system mutation).
. .\scripts\env.ps1

# Run the workspace checks locally before opening a PR. Epic 10.1 owns the
# full regression/coverage gate; release publishing is still Epic 9 work.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo deny check bans licenses advisories sources
cargo audit
```

The current raw `cargo audit` output is limited to the documented transitive
`quick-xml`/`ttf-parser` advisory exceptions. Keep those ignores explicit and
time-bounded in `deny.toml`; do not broaden the exception scope silently.

## Workflow

1. **Pick a story** from [`docs/backlog/PROGRESS.md`](docs/backlog/PROGRESS.md).
   Current ready entries include 10.1 and 11.1; Epic 9 is blocked by 6.5 and
   10.2 waits for 10.1. The parity/closure work is Epic 12.
2. **Branch:** `story-X.Y-<short-slug>`.
3. **RED commit:** write failing tests first. Commit
   `test(story-X.Y): RED — <fixture>`. (G1 — no exceptions.)
4. **GREEN commit:** the implementation. Commit `feat(story-X.Y): <one-line>`.
5. **Run the full regression matrix** (commands above). Every prior story's
   tests must still pass (G25).
6. **Open a PR.** Apply `requires-hitl-*` labels per guardrails.md G19.
7. **HITL-gated stories** (G11 list) need explicit maintainer approval
   before merge; do not self-merge.
8. **Squash-merge** to `main`. Update PROGRESS.md.

## Code style

- `cargo fmt --all -- --check` MUST pass.
- `cargo clippy --workspace --all-targets -- -D warnings` MUST pass.
- Every `unsafe` block has a `// SAFETY:` comment justifying the invariant.
- Every public item has a doc comment (workspace `missing_docs = "warn"`).
- No `dbg!()` or `todo!()` in committed code (workspace lints deny both).

## Test discipline

- Tests cite a `T-*` threshold and an `F-*` fixture where applicable
  (see `docs/backlog/nfr-thresholds.md` and `tdd-fixtures.md`).
- Unit tests live inside each crate's `src/lib.rs` (`#[cfg(test)] mod tests`).
- Integration tests live under `crates/sidebar-app/tests/`.
- `cargo test --workspace --all-targets` runs everything in one pass.

## Licensing

By contributing, you agree that your contributions are licensed under the
project's MIT license (see [`LICENSE`](LICENSE)). The bundled
`LibreHardwareMonitor.exe` remains MPL-2.0 and is upstream software.
