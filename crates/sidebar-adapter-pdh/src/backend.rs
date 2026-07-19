//! `PdhBackend` trait + `RealPdhBackend` (Win32 PDH adapter, Story 3.4).
//!
//! This module isolates all Win32 PDH FFI behind a trait so the adapter
//! ([`crate::PdhAdapterGeneric`]) can be unit-tested with a mock. The real
//! backend owns an open PDH query + read/write counters and collects one
//! sample per `refresh_and_snapshot` call; rate counters require at least two
//! samples, so the first tick after construction returns zeros (PDH baseline
//! semantics — see Boundary notes in `lib.rs`).
//!
//! ## Why a trait (not `dyn` PDH)
//!
//! The PDH API is a bag of `unsafe extern "system"` free functions; there is
//! no trait to mock. Abstracting behind `PdhBackend` lets `mockall` generate
//! a `MockPdhBackend` that returns canned [`DiskSnapshot`]s — this is how the
//! Story 3.4 TDD contract's "mock PDH C: read 1 MB/s, write 2 MB/s" tests are
//! satisfied without touching real performance counters.
//!
//! ## SAFETY discipline (guardrails.md G2)
//!
//! Every `unsafe` block below carries a `// SAFETY:` comment explaining why
//! the invariants hold (handle validity, buffer sizing + alignment, union
//! field selection). The workspace lint
//! `clippy::undocumented_unsafe_blocks = "deny"` enforces this.
//!
//! Cited: Story 3.4 Technical Context (AD-3 + §7.2, `windows = 0.62.2` PDH),
//! tdd-fixtures.md F11 (unsafe FFI test with SAFETY contract).

use std::collections::HashMap;
use std::ptr::addr_of_mut;

use windows::core::w;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
};

/// Counter path for per-drive disk read throughput (English locale — see
/// `PdhAddEnglishCounterW` rationale below).
const READ_PATH: windows::core::PCWSTR = w!("\\PhysicalDisk(*)\\Disk Read Bytes/sec");
/// Counter path for per-drive disk write throughput.
const WRITE_PATH: windows::core::PCWSTR = w!("\\PhysicalDisk(*)\\Disk Write Bytes/sec");

/// The PDH status code returned when a formatted-array buffer is too small.
/// (High bit set — does not fit in a positive `i32`; keep as `u32`.) Not
/// re-exported by the `windows` crate as a named constant, so we pin it here.
const PDH_MORE_DATA: u32 = 0x8000_07D2;

/// A plain-data snapshot of one PDH collection cycle. Translation to
/// [`Reading`](sidebar_domain::reading::Reading)s happens in
/// [`crate::readings_from_snapshot`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PdhSnapshot {
    /// Per-drive throughput samples. The synthetic `_Total` instance is
    /// filtered out by the real backend so it never reaches here.
    pub drives: Vec<DiskSnapshot>,
}

/// Per-drive disk throughput snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskSnapshot {
    /// PDH instance name (e.g. `"0 C:"`, `"1 D:"`). Used as the SensorId
    /// instance.
    pub instance: String,
    /// Read throughput (bytes/sec).
    pub read_bytes_per_sec: f64,
    /// Write throughput (bytes/sec).
    pub write_bytes_per_sec: f64,
}

/// Abstraction over the PDH data source. The production impl owns a real PDH
/// query; tests substitute a mock.
///
/// Implementations need NOT be `Send + Sync` themselves — the adapter wraps
/// the backend in a `Mutex`, so the composite is `Send + Sync` regardless.
pub trait PdhBackend {
    /// Collect one PDH sample and return a plain-data snapshot. The very
    /// first call after construction may return all-zero values (PDH needs
    /// two samples to compute a rate); subsequent calls return real rates.
    fn refresh_and_snapshot(&mut self) -> PdhSnapshot;
}

/// Production backend owning a real PDH query + read/write counters.
///
/// The query is opened once at construction and held for the backend's
/// lifetime; `PdhCloseQuery` runs on `Drop`. Each `refresh_and_snapshot`
/// calls `PdhCollectQueryData` (advancing the internal sample window) then
/// reads the two formatted counter arrays.
pub struct RealPdhBackend {
    query: PDH_HQUERY,
    read_counter: PDH_HCOUNTER,
    write_counter: PDH_HCOUNTER,
    /// `true` once the first `PdhCollectQueryData` has established a
    /// baseline. The first `refresh_and_snapshot` returns zeros because PDH
    /// rate counters need a timestamp delta.
    primed: bool,
}

