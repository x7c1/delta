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
mod error_body;
pub use error_body::WireErrorBody;
mod git_response;
pub use git_response::{WireGitBranchesResponse, WireGitRepoResponse};
mod launch_option_create_request;
pub use launch_option_create_request::WireCreateLaunchOptionRequest;
mod launch_option_update_request;
pub use launch_option_update_request::WireUpdateLaunchOptionRequest;
mod launch_options_response;
pub use launch_options_response::{WireLaunchOption, WireLaunchOptionsResponse};
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
mod pull_requests_response;
pub use pull_requests_response::{WirePullRequest, WirePullRequestsResponse};
mod question_answer_request;
pub use question_answer_request::WireQuestionAnswerRequest;
mod question_cancel_request;
pub use question_cancel_request::WireQuestionCancelRequest;
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
mod threads_response;
pub use threads_response::WireThreadsResponse;
mod version_response;
pub use version_response::WireVersionResponse;
mod workdir_list_response;
pub use workdir_list_response::{WireWorkdirEntry, WireWorkdirListResponse};
mod workdir_recent_response;
pub use workdir_recent_response::{WireRecentWorkdirItem, WireWorkdirRecentResponse};
mod worktree_spec;
pub use worktree_spec::{WireWorktreeSpec, WireWorktreeStartPoint};
