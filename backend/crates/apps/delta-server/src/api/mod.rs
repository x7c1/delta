//! Browser REST surface.
//!
//! The browser talks to the server over a REST + WebSocket hybrid: queries and
//! commands go through these `/api/*` routes (so they are easy to mock on the
//! frontend), live deltas arrive over `/ws`, and the terminal is bridged over
//! `/pty`. Every handler maps onto the use-case [`Interactor`]; errors are
//! converted to HTTP responses through a single [`ApiError`] mapping.
//!
//! The surface is multi-session: sessions are listed, created, opened, and
//! closed by id, and threads and sends are routed to a specific session rather
//! than an implicit "current" one.
//!
//! The JSON shapes themselves live in the `delta_wire` crate (its [`rest`]
//! module), which also generates the frontend's TypeScript bindings. Handlers
//! convert at this boundary: domain values in and out of the use cases, wire
//! types on the HTTP surface.
//!
//! [`Interactor`]: delta_usecase::Interactor
//! [`rest`]: delta_wire::rest

mod api_error;
pub(crate) use api_error::ApiError;
pub(crate) mod clone_root_path;
mod clone_roots;
pub(crate) use clone_roots::{create_clone_root, delete_clone_root, list_clone_roots};
mod latest_release;
pub(crate) use latest_release::{
    download_latest_release, get_latest_release, install_latest_release, restart_latest_release,
};
mod launch_options;
pub(crate) use launch_options::{
    create_launch_option, delete_launch_option, list_launch_options, update_launch_option,
};
mod open_cwd;
pub(crate) use open_cwd::open_cwd;
mod permissions;
pub(crate) use permissions::decide_permission;
mod prompt_templates;
pub(crate) use prompt_templates::{
    create_prompt_template, delete_prompt_template, list_prompt_templates, update_prompt_template,
};
mod providers;
pub(crate) use providers::list_providers;
mod pull_requests;
pub(crate) use pull_requests::list_pull_requests;
mod questions;
pub(crate) use questions::{answer_question, cancel_question};
mod repositories;
pub(crate) use repositories::{clone_repository, list_repositories};
mod sends;
pub(crate) use sends::{cancel_send, create_send, release_send};
mod session_cursor;
mod session_prune;
pub(crate) use session_prune::{preview_prune_sessions, prune_sessions};
mod sessions;
pub(crate) use sessions::{
    close_session, create_session, delete_session, interrupt, list_sends, list_sessions,
    list_threads, open_session, thread_messages,
};
mod storage;
pub(crate) use storage::get_storage;
mod storage_erase;
pub(crate) use storage_erase::erase_everything;
mod storage_snapshots;
pub(crate) use storage_snapshots::delete_snapshot;
mod storage_worktrees;
pub(crate) use storage_worktrees::{list_orphan_worktrees, remove_orphan_worktree};
mod version;
pub(crate) use version::get_version;
mod workdir;
pub(crate) use workdir::{list_workdir, recent_workdir};
mod workdir_git;
pub(crate) use workdir_git::{workdir_git, workdir_git_branches};
