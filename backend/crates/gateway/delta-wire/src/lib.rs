//! Browser-facing wire contract.
//!
//! This crate owns the JSON shapes the server sends to the browser and the
//! TypeScript bindings generated from them. The domain layer stays
//! serialization-free: [`WireSessionEvent`] mirrors the domain
//! [`SessionEvent`](delta_usecase::SessionEvent) variant-for-variant and is the
//! only type the WebSocket pump serializes.
//!
//! The `export-ts` binary (see `src/bin/export_ts.rs`) writes the TypeScript
//! union and the `EVENT_KINDS` const into the frontend's `@delta/wire-gen`
//! package, so the browser types can never drift from the Rust contract.

mod session_event;

pub use session_event::{event_kinds, WireSessionEvent};

use ts_rs::Config;

/// The ts-rs configuration every export must use.
///
/// All integer ids on this wire are JavaScript-safe (`i64` row ids minted by
/// SQLite), so they are exported as `number` rather than ts-rs's default
/// `bigint`, matching what `serde_json` puts on the wire.
pub fn export_config() -> Config {
    Config::new().with_large_int("number")
}
