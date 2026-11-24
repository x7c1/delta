use super::Instant;
use crate::Date;

impl Instant {
    /// The calendar date of this instant in UTC.
    pub fn utc_date(self) -> Date {
        Date::from_civil(jiff::tz::TimeZone::UTC.to_datetime(self.0).date())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_is_the_day_in_utc() {
        // 2026-03-07T23:59:59.999Z is still the 7th in UTC.
        let instant = Instant::from_epoch_millis(1_772_927_999_999).unwrap();
        assert_eq!(instant.utc_date(), Date::from_ymd(2026, 3, 7).unwrap());
    }
}
