//! Pure domain types for Delta.
//!
//! Delta drives a single Claude Code TUI session and reconstructs a thread
//! graph over its transcript. This crate holds the value types that model that
//! domain: identifiers, messages, threads, the outgoing-send queue, and
//! permission requests. It performs no I/O, owns no async runtime, and knows
//! nothing about how the data is stored or transported.
//!
//! `serde` derives are present so the same types can be reused for the JSONL
//! transcript and the browser wire format, but there is no database mapping
//! here — that belongs to the gateway crates.

mod content;
pub use content::ContentBlock;
mod error;
pub use error::{Error, Result};
mod ids;
pub use ids::{MessageUuid, PromptId, SessionId, ThreadId};
mod message;
pub use message::Message;
mod pending_send;
pub use pending_send::PendingSend;
mod pending_send_status;
pub use pending_send_status::PendingSendStatus;
mod permission_request;
pub use permission_request::PermissionRequest;
mod permission_status;
pub use permission_status::PermissionStatus;
mod role;
pub use role::Role;
mod session;
pub use session::Session;
mod session_status;
pub use session_status::SessionStatus;
mod thread;
pub use thread::Thread;
