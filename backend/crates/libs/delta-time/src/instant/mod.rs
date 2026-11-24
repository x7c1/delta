use jiff::Timestamp;

mod checked_sub_days;
mod epoch_millis;
mod from_epoch;
mod from_system_time;
mod to_rfc3339;
mod utc_date;

/// A point in time, in UTC, with at least millisecond precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Timestamp);

impl Instant {
    /// The Unix epoch, `1970-01-01T00:00:00Z`.
    pub const UNIX_EPOCH: Self = Self(Timestamp::UNIX_EPOCH);

    /// The current time from the system clock.
    pub fn now() -> Self {
        Self(Timestamp::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_after_the_epoch() {
        assert!(Instant::now().epoch_millis() > 0);
    }
}
