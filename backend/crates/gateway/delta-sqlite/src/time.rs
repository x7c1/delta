//! Timestamp helpers.
//!
//! SQLite has no native datetime type, so Delta stores timestamps as ISO-8601
//! text in the form [`delta_model::iso8601_utc`] produces, formatted from the
//! system clock.

use delta_time::Instant;

/// Current UTC time as an ISO-8601 `YYYY-MM-DDTHH:MM:SSZ` string.
pub fn now_iso8601() -> String {
    delta_model::iso8601_utc(Instant::now())
}
