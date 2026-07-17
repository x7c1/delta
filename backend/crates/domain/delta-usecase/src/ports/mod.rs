//! Capability traits (ports) and the data they exchange.
//!
//! These traits describe what Delta needs from the outside world. The gateway
//! crates implement them; the [`crate::Interactor`] consumes them. Everything
//! here is expressed in terms of [`delta_model`] types only.

mod async_event_sink;
pub use async_event_sink::{AsyncEventReceiver, AsyncEventSink};
mod binary_detector;
pub use binary_detector::BinaryDetector;
mod dir_listing;
pub use dir_listing::{DirEntry, DirListing};
mod external_opener;
pub use external_opener::ExternalOpener;
mod gh_cli;
pub use gh_cli::GhCli;
mod git_worktree;
pub use git_worktree::{GitRepoInfo, GitWorktree, RemoteBranches, WorktreeStartPoint};
mod message_display_hook;
pub use message_display_hook::MessageDisplayHook;
mod new_session;
pub use new_session::NewSession;
mod session_end_hook;
pub use session_end_hook::SessionEndHook;
mod session_start_hook;
pub use session_start_hook::SessionStartHook;
mod session_event;
pub use session_event::{RateLimitWindow, SessionEvent, StatusSnapshot};
mod session_lifecycle;
pub use session_lifecycle::SessionLifecycle;
mod session_store;
pub use session_store::{
    RecentWorkdir, RepositoryCloneRow, RepositoryScanRoot, SessionPageRow, SessionStore,
};
mod stop_hook;
pub use stop_hook::StopHook;
mod tmux_driver;
pub use tmux_driver::{pane_for, TmuxDriver};
mod transcript;
pub use transcript::Transcript;
// The parsed-line type lives in `delta-attribution` (it is the pure fold's
// input); re-exported here so the gateway keeps implementing the
// [`Transcript`] port against `delta_usecase` types only.
pub use delta_attribution::TranscriptMessage;
mod transcript_read;
pub use transcript_read::TranscriptRead;
mod user_prompt_submit_hook;
pub use user_prompt_submit_hook::UserPromptSubmitHook;
mod workspace;
pub use workspace::Workspace;
