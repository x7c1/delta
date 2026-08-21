//! SQLite-backed [`SessionStore`] implementation.
//!
//! [`SqliteStore`] owns Delta's schema and persists the thread overlay: the
//! session row, threads, the message cache, the send FIFO and permission
//! history. The schema is defined by the migration ladder in [`migrations`] and
//! brought up to date when the store is opened — a fresh database replays the
//! whole ladder, an existing one only the steps above its stamped version.
//!
//! SQLite runs in-process and Delta wraps a single local session, so the
//! connection is guarded by an async mutex and queries run inline. The amount
//! of data is small and there is no cross-process contention.

mod content_record;
mod error;
mod migrations;
mod store;
mod time;

pub use error::{Error, Result};
pub use migrations::SCHEMA_VERSION;
pub use store::SqliteStore;
