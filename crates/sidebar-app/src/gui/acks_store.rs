//! Story 17.1 — Alert ack persistence (acks.toml sidecar).
//!
//! Persists active alert acks (acknowledge + snooze) to
//! `%APPDATA%\sidebar\acks.toml` so a restart doesn't lose an active snooze.
//! Uses the existing `atomic_write_config` pattern (temp + rename, G15) +
//! the `toml` workspace dep. No serde in sidebar-domain — the serialization
//! is hand-rolled here using simple TOML key=value lines.
//!
//! Cited: Story 17.1, guardrails.md G28.

use sidebar_domain::alert::AlertAck;
use sidebar_domain::graph::MetricKey;
use std::collections::HashMap;
use std::path::Path;

/// Load acks from `acks.toml`. Prunes expired snoozes. Best-effort per G15:
/// a corrupt/missing file returns an empty map (no crash).
#[allow(clippy::implicit_hasher)]
pub fn load_acks(path: &Path, now_epoch: i64) -> HashMap<MetricKey, AlertAck> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key_str, val_str)) = line.split_once('=') {
            let parts: Vec<&str> = key_str.trim().split('|').collect();
            if parts.len() != 3 {
                continue;
            }
            let key = MetricKey {
                category: parts[0].to_string(),
                instance: parts[1].to_string(),
                kind: parts[2].to_string(),
            };
            let val_str = val_str.trim();
            let ack = if let Some(rest) = val_str.strip_prefix("Snoozed:") {
                let until = rest.trim().parse::<i64>().unwrap_or(0);
                if until > now_epoch {
                    Some(AlertAck::Snoozed(until))
                } else {
                    None
                }
            } else if val_str == "Acknowledged" {
                Some(AlertAck::Acknowledged)
            } else {
                None
            };
            if let Some(ack) = ack {
                map.insert(key, ack);
            }
        }
    }
    map
}

/// Save acks to `acks.toml` atomically. Best-effort per G15.
#[allow(clippy::format_push_string, clippy::implicit_hasher)]
pub fn save_acks(path: &Path, acks: &HashMap<MetricKey, AlertAck>) {
    let mut lines = String::from("# sidebar alert acks\n");
    for (key, ack) in acks {
        match ack {
            AlertAck::Acknowledged => {
                lines.push_str(&format!(
                    "{}|{}|{} = Acknowledged\n",
                    key.category, key.instance, key.kind
                ));
            }
            AlertAck::Snoozed(until) => {
                lines.push_str(&format!(
                    "{}|{}|{} = Snoozed:{}\n",
                    key.category, key.instance, key.kind, until
                ));
            }
        }
    }
    crate::gui::atomic_write_config(path, &lines);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ack_round_trip_survives_restart() {
        let dir = TempDir::new().expect("temp");
        let path = dir.path().join("acks.toml");
        let mut acks = HashMap::new();
        acks.insert(
            MetricKey {
                category: "cpu".into(),
                instance: "package".into(),
                kind: "CpuTemperature".into(),
            },
            AlertAck::Snoozed(99999),
        );
        save_acks(&path, &acks);
        let loaded = load_acks(&path, 0);
        assert_eq!(loaded.len(), 1);
        let ack = loaded.values().next().unwrap();
        assert!(matches!(ack, AlertAck::Snoozed(99999)));
    }

    #[test]
    fn expired_snooze_pruned_on_load() {
        let dir = TempDir::new().expect("temp");
        let path = dir.path().join("acks.toml");
        let mut acks = HashMap::new();
        acks.insert(
            MetricKey {
                category: "cpu".into(),
                instance: "package".into(),
                kind: "CpuTemperature".into(),
            },
            AlertAck::Snoozed(100),
        );
        save_acks(&path, &acks);
        // now_epoch = 200 > 100 → expired → pruned
        let loaded = load_acks(&path, 200);
        assert!(loaded.is_empty(), "expired snooze must be pruned");
    }

    #[test]
    fn corrupt_acks_file_returns_empty() {
        let dir = TempDir::new().expect("temp");
        let path = dir.path().join("acks.toml");
        std::fs::write(&path, b"garbage not valid toml = = =").expect("write");
        let loaded = load_acks(&path, 0);
        assert!(
            loaded.is_empty(),
            "corrupt file must return empty, not crash"
        );
    }
}
