//! Adding the session's real git directory to Codex's `workspace-write`
//! sandbox, for a session Delta launched in a worktree it created.
//!
//! Its own module for the same reason [`super::config_merge`] is: the grant is a
//! self-contained rewrite of the request's `config` object, with its own
//! vocabulary (sandbox table, writable roots, the two spellings of the leaf) and
//! its own way of failing, while [`super`] is about driving a live session. Why
//! the grant exists at all, and what the real server was measured to do with it,
//! is in [`super`]'s module docs under "The worktree git-directory grant"; this
//! module is only how the path gets into the request.

use serde_json::{json, Map, Value};

use super::CONFIG_FIELD;

/// Codex's sandbox table, as a nested config key: the table the worktree
/// git-directory grant reaches into.
///
/// `pub(super)` because [`super::config_merge`] names the same setting — it is
/// the one list a merge unions rather than refuses — and the two must agree on
/// its spelling.
pub(super) const SANDBOX_WORKSPACE_WRITE: &str = "sandbox_workspace_write";

/// The `writable_roots` leaf of [`SANDBOX_WORKSPACE_WRITE`], as spelled inside
/// the nested table.
pub(super) const WRITABLE_ROOTS_LEAF: &str = "writable_roots";

/// The dotted config key the worktree git-directory grant is injected on when
/// the config states no writable roots of its own — the `writable_roots` leaf of
/// [`SANDBOX_WORKSPACE_WRITE`].
pub(super) const WRITABLE_ROOTS_KEY: &str = "sandbox_workspace_write.writable_roots";

/// Grant the session's real git directory to Codex's `workspace-write` sandbox,
/// when the session runs in a Delta-created worktree.
///
/// A no-op without a `worktree_repo_root`: a session launched in a plain
/// directory is left byte-identical to what it was before this existed, because
/// whether the `.git` at a writable root's top level should be writable there is
/// the user's own global-config choice, not something Delta's worktree knowledge
/// bears on. With one, `<repo-root>/.git` — the directory git actually writes
/// through for the worktree — is added to the request's writable roots. See
/// [`super`]'s module docs for the empirical basis of the dotted spelling and of
/// what a leaf override does to the user's global list.
///
/// # Union, not replacement
///
/// The grant **appends** to whatever the merged `config` already says about
/// writable roots, in the spelling that config used:
///
/// - a stated `writable_roots` list — dotted [`WRITABLE_ROOTS_KEY`] or the leaf
///   inside a nested [`SANDBOX_WORKSPACE_WRITE`] table — gains one entry, unless
///   it already names the path;
/// - a nested sandbox table that states other keys but no roots gains the
///   `writable_roots` list;
/// - a config that says nothing about the sandbox (including no config at all,
///   the common case) gains the dotted [`WRITABLE_ROOTS_KEY`].
///
/// Appending rather than deferring is what [`super`]'s module docs'
/// leaf-replacement finding demands: the user's list is the whole list the
/// thread runs with, so a worktree session whose user config states one would
/// otherwise be the one session that keeps the approval prompts this grant
/// removes.
///
/// Every other key the user wrote is untouched: the grant is one added entry,
/// never a replacement.
///
/// # Standing aside
///
/// Two shapes leave nothing to append to, and are logged and left alone rather
/// than rewritten: a `config` that is not a JSON object (a launch option's value
/// is passed through unvalidated, so it can be any value the user typed), and a
/// `writable_roots` — or a `sandbox_workspace_write` — that is not the container
/// its name implies. The server is the authority on whether such a value is
/// legal; Delta neither guesses nor overwrites it.
pub(super) fn apply_worktree_git_grant(
    params: &mut Map<String, Value>,
    worktree_repo_root: Option<&str>,
) {
    let Some(repo_root) = worktree_repo_root else {
        return;
    };
    let git_dir = format!("{repo_root}/.git");
    let outcome = match params.get_mut(CONFIG_FIELD) {
        Some(Value::Object(config)) => grant_writable_root(config, &git_dir),
        // No `config` at all — the common case.
        None => {
            let mut config = Map::new();
            config.insert(WRITABLE_ROOTS_KEY.to_owned(), json!([git_dir]));
            params.insert(CONFIG_FIELD.to_owned(), Value::Object(config));
            Ok(())
        }
        Some(_) => Err(UngrantableConfig::NotAnObject),
    };
    if let Err(reason) = outcome {
        tracing::warn!(
            "codex-agent: not granting `{git_dir}` to the workspace-write sandbox \
             because {reason}; git writes inside the worktree may raise approval prompts"
        );
    }
}

/// Why the merged `config` left the worktree git grant nothing to add its path
/// to — one variant per shape that blocks it, rather than a sentence built at
/// the point of failure, so the three cases stay distinguishable in the code and
/// the wording lives in one place.
#[derive(Debug, thiserror::Error)]
enum UngrantableConfig {
    /// The selected `config` is not a JSON object, so it holds no keys to add a
    /// writable root among.
    #[error("the selected `config` launch option is not an object")]
    NotAnObject,

    /// `sandbox_workspace_write` is stated as something other than a table, so
    /// the grant cannot reach the leaf inside it.
    #[error(
        "the selected `config` states `{}` as something other than a table",
        SANDBOX_WORKSPACE_WRITE
    )]
    SandboxNotATable,

    /// `writable_roots` is stated as something other than a list, so there is no
    /// list to append the path to.
    #[error(
        "the selected `config` states `{}` as something other than a list",
        WRITABLE_ROOTS_KEY
    )]
    RootsNotAList,
}

/// Add `git_dir` to a `config` object's writable roots, in whichever spelling
/// that object already uses, or report why there was nothing to add it to.
fn grant_writable_root(
    config: &mut Map<String, Value>,
    git_dir: &str,
) -> Result<(), UngrantableConfig> {
    if let Some(roots) = config.get_mut(WRITABLE_ROOTS_KEY) {
        return push_writable_root(roots, git_dir);
    }
    if let Some(sandbox) = config.get_mut(SANDBOX_WORKSPACE_WRITE) {
        let Some(sandbox) = sandbox.as_object_mut() else {
            return Err(UngrantableConfig::SandboxNotATable);
        };
        return match sandbox.get_mut(WRITABLE_ROOTS_LEAF) {
            Some(roots) => push_writable_root(roots, git_dir),
            None => {
                sandbox.insert(WRITABLE_ROOTS_LEAF.to_owned(), json!([git_dir]));
                Ok(())
            }
        };
    }
    config.insert(WRITABLE_ROOTS_KEY.to_owned(), json!([git_dir]));
    Ok(())
}

/// Append `git_dir` to a stated writable-roots list, unless it is already there
/// (a user who listed the path themselves gets it once, not twice).
fn push_writable_root(roots: &mut Value, git_dir: &str) -> Result<(), UngrantableConfig> {
    let Some(roots) = roots.as_array_mut() else {
        return Err(UngrantableConfig::RootsNotAList);
    };
    let entry = Value::String(git_dir.to_owned());
    if !roots.contains(&entry) {
        roots.push(entry);
    }
    Ok(())
}
