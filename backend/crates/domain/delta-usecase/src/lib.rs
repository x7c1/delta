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
mod clone;
mod erase_report;
mod error;
mod interactor;
mod launch_config;
mod launch_trust;
mod pane_token;
mod pane_token_minter;
mod ports;
mod pull_request;
mod pull_request_lens;
mod pull_request_list;
mod release_check;
mod release_update;
mod repository;
mod send_target;
mod session_listing;
mod session_page;
mod session_page_cursor;
mod session_prune;
mod session_removal;
mod session_settings_file;
mod turn;
mod with_causes;
mod worktree_branch;
mod worktree_dir;
mod worktree_spec;

pub use agent::{
    AgentAdapter, AgentAdapterFactory, AgentCapabilities, AgentContentSource, AgentEvent,
    AgentEventStream, AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind,
    AgentPermissionRequest, AgentProvider, AgentSessionHandle, AgentTokenUsage,
    ContentSourceRequest, ContextInjectionCapability, EventCapability, InterruptCapability,
    LaunchCapability, LaunchOptionCardinality, LaunchOptionSpec, LaunchOptionVocabulary,
    LaunchRequest, NullContentSource, NullLaunchOptionVocabulary, PermissionCapability, PtyHandle,
    ResumeCapability, ResumeRequest, SendReceipt, SendRequest, SessionEndReason,
    SessionIdentityCapability, SessionScopedAllowCapability, TerminalCapability,
    TranscriptCapability, TurnStatus,
};
pub use clone::Clone;
pub use erase_report::EraseReport;
pub use error::{Error, Result};
pub use interactor::{
    AttachablePane, BoxedInteractor, CoreRelease, ExternalHandler, ExternalHandlerId,
    HookEndpointChange, Interactor, PendingPermission, PendingQuestion, PermissionDecision,
    PermissionWait, ReadoptionSummary, RunningSubagent, SessionLiveState, VSCODE_HANDLER_ID,
};
pub use launch_config::{
    LaunchConfig, DEFAULT_SESSION_COMMAND, ECHO_DEADLINE, LAUNCH_PREP_DEADLINE,
    PENDING_SPAWN_DEADLINE, PERMISSION_DECISION_DEADLINE, RESUME_READY_DEADLINE,
    SLASH_COMMAND_ECHO_DEADLINE,
};
pub use launch_trust::{AgentConfigChange, LaunchTrustCheck, MAX_DIFFED_PATHS, MAX_DIFF_BYTES};
pub use pane_token::PaneToken;
pub use pane_token_minter::PaneTokenMinter;
pub use ports::{
    pane_for, AssetDownloadError, AssetDownloader, AsyncEventReceiver, AsyncEventSink,
    BinaryDetector, BranchDeletion, CloneRoot, CloneRootStore, CommsDirection, CommsEntry,
    CommsFrameKind, CommsLogSink, DirEntry, DirListing, DownloadProgress, ExternalOpener,
    FeedCause, GhCli, GitRepoInfo, GitWorktree, InstallError, InstalledApp, LaunchOptionStore,
    MessageDisplayHook, MessageStore, NewSession, NullCommsLog, PermissionStore,
    PromptTemplateStore, PublishedRelease, RateLimitWindow, RecentWorkdir, ReleaseAsset,
    ReleaseFeed, ReleaseFeedError, RememberedPane, RememberedPaneStore, RemoteBranches,
    RepositoryCloneRow, RunningSubagentLaunch, SendStore, SessionEndHook, SessionEvent,
    SessionHistoryStore, SessionPageRow, SessionRecordStore, SessionStartHook, SessionStore,
    SignedAsset, SpawningSession, StatusSnapshot, StopHook, SubagentLaunchStore, ThreadStore,
    TmuxDriver, Transcript, TranscriptMessage, TranscriptRead, UpdateInstaller,
    UserPromptSubmitHook, Workspace, WorktreeInspection, WorktreeRemoval, WorktreeStartPoint,
};
pub use pull_request::PullRequest;
pub use pull_request_lens::PullRequestLens;
pub use pull_request_list::PullRequestList;
pub use release_check::{NewerRelease, ReleaseCheck, ReleaseCheckError, RELEASE_PAGE_PREFIX};
pub use release_update::{
    check_download_url, choose_asset, downloadable_asset, signed_asset, BuildOrigin,
    InstallUnavailable, Launcher, ManualInstall, NotOffered, Platform, ReleaseUpdate,
    UpdateDownload, UpdateInstall, UpdateOffer, UpdateRefusal, UpdateRestart,
    RELEASE_DOWNLOAD_PREFIX,
};
pub use repository::{display_name, identity_key, worktree_dir_slug, Repository};
pub use send_target::SendTarget;
pub use session_listing::SessionListing;
pub use session_page::SessionPage;
pub use session_page_cursor::SessionPageCursor;
pub use session_prune::{
    PruneCriteria, PruneReport, PruneStatus, SessionKeptItem, SkipReason, SkippedSession,
};
pub use session_removal::{DiskItem, KeepReason, KeptItem, SessionRemoval};
pub use session_settings_file::SessionSettingsFile;
pub use turn::{
    transition, turn_input_for_agent_event, OrphanedSend, Transition, TurnInput, TurnState,
};
pub use with_causes::with_causes;
pub use worktree_branch::WorktreeBranch;
pub use worktree_dir::WorktreeDir;
pub use worktree_spec::WorktreeSpec;

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
