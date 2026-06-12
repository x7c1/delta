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
mod launch_config;
mod open_sessions;
mod pane_token;
mod ports;
mod send_target;
mod session_listing;
mod session_page;
mod turn;

pub use error::{Error, Result};
pub use interactor::{BoxedInteractor, Interactor, PermissionDecision, PermissionWait};
pub use launch_config::{LaunchConfig, DEFAULT_SESSION_COMMAND};
pub use open_sessions::{OpenHandle, OpenSessions, PendingSpawn};
pub use pane_token::{PaneToken, PaneTokenMinter};
pub use ports::{
    pane_for, DirEntry, DirListing, NewSession, RecentWorkdir, SessionEndHook, SessionEvent,
    SessionLifecycle, SessionPageRow, SessionStartHook, SessionStore, StopHook, TmuxDriver,
    Transcript, TranscriptMessage, TranscriptRead, UserPromptSubmitHook, Workspace,
};
pub use send_target::SendTarget;
pub use session_listing::SessionListing;
pub use session_page::{SessionPage, SessionPageCursor};
pub use turn::{transition, OrphanedSend, Transition, TurnInput, TurnRegistry, TurnState};

// Re-export the domain types the transport layer needs, so the server can
// depend on the use-case surface without reaching across to delta-model for
// these identifiers and value types.
pub use delta_model::{Message, MessageUuid, Send, Session, SessionId, Thread, ThreadId};