impl RealPdhBackend {
    /// Open a PDH query and add the read/write disk-throughput counters.
    ///
    /// Returns `None` if PDH is unavailable on this machine (e.g. the
    /// Performance Counters registry hive is disabled, or the counters are
    /// not installed). The adapter treats `None` as "emit empty readings"
    /// rather than panicking — this is the Boundary #1 contract.
    #[must_use]
    pub fn new() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: `query` is a fresh zero-initialized handle; PdhOpenQueryW
        // writes a valid handle into it on success. `None` for the data
        // source selects real-time data (the documented null behavior).
        // `addr_of_mut!` gives a raw `*mut PDH_HQUERY` without an implicit
        // borrow coercion (clippy::implicit.borrow.as.raw.pointer).
        let status = unsafe { PdhOpenQueryW(None, 0, addr_of_mut!(query)) };
        if status != ERROR_SUCCESS.0 {
            tracing::debug!(target = "sidebar.pdh", status, "PdhOpenQueryW failed");
            return None;
        }

        let mut read_counter = PDH_HCOUNTER::default();
        let mut write_counter = PDH_HCOUNTER::default();

        // SAFETY: `query` is a valid open handle (opened above). The counter
        // path constants are compile-time wide string literals. Each out-param
        // is a fresh zero-initialized handle. We use the *English* counter
        // path variant (`PdhAddEnglishCounterW`) so the path works regardless
        // of the system locale — the plain `PdhAddCounterW` would fail on
        // non-English Windows installs where the counter names are localized.
        let read_status =
            unsafe { PdhAddEnglishCounterW(query, READ_PATH, 0, addr_of_mut!(read_counter)) };
        if read_status != ERROR_SUCCESS.0 {
            tracing::debug!(
                target = "sidebar.pdh",
                status = read_status,
                "PdhAddEnglishCounterW(read) failed"
            );
            // SAFETY: `query` is a valid open handle; closing it releases
            // the associated kernel resources. No counters were added yet.
            unsafe { PdhCloseQuery(query) };
            return None;
        }

        // SAFETY: same as the read counter above.
        let write_status =
            unsafe { PdhAddEnglishCounterW(query, WRITE_PATH, 0, addr_of_mut!(write_counter)) };
        if write_status != ERROR_SUCCESS.0 {
            tracing::debug!(
                target = "sidebar.pdh",
                status = write_status,
                "PdhAddEnglishCounterW(write) failed"
            );
            // SAFETY: `query` is valid; closing releases read_counter + query.
            unsafe { PdhCloseQuery(query) };
            return None;
        }

        Some(Self {
            query,
            read_counter,
            write_counter,
            primed: false,
        })
    }

    /// Read the formatted counter array for `counter`, returning a list of
    /// (instance-name, bytes/sec) pairs. Returns an empty list on any PDH
    /// error or when no instances are present this tick.
    fn read_counter_array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        let mut buf_size: u32 = 0;
        let mut item_count: u32 = 0;

        // SAFETY: First-call sizing pattern documented by PDH: pass a null
        // `itembuffer` (None) with `buf_size = 0`; PDH returns PDH_MORE_DATA
        // and writes the required byte size into `buf_size`. `counter` is a
        // valid handle added during construction. `PDH_FMT_LARGE` selects
        // the `largeValue` (i64) union arm — bytes/sec fit comfortably in
        // i64 (max ~9 EB/sec).
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                addr_of_mut!(buf_size),
                addr_of_mut!(item_count),
                None,
            )
        };
        if status != PDH_MORE_DATA || buf_size == 0 {
            return Vec::new();
        }

        // Allocate a buffer of `PDH_FMT_COUNTERVALUE_ITEM_W` slots. We size
        // in items (ceil(byte_size / item_size)) rather than raw bytes so the
        // allocation is naturally 8-byte aligned (the item struct's
        // alignment), which PDH requires. The byte size PDH reports is at
        // most `item_count * size_of::<item>()`, so item-rounded allocation
        // always satisfies it. We zero-init (`vec![default; n]`) rather than
        // `with_capacity` + `set_len` so there are no uninitialized values
        // — PDH overwrites the first `filled_count` slots on success.
        let item_size = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let cap_items = (buf_size as usize).div_ceil(item_size.max(1));
        let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
            vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); cap_items];

        let mut filled_size = buf_size;
        let mut filled_count = item_count;
        // SAFETY: `buf` is `cap_items` zero-initialized items (>= the
        // byte_size PDH asked for, divided by item size), correctly aligned.
        // We pass its mutable pointer. PDH writes `filled_count` items into
        // it on success. The `szName` pointers inside each item point into
        // PDH-allocated memory *within this buffer*, valid until `buf`
        // drops at end of scope.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                addr_of_mut!(filled_size),
                addr_of_mut!(filled_count),
                Some(buf.as_mut_ptr()),
            )
        };
        if status != ERROR_SUCCESS.0 {
            return Vec::new();
        }

        let items = &buf[..(filled_count as usize).min(buf.len())];
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            // Skip instances whose data is invalid. CStatus == 0 means valid.
            if item.FmtValue.CStatus != 0 {
                continue;
            }
            // SAFETY: We requested PDH_FMT_LARGE, so the `largeValue` union
            // arm is the active one (PDH guarantees the union arm matches
            // the requested format). Reading `i64` here is sound. The value
            // is bytes/sec (non-negative throughput in practice).
            let value = unsafe { item.FmtValue.Anonymous.largeValue };
            // SAFETY: `szName` points into PDH-allocated memory within
            // `buf`, which is alive for the remainder of this loop. The
            // pointer is a valid NUL-terminated wide string (PDH contract).
            // `to_string()` copies into an owned `String` before `buf`
            // drops, so the borrow is released in time.
            let name = unsafe { item.szName.to_string() }.unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // i64 → f64: PDH bytes/sec is at most a few GB/s (< 2^34); the
            // cast is loss-free well within f64's 2^53 exact-integer range.
            // Allow explicitly to silence cast_precision_loss.
            #[allow(clippy::cast_precision_loss)]
            let value_f = value as f64;
            out.push((name, value_f));
        }
        out
    }
}

