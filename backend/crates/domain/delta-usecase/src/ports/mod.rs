//! Capability traits (ports) and the data they exchange.
//!
//! These traits describe what Delta needs from the outside world. The gateway
//! crates implement them; the [`crate::Interactor`] consumes them. Everything
//! here is expressed in terms of [`delta_model`] types only.

mod async_event_sink;
pub use async_event_sink::{AsyncEventReceiver, AsyncEventSink};
mod binary_detector;
pub use binary_detector::BinaryDetector;
mod branch_deletion;
pub use branch_deletion::BranchDeletion;
mod comms_direction;
pub use comms_direction::CommsDirection;
mod comms_entry;
pub use comms_entry::CommsEntry;
mod comms_frame_kind;
pub use comms_frame_kind::CommsFrameKind;
mod comms_log;
pub use comms_log::CommsLogSink;
mod dir_entry;
pub use dir_entry::DirEntry;
mod dir_listing;
pub use dir_listing::DirListing;
mod external_opener;
pub use external_opener::ExternalOpener;
mod gh_cli;
pub use gh_cli::GhCli;
mod git_repo_info;
pub use git_repo_info::GitRepoInfo;
mod git_worktree;
pub use git_worktree::GitWorktree;
mod installed_app;
pub use installed_app::InstalledApp;
mod message_display_hook;
pub use message_display_hook::MessageDisplayHook;
mod new_session;
pub use new_session::NewSession;
mod null_comms_log;
pub use null_comms_log::NullCommsLog;
mod asset_download_error;
pub use asset_download_error::AssetDownloadError;
mod asset_downloader;
pub use asset_downloader::AssetDownloader;
mod download_progress;
pub use download_progress::DownloadProgress;
mod published_release;
pub use published_release::PublishedRelease;
mod release_asset;
pub use release_asset::ReleaseAsset;
mod release_feed;
pub use release_feed::{FeedCause, ReleaseFeed};
mod release_feed_error;
pub use release_feed_error::ReleaseFeedError;
mod signed_asset;
pub use signed_asset::SignedAsset;
mod rate_limit_window;
pub use rate_limit_window::RateLimitWindow;
mod remembered_pane;
pub use remembered_pane::RememberedPane;
mod remote_branches;
pub use remote_branches::RemoteBranches;
mod session_end_hook;
pub use session_end_hook::SessionEndHook;
mod session_start_hook;
pub use session_start_hook::SessionStartHook;
mod session_event;
pub use session_event::SessionEvent;
mod session_store;
pub use session_store::{
    CloneRoot, CloneRootStore, LaunchOptionStore, MessageStore, PermissionStore,
    PromptTemplateStore, RecentWorkdir, RememberedPaneStore, RepositoryCloneRow, SendStore,
    SessionHistoryStore, SessionPageRow, SessionRecordStore, SessionStore, SubagentLaunchStore,
    ThreadStore,
};
mod spawning_session;
pub use spawning_session::SpawningSession;
mod status_snapshot;
pub use status_snapshot::StatusSnapshot;
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
mod install_error;
pub use install_error::InstallError;
mod update_installer;
pub use update_installer::UpdateInstaller;
mod user_prompt_submit_hook;
pub use user_prompt_submit_hook::UserPromptSubmitHook;
mod workspace;
pub use workspace::Workspace;
mod worktree_inspection;
pub use worktree_inspection::WorktreeInspection;
mod worktree_removal;
pub use worktree_removal::WorktreeRemoval;
mod worktree_start_point;
pub use worktree_start_point::WorktreeStartPoint;
