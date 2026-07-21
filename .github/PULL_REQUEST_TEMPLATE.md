# PR template

<!--
Thanks for the PR! Brief summary + verification checklist is all we need.
See CONTRIBUTING.md for the full dev workflow (TDD, guardrails, story wiring).
-->

## Summary

<!-- One paragraph: what changed + why. Reference the issue if applicable (`Closes #123`). -->

## Verification

<!--
Check the boxes that apply. If a box doesn't apply, strike it through rather
than checking it. The CI workflow runs fmt + clippy + test on every PR; the
boxes below cover what CI can't check.
-->

- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (note any delta from the prior count)
- [ ] New / changed behavior is covered by at least one runnable `#[test]`
- [ ] No new dependencies added without justification (G3 / T-32)
- [ ] If config schema changed: existing TOML configs still load (G28)
- [ ] If touching `unsafe`: every block has a `// SAFETY:` comment (G2)
- [ ] CHANGELOG.md updated (Added / Changed / Fixed / Removed as applicable)

## Notes for the reviewer

<!-- Anything non-obvious? Alternatives considered? Follow-up work? -->
