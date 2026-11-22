use std::fmt;

use jiff::civil;
use jiff::Span;

use crate::Instant;

/// A calendar date with no time of day or time zone. Displays as
/// `YYYY-MM-DD` (four year digits for years 0 to 9999).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(civil::Date);

impl Date {
    /// The date `year`-`month`-`day`, or `None` when it does not exist or
    /// falls outside the supported range (years -9999 to 9999).
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Self> {
        let year = i16::try_from(year).ok()?;
        let month = i8::try_from(month).ok()?;
        let day = i8::try_from(day).ok()?;
        civil::Date::new(year, month, day).ok().map(Self)
    }

    /// Today's date in UTC, from the system clock.
    pub fn today_utc() -> Self {
        Instant::now().utc_date()
    }

    /// The date `days` days before this one, or `None` when it falls outside
    /// the supported range.
    pub fn checked_sub_days(self, days: i64) -> Option<Self> {
        let span = Span::new().try_days(days).ok()?;
        self.0.checked_sub(span).ok().map(Self)
    }

    pub(crate) fn from_civil(date: civil::Date) -> Self {
        Self(date)
    }
}

impl fmt::Display for Date {
    /// `YYYY-MM-DD`, with the month and day zero-padded to two digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}",
            self.0.year(),
            self.0.month(),
            self.0.day()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date::from_ymd(year, month, day).expect("valid date")
    }

    #[test]
    fn displays_as_yyyy_mm_dd_with_zero_padding() {
        assert_eq!(date(2026, 3, 7).to_string(), "2026-03-07");
        assert_eq!(date(2025, 1, 1).to_string(), "2025-01-01");
        assert_eq!(date(2026, 12, 31).to_string(), "2026-12-31");
    }

    #[test]
    fn rejects_a_date_that_does_not_exist() {
        assert_eq!(Date::from_ymd(2026, 2, 29), None);
        assert_eq!(Date::from_ymd(2026, 13, 1), None);
        assert_eq!(Date::from_ymd(2026, 4, 31), None);
        assert_eq!(Date::from_ymd(100_000, 1, 1), None);
    }

    #[test]
    fn subtracting_days_crosses_a_month_boundary() {
        assert_eq!(
            date(2026, 3, 1).checked_sub_days(1),
            Some(date(2026, 2, 28))
        );
        assert_eq!(
            date(2024, 3, 1).checked_sub_days(1),
            Some(date(2024, 2, 29))
        );
    }

    #[test]
    fn subtracting_days_crosses_a_year_boundary() {
        assert_eq!(
            date(2026, 1, 1).checked_sub_days(1),
            Some(date(2025, 12, 31))
        );
        assert_eq!(
            date(2026, 6, 24).checked_sub_days(365),
            Some(date(2025, 6, 24))
        );
        // 2024 is a leap year: 365 days before 2025-01-10 is 2024-01-11.
        assert_eq!(
            date(2025, 1, 10).checked_sub_days(365),
            Some(date(2024, 1, 11))
        );
    }

    #[test]
    fn subtracting_zero_or_negative_days() {
        assert_eq!(date(2026, 3, 7).checked_sub_days(0), Some(date(2026, 3, 7)));
        assert_eq!(
            date(2026, 12, 31).checked_sub_days(-1),
            Some(date(2027, 1, 1))
        );
    }

    #[test]
    fn subtracting_past_the_supported_range_is_none() {
        assert_eq!(date(2026, 3, 7).checked_sub_days(i64::MAX), None);
        assert_eq!(date(-9999, 1, 1).checked_sub_days(1), None);
    }

    #[test]
    fn today_utc_is_a_real_date() {
        let today = Date::today_utc();
        assert!(today > date(2020, 1, 1));
    }
}
