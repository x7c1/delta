//! Application use cases for Delta.
//!
//! This crate defines the capability traits that the outside world must
//! provide — [`TmuxDriver`] to drive the session, [`Transcript`] to read it,
//! and [`SessionStore`] to persist Delta's thread overlay — and the
//! [`Interactor`] that orchestrates them into use cases.
//!
//! It depends only on [`delta_model`]. The concrete implementations live in the
//! gateway crates; the composition root wires them together.

mod agent;
mod erase_report;
mod error;
mod interactor;
mod launch_config;
mod pane_token;
mod ports;
mod pull_request;
mod release_check;
mod release_update;
mod repository;
mod send_target;
mod session_listing;
mod session_page;
mod session_prune;
mod session_removal;
mod turn;
mod worktree_dir;

pub use agent::{
    AgentAdapter, AgentAdapterFactory, AgentCapabilities, AgentContentSource, AgentEvent,
    AgentEventStream, AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind,
    AgentPermissionRequest, AgentProvider, AgentSessionHandle, AgentTokenUsage,
    ContentSourceRequest, ContextInjectionCapability, EventCapability, ForkCapability,
    InterruptCapability, LaunchCapability, LaunchOptionCardinality, LaunchOptionSpec,
    LaunchOptionVocabulary, LaunchRequest, NullLaunchOptionVocabulary, PermissionCapability,
    PtyHandle, ResumeCapability, ResumeRequest, SendReceipt, SendRequest, SessionEndReason,
    SessionIdentityCapability, SessionScopedAllowCapability, SteerCapability, TerminalCapability,
    TranscriptCapability, TurnStatus,
};
pub use erase_report::EraseReport;
pub use error::{Error, Result};
pub use interactor::{
    AttachablePane, BoxedInteractor, CoreRelease, ExternalHandler, ExternalHandlerId, Interactor,
    PendingPermission, PendingQuestion, PermissionDecision, PermissionWait, ReadoptionSummary,
    RunningSubagent, SessionLiveState, VSCODE_HANDLER_ID,
};
pub use launch_config::{LaunchConfig, DEFAULT_SESSION_COMMAND};
pub use pane_token::{PaneToken, PaneTokenMinter};
pub use ports::{
    pane_for, AssetDownloadError, AssetDownloader, AsyncEventReceiver, AsyncEventSink,
    BinaryDetector, BranchDeletion, CloneRoot, CommsDirection, CommsEntry, CommsFrameKind,
    CommsLogSink, DirEntry, DirListing, DownloadProgress, ExternalOpener, FeedCause, GhCli,
    GitRepoInfo, GitWorktree, InstallError, MessageDisplayHook, NewSession, NullCommsLog,
    PublishedRelease, RateLimitWindow, RecentWorkdir, ReleaseAsset, ReleaseFeed, ReleaseFeedError,
    RememberedPane, RemoteBranches, RepositoryCloneRow, SessionEndHook, SessionEvent,
    SessionLifecycle, SessionPageRow, SessionStartHook, SessionStore, SpawningSession,
    StatusSnapshot, StopHook, TmuxDriver, Transcript, TranscriptMessage, TranscriptRead,
    UpdateInstaller, UserPromptSubmitHook, Workspace, WorktreeInspection, WorktreeRemoval,
    WorktreeStartPoint,
};
pub use pull_request::{PullRequest, PullRequestLens, PullRequestList};
pub use release_check::{NewerRelease, ReleaseCheck, ReleaseCheckError, RELEASE_PAGE_PREFIX};
pub use release_update::{
    check_download_url, choose_asset, downloadable_asset, manual_install_command, BuildOrigin,
    Launcher, NotOffered, Platform, ReleaseUpdate, UpdateDownload, UpdateInstall, UpdateOffer,
    UpdateRefusal, RELEASE_DOWNLOAD_PREFIX,
};
pub use repository::{display_name, identity_key, worktree_dir_slug, Clone, Repository};
pub use send_target::{SendTarget, WorktreeSpec};
pub use session_listing::SessionListing;
pub use session_page::{SessionPage, SessionPageCursor};
pub use session_prune::{
    PruneCriteria, PruneReport, PruneStatus, SessionKeptItem, SkipReason, SkippedSession,
};
pub use session_removal::{DiskItem, KeepReason, KeptItem, SessionRemoval};
pub use turn::{
    transition, turn_input_for_agent_event, OrphanedSend, Transition, TurnInput, TurnState,
};
pub use worktree_dir::WorktreeDir;

// Re-export the domain types the transport layer needs, so the server can
// depend on the use-case surface without reaching across to delta-model for
// these identifiers and value types.
pub use delta_model::{
    LaunchOption, LaunchOptionPreset, Message, MessageUuid, PromptTemplate, ProviderAvailability,
    Send, Session, SessionId, Thread, ThreadId,
};

// Re-export the neutral persistence-pipeline effect type. It originates in
// `delta-attribution` (the pure fold's output), but an implementor of
// [`AgentContentSource`] should be able to name it through this crate's surface
// without a direct `delta-attribution` dependency — keeping the gateway's
// dependency direction (`codex-agent` → `delta-usecase`) clean.
pub use delta_attribution::Effect;
