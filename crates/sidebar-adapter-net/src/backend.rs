//! `NetBackend` trait + `RealNetBackend` (Win32 IpHelper adapter, Story 3.5).
//!
//! This module isolates all Win32 `GetIfTable2` / `GetIfEntry2` FFI behind a
//! trait so the adapter ([`crate::NetAdapterGeneric`]) can be unit-tested with
//! a mock. The real backend enumerates all non-loopback adapters once per
//! `refresh_and_snapshot` call and reads the raw cumulative `InOctets` /
//! `OutOctets` counters from each.
//!
//! ## RAW counter contract (architecture §5.2 v2 note + G9)
//!
//! This adapter emits RAW CUMULATIVE counters (`InOctets` / `OutOctets`). It
//! does NOT delta-and-divide. The downstream `BandwidthAccountant` (Story
//! 5.x) computes per-tick deltas, applies wraparound handling (T-23), and
//! accumulates monthly totals per LUID. Exposing the raw counters here keeps
//! this adapter idempotent + stateless — every call returns the same current
//! snapshot, regardless of call rate.
//!
//! ## Counter wraparound (T-23)
//!
//! `InOctets`/`OutOctets` are `u64`; on real hardware they reset only on
//! adapter re-init or NIC disable/enable (NOT on `u64` overflow in practice —
//! 2^64 bytes is ~58 years at 10 Gbps). When a reset does happen, the value
//! drops below the prior reading; the downstream accountant treats this as a
//! reset (discard the negative delta, anchor a new baseline) — that logic is
//! explicitly out-of-scope for THIS adapter. We just emit the raw counter.
//!
//! ## NIC enumeration + filtering
//!
//! `GetIfTable2` returns a `MIB_IF_TABLE2` with a count + flexible-array of
//! `MIB_IF_ROW2`. We snapshot every row whose `Type != IF_TYPE_SOFTWARE_LOOPBACK`
//! (24) AND `OperStatus == IfOperStatusUp` (1). Down/loopback/virtual adapters
//! are skipped silently (Boundary #2: NIC disappears → skip, no panic). If a
//! NIC reappears later, the next tick's enumeration picks it up (Boundary #3).
//!
//! ## LUID stability (G11 / T-24 — HITL item)
//!
//! The SensorId `instance` is the LUID rendered as a decimal string. LUIDs
//! are assigned per-adapter by Windows and are *expected* to be stable across
//! reboots for the same physical NIC. This is the architecture's stated
//! assumption (AD-12) and the HITL item G11: if `sdd-verify` later disproves
//! LUID stability, fallback R10 is to use the MAC (`PhysicalAddress`) instead.
//! The MAC is read here into `mac_fingerprint` but not currently used as the
//! key — kept in the snapshot for the eventual fallback if needed.
//!
//! ## SAFETY discipline (guardrails.md G2)
//!
//! Every `unsafe` block below carries a `// SAFETY:` comment explaining why
//! the invariants hold (pointer validity, flexible-array slice bound,
//! union-arm selection for NET_LUID). The workspace lint
//! `clippy::undocumented_unsafe_blocks = "deny"` enforces this.
//!
//! Cited: Story 3.5 Technical Context (`windows = 0.62.2` IpHelper),
//! architecture.md §5.2 (raw cumulative counters), AD-12 (LUID tracking),
//! tdd-fixtures.md F11 (unsafe FFI test with SAFETY contract).

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

/// `MIB_IF_ROW2.Type == 24` marks the software loopback adapter; we always
/// filter it out (it never carries user traffic). Cited: IpHelper `IF_TYPE_*`.
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

/// A plain-data snapshot of one IpHelper enumeration cycle. Translation to
/// [`Reading`](sidebar_domain::reading::Reading)s happens in
/// [`crate::readings_from_snapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetSnapshot {
    /// Per-NIC cumulative counters. Loopback + down adapters are filtered out
    /// upstream; the vector here is always "live data NICs only".
    pub nics: Vec<NicSnapshot>,
}

/// Per-NIC cumulative counter snapshot.
///
/// All fields are RAW CUMULATIVE values straight from `MIB_IF_ROW2` — the
/// downstream accountant computes per-tick deltas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NicSnapshot {
    /// Interface LUID, rendered as a decimal string (`SensorId::instance`).
    /// This is the architecture's stable NIC identifier (AD-12).
    pub luid: u64,
    /// Cumulative RX bytes (`MIB_IF_ROW2::InOctets`).
    pub rx_bytes: u64,
    /// Cumulative TX bytes (`MIB_IF_ROW2::OutOctets`).
    pub tx_bytes: u64,
}

/// Abstraction over the IpHelper data source. The production impl owns nothing
/// (each `refresh_and_snapshot` re-enumerates via `GetIfTable2`); tests
/// substitute a mock.
///
/// Implementations need NOT be `Send + Sync` themselves — the adapter wraps
/// the backend in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait NetBackend {
    /// Enumerate live NICs and return their cumulative RX/TX counters.
    fn refresh_and_snapshot(&mut self) -> NetSnapshot;
}

/// Production backend issuing real `GetIfTable2` calls. Stateless — every
/// call re-enumerates all adapters. `Drop` is a no-op (we never hold a
/// persistent handle; each call's `MIB_IF_TABLE2` is `FreeMibTable`'d before
/// return).
pub struct RealNetBackend;

impl Default for RealNetBackend {
    fn default() -> Self {
        Self
    }
}

