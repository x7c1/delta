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

mod agent_provider;
pub use agent_provider::AgentProvider;
mod content;
pub use content::ContentBlock;
mod error;
pub use error::{Error, Result};
mod launch_option;
pub use launch_option::LaunchOption;
mod launch_option_preset;
pub use launch_option_preset::LaunchOptionPreset;
mod message;
pub use message::Message;
mod message_uuid;
pub use message_uuid::MessageUuid;
mod newtype;
mod send;
pub use send::Send;
mod send_status;
pub use send_status::SendStatus;
mod permission_request;
pub use permission_request::PermissionRequest;
mod permission_status;
pub use permission_status::PermissionStatus;
mod prompt_id;
pub use prompt_id::PromptId;
mod prompt_template;
pub use prompt_template::PromptTemplate;
mod provider_availability;
pub use provider_availability::ProviderAvailability;
mod role;
pub use role::Role;
mod session;
pub use session::Session;
mod session_id;
pub use session_id::SessionId;
mod session_status;
pub use session_status::SessionStatus;
mod thread;
pub use thread::Thread;
mod thread_id;
pub use thread_id::ThreadId;
mod timestamp;
pub use timestamp::iso8601_utc;
