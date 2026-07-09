//! Story 7.1 — Provider Registry.
//!
//! Builds the `Vec<Arc<dyn SensorProvider>>` consumed by the poller (Story
//! 7.2). The registry wires every shipped adapter (sysinfo, battery, pdh, net,
//! nvml, ohm) and filters them through the v1 classifier (Story 2.3) using the
//! active runtime tier.
//!
//! ## Design
//!
//! The registry is split into two pieces so the tier-filter logic is testable
//! WITHOUT standing up real adapters (which would make the test
//! machine-dependent — sysinfo's `read_all` on a CI runner vs. the dev laptop
//! yields different reading sets):
//!
//! - [`build_registry`] — production entrypoint. Constructs every adapter,
//!   then delegates to [`filter_providers`] for tier filtering.
//! - [`filter_providers`] — pure helper that walks a `Vec<Arc<dyn
//!   SensorProvider>>`, collects each adapter's `&SensorDescriptor`, applies
//!   [`sidebar_sensor::classifier::classify_for_v1`], and retains only the
//!   providers whose descriptors pass. This is what the TDD contract
//!   exercises: tests construct lightweight stub providers (fixture F4 —
//!   controlled descriptors) and assert the filtering outcome.
//!
//! ## Hot tier switch (rebuild)
//!
//! `build_registry` is idempotent: calling it twice with the same `active_tier`
//! produces an equivalent registry (same adapter set in the same order). A hot
//! tier switch Basic → Full mid-session simply calls `build_registry(Full)` —
//! the Full-only `OhmAdapter` is now admitted and appended to the vec. Story
//! 7.3 wires this into the `Event::TierChanged` flow; this module only owns
//! the build.
//!
//! ## Cited
//!
//! - Story 7.1 TDD contract (Happy Path #1-#2, Boundary #1-#4)
//! - architecture.md §4 (crate layout: provider_registry.rs), §5.4 (AD-5
//!   classifier gate)
//! - nfr-thresholds.md T-1 (Lightweight mandate — enforced by classifier)
//! - tdd-fixtures.md F4 (mockall — drives tier-filtering tests), F6
//!   (idempotency)

use std::sync::Arc;

use sidebar_adapter_battery::BatteryAdapter;
use sidebar_adapter_net::NetAdapter;
use sidebar_adapter_nvml::NvmlAdapter;
use sidebar_adapter_ohm::OhmAdapter;
use sidebar_adapter_pdh::PdhAdapter;
use sidebar_adapter_sysinfo::SysinfoAdapter;
use sidebar_sensor::classifier::{classify_for_v1, ActiveTier};
use sidebar_sensor::descriptor::SensorDescriptor;
use sidebar_sensor::provider::SensorProvider;

/// Build the v1 provider registry for the given active tier.
///
/// Constructs every shipped adapter (sysinfo, battery, pdh, net, nvml, ohm),
/// then filters them through [`classify_for_v1`] using `active_tier`. The
/// order is deterministic: the construction order here is the iteration order
/// the poller sees. Hot tier switches re-call this function with a new
/// `active_tier`.
///
/// # Notes
///
/// - Adapters whose construction can fail at runtime (NVML on AMD, PDH if the
///   PDH service is down, OHM if LHM is not yet running) still construct —
///   they yield empty `read_all` results until their backing service comes up.
///   This keeps the registry shape stable across reboots (Story 7.3 doesn't
///   need to handle "adapter vanished" mid-session).
/// - `Heavy` / `Deferred` descriptors are rejected by the classifier with a
///   `tracing::warn!`. No adapter in v1 ships with those cost classes today,
///   but the filter is the second line of defense (Story 2.3 is the first).
#[must_use]
pub fn build_registry(active_tier: ActiveTier) -> Vec<Arc<dyn SensorProvider>> {
    // Order matters: it's the poller's iteration order. Group Basic adapters
    // first (they always run in both tiers), then Both (always runs), then
    // Full-only (ohm). The classifier decides which survive; this ordering
    // keeps the survivors' positions stable across tier switches.
    let providers: Vec<Arc<dyn SensorProvider>> = vec![
        Arc::new(SysinfoAdapter::new()),
        Arc::new(BatteryAdapter::new()),
        Arc::new(PdhAdapter::new()),
        Arc::new(NvmlAdapter::new()),
        Arc::new(NetAdapter::new()),
        Arc::new(OhmAdapter::new()),
    ];
    filter_providers(providers, active_tier)
}

