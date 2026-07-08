# Contributing to sidebar (win11-diagnostics)

Thanks for your interest in contributing! This project uses a spec-driven
development (SDD) workflow with strict TDD. Before contributing, please read:

## Required reading

1. [`docs/PRD.md`](docs/PRD.md) — what we're building and why.
2. [`docs/architecture.md`](docs/architecture.md) — how it's structured.
3. [`docs/dev-env.md`](docs/dev-env.md) — how to set up your machine.
4. [`docs/backlog/README.md`](docs/backlog/README.md) — the audited backlog.
5. [`docs/backlog/guardrails.md`](docs/backlog/guardrails.md) — the hard
   rules (G1..G27) every contribution must follow.

## Getting started

```pwsh
git clone https://github.com/ravibaskaran/win11-diagnostics.git
cd win11-diagnostics

# Verify your machine has all prerequisites.
.\scripts\verify-dev-env.ps1

# Activate the dev env in your session (PATH only — no system mutation).
. .\scripts\env.ps1

# Run the full regression matrix locally before opening a PR.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo deny --workspace check
cargo audit
```

## Workflow

1. **Pick a story** from [`docs/backlog/PROGRESS.md`](docs/backlog/PROGRESS.md).
   The ready set is the stories whose `Depends-On` are all merged.
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
project's license (MPL-2.0; see [`LICENSE`](LICENSE)).
