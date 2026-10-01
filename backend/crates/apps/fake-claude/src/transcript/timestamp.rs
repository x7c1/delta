//! RFC 3339 timestamps for transcript lines.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current wall-clock time as an RFC 3339 UTC timestamp (second
/// precision), without pulling in a date-time dependency for one format.
pub(super) fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_from_unix(secs as i64)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Uses the standard civil-from-days algorithm (Howard Hinnant's
/// `civil_from_days`) for the date part.
fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_unix_seconds() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        // 2026-01-01T00:00:00Z
        assert_eq!(rfc3339_from_unix(1_767_225_600), "2026-01-01T00:00:00Z");
    }
}
