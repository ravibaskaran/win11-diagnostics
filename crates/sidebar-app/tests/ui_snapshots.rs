//! Test Layer: L2 — UI snapshot layer smoke marker + Story 11.3 harness.
//!
//! Story 11.3 ships the bootstrap snapshot that breaks the 8.1 <-> 11.3
//! cycle. Each snapshot module lives under `snapshots/` and is include!'d
//! here so `cargo test --test ui_snapshots` runs the full L2 layer.

mod layer_smoke {
    include!("snapshots/layer_smoke.rs");
}

mod story_11_3_harness_bootstrap {
    include!("snapshots/story_11_3_harness_bootstrap.rs");
}
