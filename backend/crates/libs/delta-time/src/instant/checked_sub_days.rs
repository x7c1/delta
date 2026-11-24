use jiff::SignedDuration;

use super::Instant;

/// Seconds in a day, for subtracting whole days from an instant.
const SECS_PER_DAY: i64 = 86_400;

impl Instant {
    /// The instant `days` whole days of 24 hours before this one (after it
    /// for a negative `days`), or `None` when it falls outside the supported
    /// range.
    pub fn checked_sub_days(self, days: i64) -> Option<Self> {
        let secs = days.checked_mul(SECS_PER_DAY)?;
        self.0
            .checked_sub(SignedDuration::from_secs(secs))
            .ok()
            .map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtracting_days_takes_whole_days_of_24_hours() {
        let now = Instant::from_epoch_secs(1_609_459_200).unwrap(); // 2021-01-01
        assert_eq!(
            now.checked_sub_days(31).map(Instant::to_rfc3339_secs),
            Some("2020-12-01T00:00:00Z".to_owned())
        );
        assert_eq!(now.checked_sub_days(0), Some(now));
        assert_eq!(
            now.checked_sub_days(-1).map(Instant::to_rfc3339_secs),
            Some("2021-01-02T00:00:00Z".to_owned())
        );
        // Before the epoch is still an instant.
        assert_eq!(
            Instant::UNIX_EPOCH.checked_sub_days(1),
            Instant::from_epoch_secs(-86_400)
        );
    }

    #[test]
    fn subtracting_past_the_supported_range_is_none() {
        assert_eq!(Instant::UNIX_EPOCH.checked_sub_days(i64::MAX), None);
        assert_eq!(Instant::UNIX_EPOCH.checked_sub_days(20_000 * 366), None);
    }
}
