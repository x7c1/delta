//! What the end-to-end tests share: the fake gateways, the app they are
//! wired into, and the request helpers that drive it.

use std::time::Duration;

mod app;
pub(crate) use app::{build_app, build_app_with_store};
mod fake_tmux;
pub(crate) use fake_tmux::FakeTmux;
mod http;
pub(crate) use http::{delete, get, listed_session_ids, post_json};
mod noop_git_worktree;
use noop_git_worktree::NoopGitWorktree;
mod noop_workspace;
use noop_workspace::NoopWorkspace;
mod shared_tmux;
use shared_tmux::SharedTmux;

/// The bearer token this test's `AppState` holds, presented on every request the
/// helpers drive so they clear the router's per-run auth guard.
const AUTH_TOKEN: &str = "delta-e2e-auth-token";

/// The hook secret this test's `AppState` holds. `post_json` appends it as
/// `?hs=<secret>` to every `/hooks/*` request so the hook auth guard lets the
/// callback through, exactly as the rendered hook URLs carry it in production.
const HOOK_SECRET: &str = "delta-e2e-hook-secret";

/// The bound this test hands a background-tick fan-out. Far longer than the
/// in-process actors take, so the tick always ends because they replied — never
/// because the clock ran out on a loaded machine.
pub(crate) const TICK_BOUND: Duration = Duration::from_secs(30);
