//! Pure helpers for Story 10.1 acceptance checks.

/// Return the nearest-rank percentile from a sample set.
///
/// Empty samples return `None`; percentile values above 100 are clamped so
/// callers cannot accidentally index past the sorted sample list.
#[must_use]
pub fn percentile(samples: &[u64], percentile: u8) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let p = usize::from(percentile.min(100));
    let rank = p.saturating_mul(sorted.len()).saturating_add(99) / 100;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[index])
}

/// Extract concrete remote endpoints from Windows `netstat -ano` output for a
/// single process. Wildcard endpoints (`*:*`, `0.0.0.0:0`, `[::]:0`) are not
/// outbound connections and are omitted.
#[must_use]
pub fn remote_endpoints_for_pid(output: &str, pid: u32) -> Vec<String> {
    let pid = pid.to_string();
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 4 || fields.last().copied() != Some(pid.as_str()) {
                return None;
            }
            let protocol = fields[0].to_ascii_uppercase();
            let remote_index = match protocol.as_str() {
                "TCP" | "UDP" => 2,
                _ => return None,
            };
            let remote = fields.get(remote_index)?.to_string();
            if matches!(remote.as_str(), "*:*" | "0.0.0.0:0" | "[::]:0") {
                None
            } else {
                Some(remote)
            }
        })
        .collect()
}

/// Parse the elapsed milliseconds emitted by `--bench-cold-start`.
///
/// The probe writes a tiny key/value file so the parent process never needs to
/// share an `Instant` across process boundaries.
#[must_use]
pub fn parse_cold_start_elapsed_ms(contents: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() == "elapsed_ms" {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_cold_start_elapsed_ms, percentile, remote_endpoints_for_pid};

    #[test]
    fn percentile_p95_uses_nearest_rank() {
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 95), Some(5));
    }

    #[test]
    fn percentile_empty_input_is_none() {
        assert_eq!(percentile(&[], 95), None);
    }

    #[test]
    fn netstat_parser_keeps_only_matching_pid_remotes() {
        let output = "  TCP    127.0.0.1:1000    127.0.0.1:2000    ESTABLISHED    42\n  TCP    0.0.0.0:0       0.0.0.0:0       LISTENING      42\n  TCP    127.0.0.1:1001    203.0.113.10:443 ESTABLISHED    99\n";
        assert_eq!(
            remote_endpoints_for_pid(output, 42),
            vec!["127.0.0.1:2000".to_string()]
        );
    }

    #[test]
    fn netstat_parser_handles_udp_shape_and_ignores_wildcards() {
        let output = "  UDP    0.0.0.0:5353    *:*                             7\n  UDP    127.0.0.1:5354  127.0.0.1:5355                   7\n";
        assert_eq!(
            remote_endpoints_for_pid(output, 7),
            vec!["127.0.0.1:5355".to_string()]
        );
    }

    #[test]
    fn cold_start_parser_reads_elapsed_ms_and_rejects_missing_value() {
        assert_eq!(
            parse_cold_start_elapsed_ms("start_ms=1\nelapsed_ms=42\n"),
            Some(42)
        );
        assert_eq!(parse_cold_start_elapsed_ms("start_ms=1\n"), None);
    }
}
