//! SQLite-backed [`SessionStore`] implementation.
//!
//! [`SqliteStore`] owns Delta's schema and persists the thread overlay: the
//! session row, threads, the message cache, the send FIFO and permission
//! history. The schema is applied as a migration when the store is opened.
//!
//! SQLite runs in-process and Delta wraps a single local session, so the
//! connection is guarded by an async mutex and queries run inline. The amount
//! of data is small and there is no cross-process contention.

mod content_record;
mod error;
mod schema;
mod store;
mod time;

pub use error::{Error, Result};
pub use schema::SCHEMA_VERSION;
pub use store::SqliteStore;
