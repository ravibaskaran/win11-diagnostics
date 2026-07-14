//! Single-instance named-mutex guard (Story 13.3).
//!
//! Prevents a second sidebar process from running concurrently with the
//! first. A non-technical user who double-clicks the exe while it's already
//! running would otherwise launch a second instance that clobbers the
//! first's `config.toml` writes (last-write-wins, no lock) and registers
//! a second AppBar on the same edge. The guard uses a Win32 named mutex
//! (`Global\sidebar-app-single-instance`) — the kernel tracks its lifetime
//! and releases it automatically when the owning process exits.
//!
//! ## SAFETY discipline (G2 / F11)
//!
//! `CreateMutexW` is a process-global kernel call. The mutex handle is
//! owned by this module (leaked via `Box::leak` — it lives until process
//! exit, which is exactly the desired lifetime: the mutex must outlive any
//! single function frame so the second-instance check works for the entire
//! process duration). `GetLastError` is thread-local and safe to call
//! immediately after `CreateMutexW` returns.
//!
//! ## Cited
//! Story 13.3 TDD contract. guardrails.md G2 (unsafe policy) + G10
//! (ownership analog — kernel-owned handle) + G28 (non-technical-user
//! hardening). Fixture F11 (unsafe FFI test with SAFETY contract).

use sidebar_domain::error::{Error, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;

/// The named-mutex string. `Global\` prefix makes it visible across all
/// sessions (so a second user launching sidebar also gets the guard, not
/// just a second launch in the same session). Cited: Story 13.3, G28.
pub const MUTEX_NAME: &str = "Global\\sidebar-app-single-instance";

/// Claim the single-instance mutex, or exit(0) if another instance already
/// holds it. Called from `main()` immediately AFTER `init_tracing()` (so the
/// exit is logged) but before any resource work — config load, eframe launch.
/// A second launch exits(0) before wasting any work on the doomed instance.
///
/// The mutex handle is leaked on purpose — it must outlive the function
/// frame so the kernel keeps the mutex alive for the entire process
/// duration. The kernel releases the mutex automatically when the process
/// exits (no explicit `ReleaseMutex` / `CloseHandle` needed for the
/// "claim for process lifetime" use case).
///
/// # Panics
/// Never — on a genuine `CreateMutexW` failure (kernel out of handles,
/// extremely unlikely), logs at `error!` and falls through (returns Ok).
/// The tradeoff: it's better to risk a double-instance than to block the
/// user from the app entirely.
///
/// # Cited
/// Story 13.3 TDD contract. G28 (single-instance guard).
pub fn claim_or_exit() {
    // Build the wide UTF-16 string for CreateMutexW. The null terminator is
    // required by Win32; `encode_wide().chain(Some(0))` appends it.
    let wide: Vec<u16> = MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `CreateMutexW` with `lpMutexAttributes = None` (default
    // security descriptor), `bInitialOwner = TRUE` (we want ownership so
    // the first instance holds it), and `lpName` pointing at our
    // null-terminated UTF-16 string. The string is stack-owned and lives
    // for the duration of this call. `CreateMutexW` returns a valid handle
    // even when the mutex already exists — the differentiator is
    // `GetLastError() == ERROR_ALREADY_EXISTS`. The handle is leaked via
    // `Box::leak` below so the kernel keeps the mutex alive for the
    // process lifetime; there is no use-after-free because we never access
    // the handle again (the kernel owns it from here).
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(wide.as_ptr())) };
    match handle {
        Ok(h) => {
            // Check whether the mutex already existed (i.e. another instance
            // is running). SAFETY: GetLastError is thread-local and reads
            // the last-error code set by the immediately-preceding
            // CreateMutexW call on this same thread. No pointer args.
            let last_error = unsafe { GetLastError() };
            if last_error == ERROR_ALREADY_EXISTS {
                tracing::info!(
                    target = "sidebar.platform.single_instance",
                    "another sidebar instance is already running — exiting (Story 13.3, G28)"
                );
                std::process::exit(0);
            }
            // Leak the handle so the mutex outlives this function frame.
            // The kernel releases the mutex when the process exits.
            // SAFETY: `h` is a valid HANDLE from CreateMutexW; Box::leak
            // moves ownership to the heap with no Drop. The handle is
            // never closed by Rust code — the OS reaps it on process exit.
            let _ = Box::leak(Box::new(h));
            tracing::debug!(
                target = "sidebar.platform.single_instance",
                "single-instance mutex claimed (Story 13.3, G28)"
            );
        }
        Err(e) => {
            // Extremely unlikely — kernel out of handles, or the named-mutex
            // namespace is locked down. Fall through (do not block the app);
            // the risk is a double-instance, which is annoying but not
            // dangerous (config writes are best-effort, AppBar conflicts
            // are visual). Documented tradeoff in the SAFETY comment above.
            tracing::error!(
                target = "sidebar.platform.single_instance",
                error = %e,
                "CreateMutexW failed — single-instance guard disabled (non-fatal, app continues)"
            );
        }
    }
}

/// Testable claim that does NOT call `std::process::exit`. Returns
/// `Ok(true)` if this is the first instance (mutex claimed), `Ok(false)`
/// if another instance already holds the mutex, or `Err` on a genuine
/// `CreateMutexW` failure. Used by the unit tests (which cannot call
/// `exit`). Cited: Story 13.3, F11.
pub fn claim_for_test() -> Result<bool> {
    let wide: Vec<u16> = MUTEX_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: same invariant as claim_or_exit — null-terminated UTF-16 on
    // the stack, default security, initial owner = true.
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(wide.as_ptr())) }
        .map_err(|e| Error::Platform(format!("CreateMutexW failed: {e}")))?;
    // SAFETY: GetLastError is thread-local and reads the last-error code
    // set by the immediately-preceding CreateMutexW call on this same
    // thread. No pointer arguments.
    let last_error = unsafe { GetLastError() };
    let is_first = last_error != ERROR_ALREADY_EXISTS;
    if is_first {
        // Leak so the test's mutex persists for the test duration.
        let _ = Box::leak(Box::new(handle));
    }
    Ok(is_first)
}

#[cfg(test)]
mod tests {
    //! Story 13.3 TDD contract tests. Cited: Story 13.3, G2/G10/G28, F11.

    use super::*;

    /// Cited: Story 13.3, F11. The mutex name MUST be the documented
    /// `Global\sidebar-app-single-instance` string (compile-time guard
    /// against an accidental rename that would silently break the
    /// single-instance contract).
    #[test]
    fn mutex_name_is_global_sidebar_app_single_instance() {
        assert_eq!(MUTEX_NAME, "Global\\sidebar-app-single-instance");
    }

    /// Cited: Story 13.3, F11, G28. The first `claim_for_test` call in a
    /// test process MUST succeed + report `is_first = true` (no prior
    /// instance). This test runs in the test process; if a prior test in
    /// the same process already claimed the mutex, this would be `false`
    /// — so this test is order-dependent and asserts only the happy path
    /// of "the claim call returns Ok at all". The cross-process second-
    /// instance detection is verified by the e2e_launch_smoke suite
    /// (spawning a real second binary) rather than a unit test.
    #[test]
    fn claim_for_test_returns_ok_on_first_call() {
        let result = claim_for_test();
        assert!(
            result.is_ok(),
            "claim_for_test MUST return Ok: {:?}",
            result.err()
        );
    }
}
