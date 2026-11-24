use std::time::SystemTime;

use jiff::Timestamp;

use super::Instant;

impl Instant {
    /// The instant a [`SystemTime`] (a file's modification time, say) names,
    /// or `None` when it falls outside the supported range (years -9999 to
    /// 9999).
    pub fn from_system_time(time: SystemTime) -> Option<Self> {
        Timestamp::try_from(time).ok().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn a_system_time_converts_to_the_same_instant() {
        let time = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(
            Instant::from_system_time(time),
            Instant::from_epoch_millis(1_700_000_000_123)
        );
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert_eq!(
            Instant::from_system_time(before),
            Instant::from_epoch_secs(-1)
        );
    }
}
