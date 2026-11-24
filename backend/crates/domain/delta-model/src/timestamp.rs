//! The text form Delta gives a point in time: ISO-8601 UTC with second
//! resolution (`YYYY-MM-DDTHH:MM:SSZ`), the form every stored timestamp takes.
//!
//! Decided here, so the store that writes timestamps and the use case that
//! compares against them (a cut-off for "older than N days") agree on one
//! spelling. Timestamps in this form compare correctly as text, which is how
//! SQLite orders and compares existing rows against new ones.

use delta_time::Instant;

/// Format `at` as ISO-8601 UTC with second resolution
/// (`YYYY-MM-DDTHH:MM:SSZ`), dropping any fraction of a second.
///
/// An instant before the Unix epoch reads as the epoch,
/// `1970-01-01T00:00:00Z`: no stored timestamp precedes it, and a clock or a
/// cut-off that reaches before it is not worth an error.
pub fn iso8601_utc(at: Instant) -> String {
    at.max(Instant::UNIX_EPOCH).to_rfc3339_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_secs(secs: i64) -> Instant {
        Instant::from_epoch_secs(secs).expect("in range")
    }

    #[test]
    fn formats_known_epoch_values() {
        assert_eq!(iso8601_utc(at_secs(0)), "1970-01-01T00:00:00Z");
        // 2021-01-01T00:00:00Z
        assert_eq!(iso8601_utc(at_secs(1_609_459_200)), "2021-01-01T00:00:00Z");
        // 2009-02-13T23:31:30Z
        assert_eq!(iso8601_utc(at_secs(1_234_567_890)), "2009-02-13T23:31:30Z");
    }

    #[test]
    fn drops_the_fraction_of_a_second() {
        let at = Instant::from_epoch_millis(1_234_567_890_999).expect("in range");
        assert_eq!(iso8601_utc(at), "2009-02-13T23:31:30Z");
    }

    #[test]
    fn a_time_before_the_epoch_reads_as_the_epoch() {
        assert_eq!(iso8601_utc(at_secs(-1)), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(at_secs(-86_400 * 400)), "1970-01-01T00:00:00Z");
    }
}
