//! Browser-facing wire contract.
//!
//! This crate owns the JSON shapes the server exchanges with the browser and
//! the TypeScript bindings generated from them. The domain layer stays
//! serialization-free: every `Wire*` type here mirrors a domain type
//! field-for-field (or variant-for-variant) and is the only form the transport
//! layer serializes.
//!
//! - [`WireSessionEvent`] is the `/ws` stream contract.
//! - [`WireCommsFrame`] is the `/comms` stream contract: the per-session
//!   observability log of the frames Delta exchanges with a headless provider.
//! - The [`rest`] module owns the `/api/*` request and response shapes,
//!   composed from the wire twins of the domain records ([`WireSession`],
//!   [`WireThread`], [`WireMessage`], [`WireSend`], …).
//! - The [`hooks`] module owns the Claude Code hook payloads (`/hooks/*`),
//!   which are never exported to TypeScript (see its module docs).
//! - The [`endpoint`] module owns the inventory of routes those shapes travel
//!   over, which is what makes this crate the whole contract rather than half
//!   of it: the server mounts its handlers through that table and refuses to
//!   boot on any disagreement with it.
//!
//! The `export-ts` binary (see `src/bin/export-ts.rs`) writes the TypeScript
//! types into the frontend's `@delta/wire-gen` package, so the browser types
//! can never drift from the Rust contract.

mod comms_direction;
pub use comms_direction::WireCommsDirection;
mod comms_frame;
pub use comms_frame::WireCommsFrame;
mod comms_frame_kind;
pub use comms_frame_kind::WireCommsFrameKind;
mod content_block;
pub use content_block::WireContentBlock;
pub mod endpoint;
mod file_change;
pub use file_change::WireFileChange;
mod file_change_detail;
pub use file_change_detail::WireFileChangeDetail;
mod file_change_kind;
pub use file_change_kind::WireFileChangeKind;
pub mod hooks;
mod message;
pub use message::WireMessage;
mod role;
pub use role::WireRole;
mod send;
pub use send::WireSend;
mod send_status;
pub use send_status::WireSendStatus;
mod agent_provider;
pub mod rest;
pub use agent_provider::WireAgentProvider;
mod session;
pub use session::WireSession;
mod session_status;
pub use session_status::WireSessionStatus;
mod session_event;
pub use session_event::{event_kinds, WireSessionEvent};
mod status_snapshot;
pub use status_snapshot::WireStatusSnapshot;
mod rate_limit_window;
pub use rate_limit_window::WireRateLimitWindow;
mod thread;
pub use thread::WireThread;

use ts_rs::Config;

/// The ts-rs configuration every export must use.
///
/// All integer ids on this wire are JavaScript-safe (`i64` row ids minted by
/// SQLite), so they are exported as `number` rather than ts-rs's default
/// `bigint`, matching what `serde_json` puts on the wire.
pub fn export_config() -> Config {
    Config::new().with_large_int("number")
}
