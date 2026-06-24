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
mod pane_token;
mod ports;
mod pull_request;
mod repository;
mod send_target;
mod session_listing;
mod session_page;
mod turn;

pub use error::{Error, Result};
pub use interactor::{
    BoxedInteractor, Interactor, PendingPermission, PendingQuestion, PermissionDecision,
    PermissionWait, RunningSubagent, SessionLiveState,
};
pub use launch_config::{LaunchConfig, DEFAULT_SESSION_COMMAND};
pub use pane_token::{PaneToken, PaneTokenMinter};
pub use pull_request::{PullRequest, PullRequestLens, PullRequestList};
pub use repository::{display_name, identity_key, Clone, Repository};
pub use ports::{
    pane_for, DirEntry, DirListing, GhCli, GitRepoInfo, GitWorktree, MessageDisplayHook,
    NewSession, RateLimitWindow, RecentWorkdir, RemoteBranches, RepositoryCloneRow, SessionEndHook,
    SessionEvent, SessionLifecycle, SessionPageRow, SessionStartHook, SessionStore,
    StatusSnapshot, StopHook, TmuxDriver, Transcript, TranscriptMessage, TranscriptRead,
    UserPromptSubmitHook, Workspace, WorktreeStartPoint,
};
pub use send_target::{SendTarget, WorktreeSpec};
pub use session_listing::SessionListing;
pub use session_page::{SessionPage, SessionPageCursor};
pub use turn::{transition, OrphanedSend, Transition, TurnInput, TurnState};

// Re-export the domain types the transport layer needs, so the server can
// depend on the use-case surface without reaching across to delta-model for
// these identifiers and value types.
pub use delta_model::{
    LaunchOption, Message, MessageUuid, Send, Session, SessionId, Thread, ThreadId,
};
