//! `sidebar-platform` — Win32 platform layer — window, AppBar, DWM, DPI, OhmSupervisor, single-instance guard (Epic 6 + Story 13.3).
//!
//! ## Module map
//! - [`dwm`] — `DwmSetWindowAttribute` wrappers (peek exclusion, capture cloak) — Story 6.1.
//! - [`window`] — `SetWindowPos` HWND_TOPMOST + `ViewportPrefs` (consumed by Epic 8) — Story 6.1.
//! - [`appbar`] — `SHAppBarMessage` register/unregister (ABM_NEW/QUERYPOS/SETPOS/REMOVE) — Story 6.2.
//! - [`dpi`] — `SetProcessDpiAwarenessContext` (PER_MONITOR_AWARE_V2) + `GetDpiForWindow` — Story 6.3.
//! - [`single_instance`] — `CreateMutexW` named-mutex guard (prevents double-instance) — Story 13.3.
//!
//! ## SAFETY discipline (guardrails.md G2 / F11)
//!
//! Every `unsafe` block below carries a `// SAFETY:` comment explaining why
//! the invariants hold (HWND validity, attribute-pointer lifetime, struct
//! sizing). The workspace lint `clippy::undocumented_unsafe_blocks = "deny"`
//! enforces this — the orchestrator's HITL review (G19) is the second check.
//!
//! ## `windows` crate features (per Cargo.toml)
//!
//! `Win32_Foundation`, `Win32_Graphics_Dwm`, `Win32_UI_WindowsAndMessaging`,
//! `Win32_UI_Shell`, `Win32_UI_HiDpi`, `Win32_System_Threading`.

pub mod appbar;
pub mod dpi;
pub mod dwm;
pub mod hotkey;
pub mod monitors;
pub mod net_info;
pub mod ohm_supervisor;
pub mod single_instance;
pub mod startup;
pub mod theme_bridge;
pub mod window;
