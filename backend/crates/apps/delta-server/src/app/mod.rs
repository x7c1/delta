//! Router assembly: the composition root that binds a handler to every
//! endpoint `delta-wire` declares.
//!
//! What each endpoint is for — and which wire shapes it speaks — is documented
//! at the declaration in [`delta_wire::endpoint`], so this file stays a list of
//! bindings, with `RouteBinder` rejecting any drift between the two.

use axum::Router;

use delta_wire::endpoint;

use crate::api;
use crate::comms;
use crate::hooks;
use crate::pty;
use crate::route_binder::RouteBinder;
use crate::state::AppState;
use crate::ws;

/// Build the application router with all routes wired to shared state.
///
/// # Panics
///
/// If the bound routes are not exactly the declared ones — see `RouteBinder`.
pub fn router(state: AppState) -> Router {
    RouteBinder::new()
        .bind(endpoint::Health, health)
        // Control plane: Claude Code HTTP hooks.
        .bind(endpoint::HookUserPromptSubmit, hooks::user_prompt_submit)
        .bind(endpoint::HookStop, hooks::stop)
        .bind(endpoint::HookMessageDisplay, hooks::message_display)
        .bind(endpoint::HookPreToolUse, hooks::pre_tool_use)
        .bind(endpoint::HookPostToolUse, hooks::post_tool_use)
        .bind(endpoint::HookPermissionRequest, hooks::permission_request)
        .bind(endpoint::HookSessionStart, hooks::session_start)
        .bind(endpoint::HookSessionEnd, hooks::session_end)
        .bind(endpoint::HookStatusLine, hooks::status_line)
        // Browser REST surface: queries and commands.
        .bind(endpoint::ListSessions, api::list_sessions)
        .bind(endpoint::CreateSession, api::create_session)
        .bind(endpoint::OpenSession, api::open_session)
        .bind(endpoint::CloseSession, api::close_session)
        .bind(endpoint::InterruptSession, api::interrupt)
        .bind(endpoint::ListThreads, api::list_threads)
        .bind(endpoint::ListSends, api::list_sends)
        .bind(endpoint::ListThreadMessages, api::thread_messages)
        .bind(endpoint::CreateSend, api::create_send)
        .bind(endpoint::CancelSend, api::cancel_send)
        .bind(endpoint::ReleaseSend, api::release_send)
        .bind(endpoint::DecidePermission, api::decide_permission)
        .bind(endpoint::AnswerQuestion, api::answer_question)
        .bind(endpoint::CancelQuestion, api::cancel_question)
        .bind(endpoint::ListWorkdir, api::list_workdir)
        .bind(endpoint::RecentWorkdir, api::recent_workdir)
        .bind(endpoint::WorkdirGit, api::workdir_git)
        .bind(endpoint::WorkdirGitBranches, api::workdir_git_branches)
        .bind(endpoint::OpenCwd, api::open_cwd)
        .bind(endpoint::ListRepositories, api::list_repositories)
        .bind(endpoint::CloneRepository, api::clone_repository)
        .bind(endpoint::ListCloneRoots, api::list_clone_roots)
        .bind(endpoint::CreateCloneRoot, api::create_clone_root)
        .bind(endpoint::DeleteCloneRoot, api::delete_clone_root)
        .bind(endpoint::ListPullRequests, api::list_pull_requests)
        .bind(endpoint::ListProviders, api::list_providers)
        .bind(endpoint::ListLaunchOptions, api::list_launch_options)
        .bind(endpoint::CreateLaunchOption, api::create_launch_option)
        .bind(endpoint::UpdateLaunchOption, api::update_launch_option)
        .bind(endpoint::DeleteLaunchOption, api::delete_launch_option)
        .bind(endpoint::ListPromptTemplates, api::list_prompt_templates)
        .bind(endpoint::CreatePromptTemplate, api::create_prompt_template)
        .bind(endpoint::UpdatePromptTemplate, api::update_prompt_template)
        .bind(endpoint::DeletePromptTemplate, api::delete_prompt_template)
        .bind(endpoint::GetVersion, api::get_version)
        // Streams.
        .bind(endpoint::SessionEventStream, ws::ws_handler)
        .bind(endpoint::PtyStream, pty::pty_handler)
        .bind(endpoint::CommsStream, comms::comms_handler)
        .finish(state)
}

async fn health() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests;
