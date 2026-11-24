use super::Instant;

impl Instant {
    /// Milliseconds since the Unix epoch, truncated toward the epoch.
    pub fn epoch_millis(self) -> i64 {
        self.0.as_millisecond()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_millis_round_trips() {
        let instant = Instant::from_epoch_millis(1_700_000_000_123).unwrap();
        assert_eq!(instant.epoch_millis(), 1_700_000_000_123);
    }
}
