//! Story 2.3 — Compile-time cost + tier classifier gate.
//!
//! Filters adapters based on `CostClass` + `ProviderTier` before they enter
//! the v1 provider registry. This is the enforcement mechanism for NFR-1
//! (lightweight mandate) and the two-tier model.
//!
//! Cited: architecture.md §5.4 + AD-5, Story 2.3, T-1.

use crate::descriptor::{CostClass, ProviderTier, SensorDescriptor};

/// The runtime tier active when classification happens. Separate from
/// `ProviderTier` (which is a *requirement*); this is the *active* mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTier {
    /// Basic mode — no admin, no LHM.
    Basic,
    /// Full mode — LHM subprocess running.
    Full,
}

/// Filter descriptors to those that should run in v1 under the active tier.
///
/// Returns references to the retained descriptors. Heavy + Deferred sources
/// are rejected with a `tracing::warn!` carrying structured fields for
/// orchestrator audit. Tier filtering: `Full`-required providers only run
/// when `active_tier=Full`; `Basic` and `Both` providers run in both modes.
///
/// # Rules
///
/// | CostClass | Accepted? |
/// |---|---|
/// | Lightweight | ✅ if tier matches |
/// | Watch | ✅ if tier matches (CI bench verifies threshold) |
/// | Heavy | ❌ always (NFR-1 violation) |
/// | Deferred | ❌ always (out of v1 scope) |
///
/// | ProviderTier | ActiveTier::Basic | ActiveTier::Full |
/// |---|---|---|
/// | Basic | ✅ | ✅ |
/// | Full | ❌ (silent) | ✅ |
/// | Both | ✅ | ✅ |
pub fn classify_for_v1<'a>(
    descriptors: &'a [&'a SensorDescriptor],
    active_tier: ActiveTier,
) -> Vec<&'a SensorDescriptor> {
    descriptors
        .iter()
        .copied()
        .filter(|d| {
            // Cost-class gate.
            let cost_ok = match d.cost_class {
                CostClass::Lightweight | CostClass::Watch => true,
                CostClass::Heavy => {
                    tracing::warn!(
                        source = d.name,
                        cost_class = ?d.cost_class,
                        reason = "NFR-1 violation: cost_class is Heavy",
                        "rejecting provider from v1 registry"
                    );
                    false
                }
                CostClass::Deferred => {
                    tracing::warn!(
                        source = d.name,
                        cost_class = ?d.cost_class,
                        reason = "out of v1 scope: cost_class is Deferred",
                        "rejecting provider from v1 registry"
                    );
                    false
                }
            };
            if !cost_ok {
                return false;
            }
            // Tier gate.
            match (d.requires_tier, active_tier) {
                (ProviderTier::Both | ProviderTier::Basic, _)
                | (ProviderTier::Full, ActiveTier::Full) => true,
                (ProviderTier::Full, ActiveTier::Basic) => false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sidebar_domain::reading::MetricKind;

    fn make_desc(
        name: &'static str,
        cost: CostClass,
        tier: ProviderTier,
    ) -> &'static SensorDescriptor {
        Box::leak(Box::new(SensorDescriptor::new(
            name,
            cost,
            &[MetricKind::CpuUtilization],
            tier,
        )))
    }

    #[test]
    fn lightweight_and_watch_retained() {
        let lw = make_desc("lw", CostClass::Lightweight, ProviderTier::Basic);
        let watch = make_desc("watch", CostClass::Watch, ProviderTier::Full);
        let descs = [lw, watch];
        let result = classify_for_v1(&descs, ActiveTier::Full);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn heavy_and_deferred_rejected() {
        let heavy = make_desc("heavy", CostClass::Heavy, ProviderTier::Basic);
        let deferred = make_desc("defer", CostClass::Deferred, ProviderTier::Both);
        let descs = [heavy, deferred];
        let result = classify_for_v1(&descs, ActiveTier::Full);
        assert!(result.is_empty(), "Heavy + Deferred must be rejected");
    }

    #[test]
    fn full_tier_rejected_in_basic_mode() {
        let full = make_desc("ohm", CostClass::Lightweight, ProviderTier::Full);
        let descs = [full];
        let result = classify_for_v1(&descs, ActiveTier::Basic);
        assert!(
            result.is_empty(),
            "Full-tier provider must not run in Basic mode"
        );
    }

    #[test]
    fn full_tier_accepted_in_full_mode() {
        let full = make_desc("ohm", CostClass::Lightweight, ProviderTier::Full);
        let descs = [full];
        let result = classify_for_v1(&descs, ActiveTier::Full);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn both_tier_runs_in_both_modes() {
        let both = make_desc("net", CostClass::Lightweight, ProviderTier::Both);
        let descs = [both];
        assert_eq!(
            classify_for_v1(&descs, ActiveTier::Basic).len(),
            1,
            "Both should run in Basic"
        );
        assert_eq!(
            classify_for_v1(&descs, ActiveTier::Full).len(),
            1,
            "Both should run in Full"
        );
    }

    #[test]
    fn basic_tier_runs_in_both_modes() {
        // Basic providers run in Full mode too — Full is a superset.
        let basic = make_desc("sysinfo", CostClass::Lightweight, ProviderTier::Basic);
        let descs = [basic];
        assert_eq!(classify_for_v1(&descs, ActiveTier::Basic).len(), 1);
        assert_eq!(classify_for_v1(&descs, ActiveTier::Full).len(), 1);
    }

    #[test]
    fn empty_input_returns_empty() {
        let result = classify_for_v1(&[], ActiveTier::Full);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicates_both_retained() {
        let d = make_desc("dup", CostClass::Lightweight, ProviderTier::Basic);
        let descs = [d, d];
        let result = classify_for_v1(&descs, ActiveTier::Full);
        assert_eq!(result.len(), 2, "dedup is NOT the classifier's job");
    }

    #[test]
    fn mixed_descriptors_filter_correctly() {
        let sysinfo = make_desc("sysinfo", CostClass::Lightweight, ProviderTier::Basic);
        let net = make_desc("net", CostClass::Lightweight, ProviderTier::Both);
        let ohm = make_desc("ohm", CostClass::Lightweight, ProviderTier::Full);
        let heavy = make_desc("x", CostClass::Heavy, ProviderTier::Basic);
        let descs = [sysinfo, net, ohm, heavy];

        // Basic mode: sysinfo ✅, net ✅, ohm ❌, heavy ❌ → 2 retained.
        let basic = classify_for_v1(&descs, ActiveTier::Basic);
        assert_eq!(basic.len(), 2);

        // Full mode: sysinfo ✅, net ✅, ohm ✅, heavy ❌ → 3 retained.
        let full = classify_for_v1(&descs, ActiveTier::Full);
        assert_eq!(full.len(), 3);
    }
}
