//! Application use cases for Delta.
//!
//! This crate defines the capability traits that the outside world must
//! provide — [`TmuxDriver`] to drive the session, [`Transcript`] to read it,
//! and [`SessionStore`] to persist Delta's thread overlay — and the
//! [`Interactor`] that orchestrates them into use cases.
//!
//! It depends only on [`delta_model`]. The concrete implementations live in the
//! gateway crates; the composition root wires them together.

mod error;
mod interactor;
mod ports;

pub use error::{Error, Result};
pub use interactor::{BoxedInteractor, Interactor};
pub use ports::{
    NewSession, SessionEvent, SessionStore, StopHook, TmuxDriver, Transcript,
    TranscriptMessage, UserPromptSubmitHook,
};

// Re-export the domain types the transport layer needs, so the server can
// depend on the use-case surface without reaching across to delta-model for
// these identifiers and value types.
pub use delta_model::{
    Message, MessageUuid, PendingSend, Session, SessionId, Thread, ThreadId,
};