impl Default for RealPdhBackend {
    fn default() -> Self {
        // If PDH is unavailable we cannot construct a meaningful backend;
        // fall back to a fresh attempt (best-effort). Callers that care
        // about the availability signal should use `new()` directly.
        Self::new().unwrap_or_else(|| Self {
            query: PDH_HQUERY::default(),
            read_counter: PDH_HCOUNTER::default(),
            write_counter: PDH_HCOUNTER::default(),
            primed: false,
        })
    }
}

impl PdhBackend for RealPdhBackend {
    fn refresh_and_snapshot(&mut self) -> PdhSnapshot {
        // Cert v1.0 (backend audit I1) — if construction failed (PDH service
        // unavailable on locked-down / Server Core builds), `query` is null.
        // Skip the FFI call entirely; calling PdhCollectQueryData(NULL) is
        // UB on older Windows builds and a per-tick wasted syscall on modern
        // ones. Return empty so the adapter reports no disk telemetry rather
        // than risking an AV.
        if self.query.is_invalid() {
            return PdhSnapshot::default();
        }
        // SAFETY: `self.query` is a valid open handle (checked above).
        // CollectQueryData advances the internal timestamp window; the first
        // call establishes a baseline and the second computes the actual rate.
        let collect_status = unsafe { PdhCollectQueryData(self.query) };
        if collect_status != ERROR_SUCCESS.0 {
            return PdhSnapshot::default();
        }

        // First-ever collect: the rate counters have no prior sample to
        // diff against, so formatted values are zero/garbage. Record that
        // we've primed and return empty — real values arrive next tick.
        if !self.primed {
            self.primed = true;
            return PdhSnapshot::default();
        }

        let reads = Self::read_counter_array(self.read_counter);
        let writes = Self::read_counter_array(self.write_counter);

        // Merge read + write arrays by instance name. A drive may appear in
        // only one array if the other direction had no activity this tick;
        // default the missing direction to 0.0 (Boundary #2: zero-activity
        // drive → value 0.0, not omitted).
        let mut by_instance: HashMap<String, DiskSnapshot> =
            HashMap::with_capacity(reads.len().max(writes.len()));
        for (name, v) in &reads {
            by_instance
                .entry(name.clone())
                .or_insert_with(|| DiskSnapshot {
                    instance: name.clone(),
                    ..Default::default()
                })
                .read_bytes_per_sec = *v;
        }
        for (name, v) in &writes {
            by_instance
                .entry(name.clone())
                .or_insert_with(|| DiskSnapshot {
                    instance: name.clone(),
                    ..Default::default()
                })
                .write_bytes_per_sec = *v;
        }

        // Filter out the synthetic `_Total` instance PDH includes in wildcard
        // queries — it double-counts per-drive throughput.
        let drives = by_instance
            .into_iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("_Total"))
            .map(|(_, snap)| snap)
            .collect();

        PdhSnapshot { drives }
    }
}

// SAFETY: `RealPdhBackend` owns PDH handles (`PDH_HQUERY`, `PDH_HCOUNTER`)
// which are raw `*mut c_void` pointers — hence not auto-`Send`. PDH handles
// are opaque kernel objects (like file handles), NOT thread-affine: the PDH
// docs place no thread-affinity requirement on `PdhCollectQueryData` /
// `PdhGetFormattedCounterArrayW` — any thread holding the handle may call
// them. The backend is always accessed through a `Mutex` in the adapter
// (serializing all PDH calls), so there is no concurrent access. Sending the
// backend across threads is therefore sound. This impl lets
// `PdhAdapterGeneric<RealPdhBackend>: SensorProvider` (which requires
// `Send + Sync`, satisfied via `Mutex<RealPdhBackend>: Send + Sync`).
unsafe impl Send for RealPdhBackend {}

impl Drop for RealPdhBackend {
    fn drop(&mut self) {
        // Only close if the query was successfully opened (non-null handle).
        // A default-constructed fallback backend has a null query.
        if !self.query.is_invalid() {
            // SAFETY: `self.query` was opened by PdhOpenQueryW in `new()` and
            // is still valid (we never close it elsewhere). PdhCloseQuery
            // releases the query and all attached counters. After this call
            // the handles are invalid; we are dropping, so that's fine.
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}
