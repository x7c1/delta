//! [`Git`] tests, split by the behaviour they cover. Removal and inspection
//! keep theirs next to their own modules.

mod checkout;
mod command;
mod create_worktree;
mod fetch_remote_branches;
mod repository;
mod trust;

use super::testing::{canonical, git_ok, init_repo_with_commit};
use super::*;
