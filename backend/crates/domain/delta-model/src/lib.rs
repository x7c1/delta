//! Pure domain types for Delta.
//!
//! Delta drives a single Claude Code TUI session and reconstructs a thread
//! graph over its transcript. This crate holds the value types that model that
//! domain: identifiers, messages, threads, the outgoing-send queue, and
//! permission requests. It performs no I/O, owns no async runtime, and knows
//! nothing about how the data is stored or transported.
//!
//! There is no serde here and no database mapping: how these types appear in
//! the JSONL transcript, in SQLite, or on the browser wire is owned by the
//! gateway crates' wire/record twins (`delta-transcript`, `delta-sqlite`,
//! `delta-wire`).

mod content;
pub use content::ContentBlock;
mod error;
pub use error::{Error, Result};
mod launch_option;
mod newtype;
pub use launch_option::LaunchOption;
mod message;
pub use message::{Message, MessageUuid, PromptId};
mod send;
pub use send::Send;
mod send_status;
pub use send_status::SendStatus;
mod permission_request;
pub use permission_request::PermissionRequest;
mod permission_status;
pub use permission_status::PermissionStatus;
mod role;
pub use role::Role;
mod session;
pub use session::{Session, SessionId};
mod session_status;
pub use session_status::SessionStatus;
mod thread;
pub use thread::{Thread, ThreadId};
