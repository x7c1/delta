//! Dates and times for Delta's crates, as Delta's own types: [`Instant`], a
//! point in time in UTC, and [`Date`], a calendar date. Their string forms are fixed, because callers
//! store and compare them as text:
//!
//! - a [`Date`] displays as `YYYY-MM-DD`;
//! - an [`Instant`] renders as an RFC 3339 UTC string with millisecond
//!   precision and a `Z` suffix (`2026-03-07T01:02:03.004Z`) through
//!   [`Instant::to_rfc3339_millis`], or with whole seconds
//!   (`2026-03-07T01:02:03Z`) through [`Instant::to_rfc3339_secs`].
//!
//! The implementation is `jiff`, which no public signature exposes.

mod date;
pub use date::Date;

mod instant;
pub use instant::Instant;
