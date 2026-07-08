# Story 0.1 — RED phase test files

This directory holds workspace-level integration tests that verify the
**shape** of the Cargo workspace. They are compiled as part of the workspace's
implicit root test target once Story 0.1's GREEN phase lands the root
`Cargo.toml`.

## Test files (RED — failing until GREEN phase)

| File | Contract | Cited |
|---|---|---|
| `workspace_shape.rs` | Exactly 11 packages with the expected names; exactly 1 binary crate (`sidebar-app`) | Story 0.1 Happy Path #1 + #2; architecture.md §4; G17 |
| `msrv_contract.rs` | No crate declares `rust-version` above workspace MSRV 1.95 | Story 0.1 Boundary #1; T-44; architecture.md AD-3 |
| `no_duplicate_deps.rs` | No dependency resolves to multiple major versions | Story 0.1 Boundary #2; G3/G18; F6 |
| `every_crate_compiles.rs` | Every workspace member's source file exists on disk | Story 0.1 Boundary #3 |

## Required dev-dependencies

The GREEN-phase root `Cargo.toml` must declare these as workspace-level
dev-dependencies (the tests live at workspace root, not inside any crate):

```toml
[workspace.dependencies]
cargo_metadata = "0.19"   # MIT — workspace introspection
semver = "1"              # MIT/Apache-2.0 — version parsing
```

These are both MIT/Apache-2.0 licensed (T-32-allowed) and add ~50 KB to the
dev build. They are NOT runtime dependencies — sidebar.exe will not link them.

## Per-crate smoke tests (inside each crate's src/lib.rs)

In addition to these workspace-level tests, each crate's `lib.rs` (or
`main.rs` for the binary) MUST expose:

```rust
/// Story 0.1 smoke marker — proves the crate is reachable via cargo test.
pub fn crate_present() -> bool { true }

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert!(super::crate_present(), "crate_present must return true");
    }
}
```

This satisfies Story 0.1 Happy Path test #1 ("Each crate's lib.rs exposes
pub fn crate_present() -> bool { true }") and avoids the empty-stub anti-
pattern (G17 forbids generated stub files).
