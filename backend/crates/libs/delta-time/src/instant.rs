use jiff::Timestamp;

use crate::Date;

/// A point in time, in UTC, with at least millisecond precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Timestamp);

impl Instant {
    /// The current time from the system clock.
    pub fn now() -> Self {
        Self(Timestamp::now())
    }

    /// The instant `millis` milliseconds after the Unix epoch (negative for
    /// before it), or `None` when it falls outside the supported range
    /// (years -9999 to 9999).
    pub fn from_epoch_millis(millis: i64) -> Option<Self> {
        Timestamp::from_millisecond(millis).ok().map(Self)
    }

    /// Milliseconds since the Unix epoch, truncated toward the epoch.
    pub fn epoch_millis(self) -> i64 {
        self.0.as_millisecond()
    }

    /// The calendar date of this instant in UTC.
    pub fn utc_date(self) -> Date {
        Date::from_civil(jiff::tz::TimeZone::UTC.to_datetime(self.0).date())
    }

    /// The RFC 3339 form in UTC with exactly three fractional digits and a
    /// `Z` suffix, e.g. `2026-03-07T01:02:03.004Z`. Every field is
    /// zero-padded, so for years 0 to 9999 two of these strings order as
    /// text the way their instants order in time.
    pub fn to_rfc3339_millis(self) -> String {
        format!("{:.3}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_millis_pads_every_field() {
        // 2026-03-07T01:02:03.004Z: single-digit month, day, hour, minute,
        // second and millisecond.
        let instant = Instant::from_epoch_millis(1_772_845_323_004).unwrap();
        assert_eq!(instant.to_rfc3339_millis(), "2026-03-07T01:02:03.004Z");
    }

    #[test]
    fn rfc3339_millis_keeps_three_digits_for_whole_seconds() {
        let instant = Instant::from_epoch_millis(1_700_000_000_000).unwrap();
        assert_eq!(instant.to_rfc3339_millis(), "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn rfc3339_millis_of_the_epoch_and_before_it() {
        assert_eq!(
            Instant::from_epoch_millis(0).unwrap().to_rfc3339_millis(),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            Instant::from_epoch_millis(-1).unwrap().to_rfc3339_millis(),
            "1969-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn out_of_range_epoch_millis_is_none() {
        assert_eq!(Instant::from_epoch_millis(i64::MAX), None);
        assert_eq!(Instant::from_epoch_millis(i64::MIN), None);
    }

    #[test]
    fn epoch_millis_round_trips() {
        let instant = Instant::from_epoch_millis(1_700_000_000_123).unwrap();
        assert_eq!(instant.epoch_millis(), 1_700_000_000_123);
    }

    #[test]
    fn utc_date_is_the_day_in_utc() {
        // 2026-03-07T23:59:59.999Z is still the 7th in UTC.
        let instant = Instant::from_epoch_millis(1_772_927_999_999).unwrap();
        assert_eq!(instant.utc_date(), Date::from_ymd(2026, 3, 7).unwrap());
    }

    #[test]
    fn now_is_after_the_epoch() {
        assert!(Instant::now().epoch_millis() > 0);
    }
}
