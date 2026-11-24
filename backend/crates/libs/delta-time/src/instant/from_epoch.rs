use jiff::Timestamp;

use super::Instant;

impl Instant {
    /// The instant `secs` seconds after the Unix epoch (negative for before
    /// it), or `None` when it falls outside the supported range (years -9999
    /// to 9999).
    pub fn from_epoch_secs(secs: i64) -> Option<Self> {
        Timestamp::from_second(secs).ok().map(Self)
    }

    /// The instant `millis` milliseconds after the Unix epoch (negative for
    /// before it), or `None` when it falls outside the supported range
    /// (years -9999 to 9999).
    pub fn from_epoch_millis(millis: i64) -> Option<Self> {
        Timestamp::from_millisecond(millis).ok().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_epoch_millis_is_none() {
        assert_eq!(Instant::from_epoch_millis(i64::MAX), None);
        assert_eq!(Instant::from_epoch_millis(i64::MIN), None);
    }

    #[test]
    fn epoch_secs_name_the_same_instant_as_epoch_millis() {
        assert_eq!(
            Instant::from_epoch_secs(1_609_459_200),
            Instant::from_epoch_millis(1_609_459_200_000)
        );
        assert_eq!(Instant::from_epoch_secs(0), Some(Instant::UNIX_EPOCH));
        assert_eq!(Instant::from_epoch_secs(i64::MAX), None);
    }
}
