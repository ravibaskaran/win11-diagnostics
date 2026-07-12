# sidebar — Dev Environment Scripts

Three PowerShell 7 scripts that activate, verify, and provision the
relocatable dev environment. All three are session-scoped — they do NOT
mutate persistent state (no `$PROFILE`, no registry, no `[Environment]
::SetEnvironmentVariable`, no global config files).

## Usage

```pwsh
# 1. Activate the dev env in your current PowerShell session (PATH only).
. .\scripts\env.ps1

# 2. Verify all prerequisites + project-local tools are installed.
.\scripts\verify-dev-env.ps1
# (or with machine-readable JSON for CI)
.\scripts\verify-dev-env.ps1 -Json

# 3. (Re)download the bundled LibreHardwareMonitor binary (idempotent).
.\scripts\fetch_ohm.ps1
# Validate the local executable/pin without network access.
.\scripts\fetch_ohm.ps1 -CheckOnly
```

## Files

| Script | Purpose |
|---|---|
| `env.ps1` | Prepends `tools/{cargo-bin,ci,sqlite}` to `$env:PATH` for the current session. |
| `verify-dev-env.ps1` | 16-point verification gate; exits non-zero on any failure. CI pre-flight. |
| `fetch_ohm.ps1` | Downloads pinned LHM release, verifies the committed SHA-256 pin, extracts to `resources/`, and supports offline `-CheckOnly` validation. |

## Reference

- `docs/dev-env.md` — full setup guide + machine inventory.
- `docs/backlog/nfr-thresholds.md` T-44 — dev-env prerequisite contract.
- Story 0.7 — original spec.
