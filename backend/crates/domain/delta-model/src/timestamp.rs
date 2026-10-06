//! The text form Delta gives a point in time: ISO-8601 UTC with second
//! resolution (`YYYY-MM-DDTHH:MM:SSZ`), the form every stored timestamp takes.
//!
//! Formatted here, without a date/time dependency, so the store that writes
//! timestamps and the use case that compares against them (a cut-off for
//! "older than N days") agree on one spelling. Timestamps in this form compare
//! correctly as text.

/// Format `unix_secs` (seconds since the Unix epoch) as ISO-8601 UTC.
pub fn iso8601_utc(unix_secs: u64) -> String {
    // Days since the Unix epoch and seconds within the day.
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert days since 1970-01-01 to a (year, month, day) Gregorian date.
///
/// Adapted from Howard Hinnant's well-known `civil_from_days` algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_epoch_values() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        // 2021-01-01T00:00:00Z
        assert_eq!(iso8601_utc(1_609_459_200), "2021-01-01T00:00:00Z");
        // 2009-02-13T23:31:30Z
        assert_eq!(iso8601_utc(1_234_567_890), "2009-02-13T23:31:30Z");
    }
}
