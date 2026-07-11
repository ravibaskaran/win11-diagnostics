//! Test Layer: L3 — bench layer smoke marker.
//!
//! Story 10.1 owns the performance benches. This target proves the L3 runner
//! is wired without introducing an NFR benchmark prematurely.

fn main() {
    assert_eq!(std::hint::black_box(1_u8.wrapping_add(1)), 2);
}