/// Filter a list of providers by the v1 classifier (Story 2.3).
///
/// Walks `providers`, collects each adapter's `&SensorDescriptor`, applies
/// [`classify_for_v1`] with `active_tier`, and retains only the providers
/// whose descriptors pass. Returns a new `Vec` in the same order as the input.
///
/// This is the function the TDD contract exercises: tests construct stub
/// providers with controlled descriptors and assert the filtering outcome.
/// Keeping it separate from [`build_registry`] means the tier logic is
/// testable without constructing real adapters (which would make tests
/// machine-dependent).
fn filter_providers(
    providers: Vec<Arc<dyn SensorProvider>>,
    active_tier: ActiveTier,
) -> Vec<Arc<dyn SensorProvider>> {
    // Collect descriptors by reference. The descriptors outlive the call:
    // each adapter returns a `&'static SensorDescriptor` (declared `const` in
    // its lib.rs). For mock providers in tests the descriptor is leaked for
    // address stability — see the test helpers.
    let descriptors: Vec<&SensorDescriptor> = providers.iter().map(|p| p.descriptor()).collect();
    let accepted: Vec<&SensorDescriptor> = classify_for_v1(&descriptors, active_tier);

    // Capture accepted descriptor addresses as raw pointers so `accepted` no
    // longer borrows `providers`. This lets us move `providers` into the
    // filter below. The descriptors themselves are `&'static` (adapters) or
    // leaked (test stubs), so the raw pointers remain valid through the
    // filter call.
    let accepted_ptrs: Vec<*const SensorDescriptor> =
        accepted.iter().map(|d| std::ptr::from_ref(*d)).collect();

    // Walk the providers in order, retain each whose descriptor pointer is in
    // the accepted set. Pointer-equality is exactly what we want: each
    // adapter's descriptor is a unique `const`, and the classifier returned
    // references to the SAME descriptors we passed in (no cloning).
    providers
        .into_iter()
        .filter(|p| {
            let ptr = std::ptr::from_ref(p.descriptor());
            accepted_ptrs.iter().any(|&d| std::ptr::eq(d, ptr))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Story 7.1 TDD contract tests.
    //!
    //! These tests exercise `filter_providers` (the tier-filtering core) with
    //! lightweight in-process stub providers — fixture F4. Each stub returns
    //! a controlled `&SensorDescriptor` via `descriptor()`; the test asserts
    //! which stubs survive filtering. We don't use `mockall::automock` for
    //! this because `SensorProvider`'s automock is only emitted inside
    //! `sidebar-sensor`'s own test build (cfg(test)) — it isn't exported to
    //! downstream crates. A 5-line hand-rolled stub is simpler and dependency-
    //! free.
    //!
    //! Cited:
    //!   - Story 7.1 TDD contract (Happy Path #1-#2, Boundary #1-#4)
    //!   - architecture.md §5.4 (AD-5 classifier gate)
    //!   - tdd-fixtures.md F4 (controlled-mock providers), F6 (idempotency)

    use super::*;
    use sidebar_domain::reading::{MetricKind, Reading};
    use sidebar_sensor::descriptor::{CostClass, ProviderTier, SensorDescriptor};

    /// In-process stub provider for tier-filter tests. Holds a single leaked
    /// `&'static SensorDescriptor` so pointer-equality identification works.
    /// `read_all` returns empty (the filter never calls it).
    struct StubProvider {
        descriptor: &'static SensorDescriptor,
    }

    impl SensorProvider for StubProvider {
        fn descriptor(&self) -> &SensorDescriptor {
            self.descriptor
        }
        fn read_all(&self) -> Vec<Reading> {
            Vec::new()
        }
    }

    /// Leak a descriptor so its address is stable across clones.
    /// `filter_providers` identifies survivors by pointer-equality on the
    /// `&SensorDescriptor`; leaking gives us a stable address.
    fn leaked_descriptor(desc: SensorDescriptor) -> &'static SensorDescriptor {
        Box::leak(Box::new(desc))
    }

    /// Build a stub provider around a leaked descriptor.
    fn stub(descriptor: &'static SensorDescriptor) -> Arc<dyn SensorProvider> {
        Arc::new(StubProvider { descriptor })
    }

    // ----- Happy Path #1: Basic tier keeps sysinfo(Basic) + net(Both), drops ohm(Full) -----

    /// Story 7.1 Happy Path #1. Cited: Story 7.1 TDD contract.
    ///
    /// Input descriptors: sysinfo (LW/Basic), ohm (LW/Full), net (LW/Both).
    /// Active tier: Basic.
    /// Expected: sysinfo + net survive; ohm dropped.
    #[test]
    fn basic_tier_keeps_basic_and_both_drops_full() {
        let sysinfo_d = leaked_descriptor(SensorDescriptor::new(
            "sysinfo",
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        ));
        let ohm_d = leaked_descriptor(SensorDescriptor::new(
            "ohm",
            CostClass::Lightweight,
            &[MetricKind::CpuTemperature],
            ProviderTier::Full,
        ));
        let net_d = leaked_descriptor(SensorDescriptor::new(
            "net-nic",
            CostClass::Lightweight,
            &[MetricKind::NetRxBytes],
            ProviderTier::Both,
        ));

        let providers: Vec<Arc<dyn SensorProvider>> =
            vec![stub(sysinfo_d), stub(ohm_d), stub(net_d)];
        let filtered = filter_providers(providers, ActiveTier::Basic);

        // Two survivors: sysinfo + net. ohm (Full) is dropped.
        assert_eq!(filtered.len(), 2, "Basic tier: sysinfo + net, drops ohm");
        let names: Vec<&str> = filtered.iter().map(|p| p.descriptor().name).collect();
        assert!(names.contains(&"sysinfo"), "sysinfo must survive in Basic");
        assert!(names.contains(&"net-nic"), "net must survive in Basic");
        assert!(!names.contains(&"ohm"), "ohm must be dropped in Basic");
    }

    // ----- Happy Path #2: Full tier keeps all three -----

    /// Story 7.1 Happy Path #2. Cited: Story 7.1 TDD contract.
    ///
    /// Same descriptors as #1, but active tier = Full. All three survive.
    #[test]
    fn full_tier_keeps_all_three() {
        let sysinfo_d = leaked_descriptor(SensorDescriptor::new(
            "sysinfo",
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        ));
        let ohm_d = leaked_descriptor(SensorDescriptor::new(
            "ohm",
            CostClass::Lightweight,
            &[MetricKind::CpuTemperature],
            ProviderTier::Full,
        ));
        let net_d = leaked_descriptor(SensorDescriptor::new(
            "net-nic",
            CostClass::Lightweight,
            &[MetricKind::NetRxBytes],
            ProviderTier::Both,
        ));

        let providers: Vec<Arc<dyn SensorProvider>> =
            vec![stub(sysinfo_d), stub(ohm_d), stub(net_d)];
        let filtered = filter_providers(providers, ActiveTier::Full);

        assert_eq!(filtered.len(), 3, "Full tier keeps all three");
        let names: Vec<&str> = filtered.iter().map(|p| p.descriptor().name).collect();
        assert!(names.contains(&"sysinfo"));
        assert!(names.contains(&"ohm"));
        assert!(names.contains(&"net-nic"));
    }

    // ----- Boundary #1: Heavy descriptor rejected -----

    /// Story 7.1 Boundary #1. Cited: Story 7.1 TDD contract.
    ///
    /// A Heavy descriptor is rejected by the classifier with a `warn!`. The
    /// provider does NOT enter the registry.
    #[test]
    fn heavy_descriptor_is_rejected() {
        let heavy_d = leaked_descriptor(SensorDescriptor::new(
            "expensive",
            CostClass::Heavy,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        ));
        let providers: Vec<Arc<dyn SensorProvider>> = vec![stub(heavy_d)];
        let filtered = filter_providers(providers, ActiveTier::Full);
        assert!(
            filtered.is_empty(),
            "Heavy provider MUST NOT enter the registry"
        );
    }

    // ----- Boundary #2: Hot tier switch Basic → Full adds ohm -----

    /// Story 7.1 Boundary #2. Cited: Story 7.1 TDD contract.
    ///
    /// Simulates a hot tier switch mid-session: build the registry at Basic,
    /// then rebuild at Full. The second build MUST include ohm (Full-tier
    /// provider) that the first excluded.
    #[test]
    fn hot_tier_switch_basic_to_full_adds_ohm() {
        let sysinfo_d = leaked_descriptor(SensorDescriptor::new(
            "sysinfo",
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        ));
        let ohm_d = leaked_descriptor(SensorDescriptor::new(
            "ohm",
            CostClass::Lightweight,
            &[MetricKind::CpuTemperature],
            ProviderTier::Full,
        ));

        // Build at Basic — ohm dropped.
        let basic_providers: Vec<Arc<dyn SensorProvider>> = vec![stub(sysinfo_d), stub(ohm_d)];
        let basic_registry = filter_providers(basic_providers, ActiveTier::Basic);
        assert_eq!(basic_registry.len(), 1);
        assert_eq!(basic_registry[0].descriptor().name, "sysinfo");

        // Re-build at Full — ohm now admitted.
        let full_providers: Vec<Arc<dyn SensorProvider>> = vec![stub(sysinfo_d), stub(ohm_d)];
        let full_registry = filter_providers(full_providers, ActiveTier::Full);
        assert_eq!(full_registry.len(), 2, "Full tier rebuild adds ohm");
        let names: Vec<&str> = full_registry.iter().map(|p| p.descriptor().name).collect();
        assert!(names.contains(&"ohm"), "ohm admitted on Full rebuild");
    }

    // ----- Boundary #3: Empty registry → empty vec, no panic -----

    /// Story 7.1 Boundary #3. Cited: Story 7.1 TDD contract.
    ///
    /// An empty provider list yields an empty filtered vec. No panic, no
    /// error.
    #[test]
    fn empty_registry_returns_empty_vec() {
        let filtered = filter_providers(Vec::new(), ActiveTier::Full);
        assert!(filtered.is_empty(), "empty input → empty output, no panic");
    }

    // ----- Boundary #4: Idempotency (F6) -----

    /// Story 7.1 Boundary #4. Cited: Story 7.1 TDD contract, F6.
    ///
    /// Building the registry twice with the same active tier MUST produce
    /// equivalent registries (same provider count + same names in the same
    /// order).
    #[test]
    fn rebuild_twice_produces_identical_registry() {
        fn build(
            sysinfo_d: &'static SensorDescriptor,
            net_d: &'static SensorDescriptor,
        ) -> Vec<Arc<dyn SensorProvider>> {
            let providers: Vec<Arc<dyn SensorProvider>> = vec![stub(sysinfo_d), stub(net_d)];
            filter_providers(providers, ActiveTier::Basic)
        }

        let sysinfo_d = leaked_descriptor(SensorDescriptor::new(
            "sysinfo",
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization],
            ProviderTier::Basic,
        ));
        let net_d = leaked_descriptor(SensorDescriptor::new(
            "net-nic",
            CostClass::Lightweight,
            &[MetricKind::NetRxBytes],
            ProviderTier::Both,
        ));

        let first = build(sysinfo_d, net_d);
        let second = build(sysinfo_d, net_d);

        assert_eq!(first.len(), second.len(), "idempotent length");
        let first_names: Vec<&str> = first.iter().map(|p| p.descriptor().name).collect();
        let second_names: Vec<&str> = second.iter().map(|p| p.descriptor().name).collect();
        assert_eq!(first_names, second_names, "idempotent order + names");
    }

    // ----- Smoke: production build_registry constructs without panic -----

    /// Smoke that the production `build_registry` constructs without panic on
    /// the dev machine. The adapter set is machine-dependent (NVML empty on
    /// AMD, PDH may have no drives, OHM empty before LHM launches), so we
    /// only assert: no panic, all returned providers are `Send + Sync` (the
    /// `Arc<dyn SensorProvider>` is statically `Send + Sync`).
    #[test]
    fn production_build_registry_constructs_without_panic_basic() {
        let registry = build_registry(ActiveTier::Basic);
        // Basic tier excludes ohm (Full). At minimum, sysinfo + battery + pdh
        // + net + nvml are all Basic-or-Both → at least 4 survive (nvml is
        // Basic; even if all returned empty, the descriptor still passes).
        assert!(
            !registry.is_empty(),
            "Basic registry must contain at least the Basic/Both adapters"
        );
        // ohm (Full-tier) MUST be excluded in Basic mode.
        let names: Vec<&str> = registry.iter().map(|p| p.descriptor().name).collect();
        assert!(
            !names.contains(&"ohm"),
            "ohm MUST be excluded in Basic mode"
        );
    }

    /// Same smoke for Full mode — ohm IS admitted.
    #[test]
    fn production_build_registry_constructs_without_panic_full() {
        let registry = build_registry(ActiveTier::Full);
        // Full mode admits every shipped adapter.
        let names: Vec<&str> = registry.iter().map(|p| p.descriptor().name).collect();
        assert!(names.contains(&"ohm"), "ohm MUST be admitted in Full mode");
        assert!(
            names.contains(&"sysinfo"),
            "sysinfo present in Full registry"
        );
        assert!(names.contains(&"net-nic"), "net present in Full registry");
    }
}
