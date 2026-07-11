//! Story 12.5 — battery health + adapter metadata DTOs.
//!
//! Doc-marked DEFERRED per epics-and-stories.md. The v1 deliverable is the
//! data shape for the GUI; the actual Win32/WinRT battery-health source +
//! GetAdaptersAddresses IP lookup land post-v1 pending NFR-1 measurement
//! on supported hardware.
//!
//! Per Story 12.5 scope: adapter name/IP is DISPLAY-ONLY metadata alongside
//! the existing LUID-keyed bandwidth accounting (T-24). The accounting
//! identity is unaffected.

/// Battery health snapshot. None of these fields affect bandwidth or sensor
/// accounting; they're pure display metadata for the bandwidth panel.
///
/// `health_percent` is the design-capacity ratio (0–100). `cycle_count` is
/// the cumulative charge-cycle count when available (None on hardware that
/// doesn't expose it — common on older laptops).
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryHealth {
    /// Design-capacity health, 0–100 percent. None when the Windows source
    /// is unavailable (desktops, unsupported laptops).
    pub health_percent: Option<u8>,
    /// Cumulative charge-cycle count. None when not exposed by the battery
    /// controller.
    pub cycle_count: Option<u32>,
}

/// Adapter display metadata for the bandwidth panel. The LUID remains the
/// accounting identity (T-24); these fields are display-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterMetadata {
    /// The LUID this metadata attaches to (matches `AccEntry`/`NICtotals`).
    pub luid: u64,
    /// Human-friendly adapter name (e.g. "Ethernet", "Wi-Fi").
    pub friendly_name: String,
    /// IPv4 address string (e.g. "192.168.1.42") when connected, None otherwise.
    pub ipv4: Option<String>,
    /// IPv6 address string (link-local or global) when connected, None otherwise.
    pub ipv6: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AdapterMetadata, BatteryHealth};

    #[test]
    fn battery_health_with_full_data() {
        let h = BatteryHealth {
            health_percent: Some(92),
            cycle_count: Some(137),
        };
        assert_eq!(h.health_percent, Some(92));
        assert_eq!(h.cycle_count, Some(137));
    }

    #[test]
    fn battery_health_unsupported_hardware() {
        // Desktops / unsupported laptops report None for both.
        let h = BatteryHealth {
            health_percent: None,
            cycle_count: None,
        };
        assert!(h.health_percent.is_none());
        assert!(h.cycle_count.is_none());
    }

    #[test]
    fn adapter_metadata_display_only_identity_is_luid() {
        let m = AdapterMetadata {
            luid: 123_456,
            friendly_name: "Wi-Fi".to_string(),
            ipv4: Some("192.168.1.42".to_string()),
            ipv6: None,
        };
        // The LUID is the accounting identity; the rest is display-only.
        assert_eq!(m.luid, 123_456);
        assert_eq!(m.friendly_name, "Wi-Fi");
        assert_eq!(m.ipv4.as_deref(), Some("192.168.1.42"));
        assert!(m.ipv6.is_none());
    }
}
