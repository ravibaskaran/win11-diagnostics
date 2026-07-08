//! Story 2.2 — Sensor descriptor, cost class, and tier enums.
//!
//! These types let adapters self-declare their cost and tier requirements.
//! The classifier (Story 2.3) uses them to gate which adapters run.
//!
//! Cited: architecture.md §5.3 + AD-5, Story 2.2.

use sidebar_domain::reading::MetricKind;

/// Profiling-based cost classification per NFR-1.
///
/// Adapters MUST declare one of these. The classifier rejects `Heavy` and
/// `Deferred` at registry-build time (Story 2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostClass {
    /// Profiling evidence: < 0.1% CPU avg per tick. Ship unconditionally.
    Lightweight,
    /// Profiling evidence: 0.1–0.5% CPU avg per tick. Ship, but bench in CI.
    Watch,
    /// > 0.5% CPU avg OR disproportionate syscall churn. DO NOT ship in v1.
    Heavy,
    /// Lightweight by measurement, but out of v1 scope.
    Deferred,
}

/// Runtime tier requirement. Distinct from `sidebar-domain::snapshot::Tier`
/// (which is the *active* runtime mode). This is a *requirement* a provider
/// declares; the classifier checks it against the active tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderTier {
    /// Runs in both Basic and Full modes.
    Basic,
    /// Runs only when the active tier is Full.
    Full,
    /// Tier-agnostic — always runs regardless of mode (e.g. network, bandwidth).
    Both,
}

/// Metadata every `SensorProvider` MUST declare.
///
/// The `name` is a stable identifier for log/audit trail purposes. `metrics`
/// is the list of `MetricKind` variants this provider emits — used for
/// documentation and future GUI auto-config. `cost_class` gates inclusion
/// per NFR-1. `requires_tier` gates inclusion per the two-tier model.
#[derive(Debug, Clone)]
pub struct SensorDescriptor {
    /// Stable provider name (e.g. `"sysinfo-cpu"`, `"ohm-cpu-temp"`).
    pub name: &'static str,
    /// NFR-1 cost classification.
    pub cost_class: CostClass,
    /// Metric kinds this provider emits.
    pub metrics: &'static [MetricKind],
    /// Tier requirement.
    pub requires_tier: ProviderTier,
}

impl SensorDescriptor {
    /// Construct a new descriptor.
    #[must_use]
    pub const fn new(
        name: &'static str,
        cost_class: CostClass,
        metrics: &'static [MetricKind],
        requires_tier: ProviderTier,
    ) -> Self {
        Self {
            name,
            cost_class,
            metrics,
            requires_tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_constructs() {
        let d = SensorDescriptor::new(
            "sysinfo-cpu",
            CostClass::Lightweight,
            &[MetricKind::CpuUtilization, MetricKind::CpuFrequency],
            ProviderTier::Basic,
        );
        assert_eq!(d.name, "sysinfo-cpu");
        assert_eq!(d.cost_class, CostClass::Lightweight);
        assert_eq!(d.metrics.len(), 2);
        assert_eq!(d.requires_tier, ProviderTier::Basic);
    }

    #[test]
    fn cost_class_exhaustive_match() {
        fn classify(c: CostClass) -> &'static str {
            match c {
                CostClass::Lightweight => "ship",
                CostClass::Watch => "ship-bench",
                CostClass::Heavy => "reject",
                CostClass::Deferred => "defer",
            }
        }
        assert_eq!(classify(CostClass::Lightweight), "ship");
        assert_eq!(classify(CostClass::Heavy), "reject");
    }

    #[test]
    fn tier_exhaustive_match() {
        fn runs_in_basic(t: ProviderTier) -> bool {
            match t {
                ProviderTier::Basic | ProviderTier::Both => true,
                ProviderTier::Full => false,
            }
        }
        assert!(runs_in_basic(ProviderTier::Basic));
        assert!(!runs_in_basic(ProviderTier::Full));
        assert!(runs_in_basic(ProviderTier::Both));
    }

    #[test]
    fn empty_metrics_is_legal_but_suspicious() {
        let d = SensorDescriptor::new("empty", CostClass::Lightweight, &[], ProviderTier::Basic);
        assert_eq!(d.metrics.len(), 0);
    }
}
