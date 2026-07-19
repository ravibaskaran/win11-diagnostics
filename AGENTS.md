# AGENTS.md — sidebar repository agent instructions

This file is read automatically by Codex (and other agents) at session start.
It tells the agent how to activate the dev environment before running any
build, test, or verification command.

## Mandatory: activate the dev environment first

**Before running `cargo`, `rustc`, `actionlint`, `gh`, or any verification
script in this repo, activate BOTH scripts in this exact order:**

```pwsh
. .\scripts\env.ps1        # Prepends tools/{cargo-bin,ci,sqlite} to $env:PATH
. .\scripts\codex-env.ps1  # Rust + MSVC x64 + TEMP redirect (Codex-specific)
```

### Why this is non-negotiable

Codex's managed PowerShell host runs under
`shell_environment_policy.inherit = "none"` (in the user's `config.toml`),
which strips these variables from the spawned shell:

- `USERPROFILE`, `HOME` — needed by cargo, git, and many crates
- `RUSTUP_HOME`, `CARGO_HOME` — needed by rustup/cargo to locate the toolchain
- `INCLUDE`, `LIB`, `LIBPATH` — needed by `link.exe` to find MSVC + Windows SDK
- The MSVC `HostX64\x64` bin dir on `PATH` — needed to invoke `link.exe`

Without activation:
- `cargo --version` fails ("cargo not found")
- `rustc --version` fails ("rustc not found")
- `cargo build` for `x86_64-pc-windows-msvc` fails at link time (`linker 'link.exe' not found`)
- `cargo test` fails with the same link error

`scripts/codex-env.ps1` resolves the user profile via
`[Environment]::GetFolderPath('UserProfile')` (registry-backed, works even
when `$env:USERPROFILE` is null under the sandbox), replays the MSVC env
block from `VsDevCmd.bat -arch=x64`, and redirects `TEMP`/`TMP`/
`CARGO_TARGET_DIR` into the workspace so builds are always sandbox-writable.

### Activation is session-scoped

Both scripts mutate only the current process's environment. Nothing is
written to:

- the user's `$PROFILE`
- the Windows Registry
- `[Environment]::SetEnvironmentVariable(..., User)`
- any global config file (`cargo config`, `git config --global`, `rustup`)

Re-running them in the same session is safe (idempotent).

## Verification commands

After activation, the full verification chain (in order):

```pwsh
.\scripts\verify-dev-env.ps1 -NoActivate     # 16-point tool audit
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --locked
cargo deny check
cargo test --workspace --all-targets --quiet
actionlint (Get-ChildItem .github/workflows/*.yml).FullName   # glob must be expanded
.\verify\smoke-checklist.ps1 -SkipIgnored
gh auth status
```

Note: `actionlint .github/workflows/*.yml` does NOT work on Windows — the
shell does not expand the glob, and `actionlint` receives the literal
`*.yml` string. Use `Get-ChildItem` to expand, as shown above.

## Known flaky test

`gui::tests::hotkey_thread_wakes_on_wm_quit_and_joins_cleanly` in
`crates/sidebar-app/src/gui/mod.rs:1757` is flaky. It fails ~1/200 runs
under full workspace load with `PostThreadMessageW` returning
`ERROR_INVALID_THREAD_IDENTIFIER (0x800705A4)`. This is a race in the
test (TID sent before the helper thread calls `GetMessageW`), NOT a
sandbox issue. If it fails, re-run in isolation:

```pwsh
cargo test -p sidebar-app --lib gui::tests::hotkey_thread_wakes_on_wm_quit_and_joins_cleanly
```

If it passes in isolation, the workspace run was the flake. Do not treat
it as a regression unless it fails in isolation too.

## GitHub CLI auth

`gh auth status` works from this host via the Windows Credential Manager
keyring (account `ravibaskaran`, scopes: `gist`, `read:org`, `repo`,
`workflow`). Inside the Codex sandbox, the keyring may not propagate. If
`gh auth status` reports not logged in, Codex requires one of:

1. `GH_TOKEN` env var set to a PAT with `repo` + `workflow` scopes, OR
2. `GH_CONFIG_DIR` pointed at a directory containing a valid `hosts.yml`, OR
3. Pre-approved keyring auth propagated into the sandbox.

Never run `gh auth login` from inside a Codex session — it would persist
credentials outside the user's approved auth path.

## Constraints

- Do not modify `config.toml`, source code, tests, or any tracked file
  unless explicitly asked.
- Do not commit generated files (`target/`, `tmp/` — both gitignored).
- Do not request, print, persist, or copy GitHub credentials.
- If a command is blocked by sandbox permissions, report it explicitly —
  do not claim success.
- Distinguish "tool missing" from "tool installed but inaccessible".
