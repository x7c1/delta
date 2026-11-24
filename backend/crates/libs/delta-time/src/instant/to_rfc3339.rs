use super::Instant;

impl Instant {
    /// The RFC 3339 form in UTC with exactly three fractional digits and a
    /// `Z` suffix, e.g. `2026-03-07T01:02:03.004Z`. Every field is
    /// zero-padded, so for years 0 to 9999 two of these strings order as
    /// text the way their instants order in time.
    pub fn to_rfc3339_millis(self) -> String {
        format!("{:.3}", self.0)
    }

    /// The RFC 3339 form in UTC with whole seconds and a `Z` suffix, e.g.
    /// `2026-03-07T01:02:03Z`; any fraction of a second is dropped. Every
    /// field is zero-padded, so for years 0 to 9999 two of these strings
    /// order as text the way their instants, cut to the second, order in
    /// time.
    pub fn to_rfc3339_secs(self) -> String {
        format!("{:.0}", self.0)
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
    fn rfc3339_secs_pads_every_field_and_has_no_fraction() {
        let instant = Instant::from_epoch_millis(1_772_845_323_004).unwrap();
        assert_eq!(instant.to_rfc3339_secs(), "2026-03-07T01:02:03Z");
        assert_eq!(
            Instant::UNIX_EPOCH.to_rfc3339_secs(),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn rfc3339_secs_drops_the_fraction_rather_than_rounding() {
        let instant = Instant::from_epoch_millis(1_700_000_000_999).unwrap();
        assert_eq!(instant.to_rfc3339_secs(), "2023-11-14T22:13:20Z");
        // Before the epoch, the second it falls within.
        let before = Instant::from_epoch_millis(-1).unwrap();
        assert_eq!(before.to_rfc3339_secs(), "1969-12-31T23:59:59Z");
    }
}
