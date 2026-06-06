//! Timestamp helpers.
//!
//! SQLite has no native datetime type, so Delta stores timestamps as ISO-8601
//! text. This module produces a UTC timestamp without pulling in a date/time
//! dependency, formatting from the system clock.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC time as an ISO-8601 `YYYY-MM-DDTHH:MM:SSZ` string.
pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_utc(secs)
}

/// Format a Unix timestamp (seconds) as ISO-8601 UTC.
fn format_unix_utc(secs: u64) -> String {
    // Days since the Unix epoch and seconds within the day.
    let days = secs / 86_400;
    let rem = secs % 86_400;
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
        assert_eq!(format_unix_utc(0), "1970-01-01T00:00:00Z");
        // 2021-01-01T00:00:00Z
        assert_eq!(format_unix_utc(1_609_459_200), "2021-01-01T00:00:00Z");
        // 2009-02-13T23:31:30Z
        assert_eq!(format_unix_utc(1_234_567_890), "2009-02-13T23:31:30Z");
    }
}