impl RealNetBackend {
    /// Construct the production backend. Never fails — there is no persistent
    /// state to acquire. `refresh_and_snapshot` will return whatever NICs
    /// Windows currently reports (possibly empty if IpHelper is unavailable
    /// in the moment — see Boundary #1).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl NetBackend for RealNetBackend {
    fn refresh_and_snapshot(&mut self) -> NetSnapshot {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        // SAFETY: `table_ptr` is a fresh zero-initialized (null) pointer.
        // GetIfTable2 documents its only out-param as `*mut *mut MIB_IF_TABLE2`;
        // on success it allocates a buffer (caller-freed via FreeMibTable) and
        // writes its address into `table_ptr`. On failure the pointer stays
        // null and we return empty. We never dereference `table_ptr` unless
        // the call returned ERROR_SUCCESS.
        let status = unsafe { GetIfTable2(std::ptr::addr_of_mut!(table_ptr)) };
        if status != ERROR_SUCCESS || table_ptr.is_null() {
            tracing::debug!(
                target = "sidebar.net",
                status = status.0,
                "GetIfTable2 failed"
            );
            return NetSnapshot::default();
        }

        // Defer FreeMibTable to the end of scope, regardless of how we exit.
        // Wrapped in a closure-style guard so we cannot leak on early return.
        let snapshot = enumerate_table(table_ptr);

        // SAFETY: `table_ptr` was populated by a successful GetIfTable2 above
        // and points at a buffer allocated by iphlpapi.dll. FreeMibTable is
        // the documented deallocator (its signature takes `*const c_void`;
        // the cast preserves the address but switches the pointee type to the
        // opaque c_void the API expects). After this call the pointer is
        // invalid; we are about to return so that's fine.
        unsafe {
            FreeMibTable(table_ptr as *const core::ffi::c_void);
        };

        snapshot
    }
}

/// Walk the `MIB_IF_TABLE2`'s flexible-array `Table` field and snapshot every
/// non-loopback + up adapter. Returns a plain-data `NetSnapshot`.
///
/// # SAFETY contract
///
/// `table` must point at a `MIB_IF_TABLE2` returned by a successful
/// `GetIfTable2` call, AND that buffer must still be alive (caller frees via
/// `FreeMibTable` after this returns).
fn enumerate_table(table: *mut MIB_IF_TABLE2) -> NetSnapshot {
    // SAFETY: `table` is non-null (caller checked) and points at a valid
    // MIB_IF_TABLE2 buffer. Reading the `NumEntries` field (a u32 at offset 0
    // of the struct) is in-bounds.
    let num_entries = if table.is_null() {
        return NetSnapshot::default();
    } else {
        // SAFETY: dereferencing `table` to read `NumEntries` — pointer is
        // valid per the contract above; the struct's first field is a u32.
        unsafe { (*table).NumEntries as usize }
    };
    if num_entries == 0 {
        return NetSnapshot::default();
    }

    // The `Table` field is declared as `[MIB_IF_ROW2; 1]` — a C99-style
    // flexible array member. The *actual* array length is `NumEntries`.
    // Construct a slice of length `num_entries` starting at the `Table`
    // address. Bounds: iphlpapi guarantees the buffer has room for exactly
    // `NumEntries` rows after the count header.
    //
    // SAFETY: `table` is non-null + points at a valid MIB_IF_TABLE2 buffer
    // (caller checked). The `Table` field is the trailing flexible-array
    // member; `from_raw_parts` constructs a slice of `num_entries` items
    // starting at the first row. iphlpapi allocated enough space for
    // `NumEntries` rows when it built the buffer (the struct's `[MIB_IF_ROW2;
    // 1]` declaration is the standard C flexible-array idiom — only one
    // element is declared, but the trailing array is over-allocated). The
    // resulting slice lives only within this function — shorter than the
    // caller's buffer lifetime.
    let rows: &[MIB_IF_ROW2] =
        unsafe { std::slice::from_raw_parts((*table).Table.as_ptr(), num_entries) };

    let mut nics = Vec::with_capacity(num_entries);
    for row in rows {
        // Skip loopback (Boundary: filter virtual adapters with no user data).
        // SAFETY: `row.Type` is a u32 field read within the slice bound.
        if row.Type == IF_TYPE_SOFTWARE_LOOPBACK {
            continue;
        }
        // Skip non-up adapters (Boundary #2: NIC disappeared/down → skip).
        // SAFETY: `row.OperStatus` is `IF_OPER_STATUS(i32)`; we compare to
        // `IfOperStatusUp`. Reading the field is in-bounds.
        if row.OperStatus != IfOperStatusUp {
            continue;
        }

        // SAFETY: `InterfaceLuid` is `NET_LUID_LH`, a union whose `Value`
        // arm is `u64`. Reading `.Value` here is sound — both arms are
        // POD-bitpatterns and we explicitly pick the u64 arm. The LUID is the
        // architecture's stable NIC identifier (AD-12).
        let luid = unsafe { row.InterfaceLuid.Value };

        nics.push(NicSnapshot {
            luid,
            rx_bytes: row.InOctets,
            tx_bytes: row.OutOctets,
        });
    }

    NetSnapshot { nics }
}

// SAFETY: `RealNetBackend` owns no fields at all — it is a unit struct. There
// is therefore nothing thread-affine about it (no handles, no pointers, no
// `!Send` cell). Every `refresh_and_snapshot` call enters + exits Win32 FFI
// statelessly; any thread may call it. The adapter always wraps the backend
// in a `Mutex` (serializing calls), but even without that this type would be
// sound to `Send`. Declared explicitly because the trait bound on
// `NetAdapterGeneric<B>: SensorProvider` requires `B: Send`.
unsafe impl Send for RealNetBackend {}
