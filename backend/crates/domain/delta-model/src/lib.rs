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
mod error;
mod ids;
mod message;
mod pending_send;
mod permission;
mod thread;

pub use content::ContentBlock;
pub use error::{Error, Result};
pub use ids::{MessageUuid, PromptId, SessionId, ThreadId};
pub use message::{Message, Role};
pub use pending_send::{PendingSend, PendingSendStatus};
pub use permission::{PermissionRequest, PermissionStatus};
pub use thread::{Session, SessionStatus, Thread};
