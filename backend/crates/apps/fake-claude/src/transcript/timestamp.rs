//! RFC 3339 timestamps for transcript lines.

use delta_time::Instant;

/// The current wall-clock time as an RFC 3339 UTC timestamp (second
/// precision).
pub(super) fn rfc3339_now() -> String {
    rfc3339(Instant::now())
}

/// Format `at` as `YYYY-MM-DDTHH:MM:SSZ`, the form `claude` writes. A clock
/// before the Unix epoch reads as the epoch.
fn rfc3339(at: Instant) -> String {
    at.max(Instant::UNIX_EPOCH).to_rfc3339_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_secs(secs: i64) -> Instant {
        Instant::from_epoch_secs(secs).expect("in range")
    }

    #[test]
    fn formats_known_instants() {
        assert_eq!(rfc3339(at_secs(0)), "1970-01-01T00:00:00Z");
        // 2026-01-01T00:00:00Z
        assert_eq!(rfc3339(at_secs(1_767_225_600)), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn a_clock_before_the_epoch_reads_as_the_epoch() {
        assert_eq!(rfc3339(at_secs(-1)), "1970-01-01T00:00:00Z");
    }
}
