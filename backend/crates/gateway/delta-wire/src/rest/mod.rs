//! Request and response shapes of the browser REST surface (`/api/*`).
//!
//! Each module owns the wire form of one endpoint's payloads, converting
//! to/from the domain types at the boundary: responses are built `From` the
//! domain values the use cases return, and the one request body that flows
//! inward ([`WireCreateSendRequest`]) resolves into a domain
//! [`SendTarget`](delta_usecase::SendTarget). All of these are exported to
//! TypeScript by the `export-ts` binary, so the browser types can never drift
//! from the Rust contract.

mod clone_repository_request;
pub use clone_repository_request::WireCloneRepositoryRequest;
mod clone_root_create_request;
pub use clone_root_create_request::WireCreateCloneRootRequest;
mod clone_roots_response;
pub use clone_roots_response::{WireCloneRoot, WireCloneRootsResponse};
mod delete_snapshot_request;
pub use delete_snapshot_request::WireDeleteSnapshotRequest;
mod disk_item_kind;
pub use disk_item_kind::WireDiskItemKind;
mod erase_response;
pub use erase_response::WireEraseResponse;
mod erased_items;
pub use erased_items::WireErasedItems;
mod error_body;
pub use error_body::WireErrorBody;
mod git_response;
pub use git_response::{WireGitBranchesResponse, WireGitRepoResponse};
mod keep_reason;
pub use keep_reason::WireKeepReason;
mod kept_item;
pub use kept_item::WireKeptItem;
mod install_unavailable;
pub use install_unavailable::WireInstallUnavailable;
mod latest_release_response;
pub use latest_release_response::{WireLatestReleaseResponse, WireNewerRelease};
mod launch_option_create_request;
pub use launch_option_create_request::WireCreateLaunchOptionRequest;
mod launch_option_update_request;
pub use launch_option_update_request::WireUpdateLaunchOptionRequest;
mod launch_options_response;
pub use launch_options_response::{WireLaunchOption, WireLaunchOptionsResponse};
mod manual_install;
pub use manual_install::WireManualInstall;
mod messages_response;
pub use messages_response::WireMessagesResponse;
mod new_session_response;
pub use new_session_response::{WireNewSessionResponse, WireSessionLifecycle};
mod open_cwd_request;
pub use open_cwd_request::WireOpenCwdRequest;
mod permission_decision_request;
pub use permission_decision_request::{WirePermissionDecision, WirePermissionDecisionRequest};
mod prompt_template_create_request;
pub use prompt_template_create_request::WireCreatePromptTemplateRequest;
mod prompt_template_update_request;
pub use prompt_template_update_request::WireUpdatePromptTemplateRequest;
mod prompt_templates_response;
pub use prompt_templates_response::{WirePromptTemplate, WirePromptTemplatesResponse};
mod providers_response;
pub use providers_response::{
    WireLaunchOptionStyle, WireProviderAvailability, WireProviderCapabilities,
    WireProvidersResponse,
};
mod prune_preview_response;
pub use prune_preview_response::WirePrunePreviewResponse;
mod prune_sessions_request;
pub use prune_sessions_request::WirePruneSessionsRequest;
mod prune_sessions_response;
pub use prune_sessions_response::WirePruneSessionsResponse;
mod prune_status;
pub use prune_status::WirePruneStatus;
mod pull_requests_response;
pub use pull_requests_response::{WirePullRequest, WirePullRequestsResponse};
mod question_answer_request;
pub use question_answer_request::WireQuestionAnswerRequest;
mod question_cancel_request;
pub use question_cancel_request::WireQuestionCancelRequest;
mod remove_worktree_request;
pub use remove_worktree_request::WireRemoveWorktreeRequest;
mod repositories_response;
pub use repositories_response::{
    WireRepositoriesResponse, WireRepositoryClone, WireRepositoryEntry,
};
mod send_request;
pub use send_request::{SendTargetError, WireCreateSendRequest};
mod send_response;
pub use send_response::WireSendResponse;
mod sends_response;
pub use sends_response::{
    WirePendingPermission, WirePendingQuestion, WireSendsResponse, WireTurn, WireTurnPhase,
};
mod sessions_response;
pub use sessions_response::{WireSessionListItem, WireSessionsResponse};
mod skip_reason;
pub use skip_reason::WireSkipReason;
mod skipped_session;
pub use skipped_session::WireSkippedSession;
mod storage_file;
pub use storage_file::WireStorageFile;
mod storage_response;
pub use storage_response::WireStorageResponse;
mod storage_worktree;
pub use storage_worktree::WireStorageWorktree;
mod storage_worktrees_response;
pub use storage_worktrees_response::WireStorageWorktreesResponse;
mod threads_response;
pub use threads_response::WireThreadsResponse;
mod update_download;
pub use update_download::WireUpdateDownload;
mod update_install;
pub use update_install::WireUpdateInstall;
mod update_offer;
pub use update_offer::WireUpdateOffer;
mod version_response;
pub use version_response::WireVersionResponse;
mod workdir_list_response;
pub use workdir_list_response::{WireWorkdirEntry, WireWorkdirListResponse};
mod workdir_recent_response;
pub use workdir_recent_response::{WireRecentWorkdirItem, WireWorkdirRecentResponse};
mod worktree_spec;
pub use worktree_spec::{WireWorktreeSpec, WireWorktreeStartPoint};
