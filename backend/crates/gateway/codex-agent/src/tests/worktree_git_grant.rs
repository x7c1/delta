//! The worktree git-directory grant.

use super::*;

/// A session launched in a Delta-created worktree grants that worktree's REAL
/// git directory — the source repository's `.git`, where git's writes actually
/// land — to Codex's `workspace-write` sandbox.
///
/// The worktree's own `<cwd>/.git` is deliberately NOT the granted path: it is
/// only a pointer file, so granting it would leave every `git add` in the
/// session escalating for approval exactly as before.
#[test]
fn a_worktree_launch_grants_the_source_repositorys_git_directory() {
    let params = thread_start_params("/worktrees/org-repo-01", &[], Some("/repos/org/repo"))
        .expect("no options to reject");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/repos/org/repo/.git"] },
        })
    );
}

/// A launch outside a Delta-created worktree is byte-identical to what it was
/// before the grant existed — no `config` appears, whether or not the user
/// selected options. Whether a plain clone's `.git` should be writable is the
/// user's own global-config choice; Delta only speaks for worktrees it made.
#[test]
fn a_non_worktree_launch_injects_nothing() {
    let bare = thread_start_params("/work", &[], None).expect("no options to reject");
    assert_eq!(bare, json!({ "cwd": "/work" }));

    let with_options = thread_start_params("/work", &[option("model", Some("gpt-5.6-sol"))], None)
        .expect("no options to reject");
    assert_eq!(
        with_options,
        json!({ "cwd": "/work", "model": "gpt-5.6-sol" })
    );
}

/// A resume re-supplies the same grant a fresh start got, so a worktree session
/// reattached after a restart is sandboxed exactly like it was launched.
#[test]
fn a_worktree_resume_grants_the_same_git_directory() {
    let params = thread_resume_params(
        "thr_worktree",
        "/worktrees/org-repo-01",
        Some("/repos/org/repo"),
    );

    assert_eq!(
        params,
        json!({
            "threadId": "thr_worktree",
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/repos/org/repo/.git"] },
        })
    );
}

/// Resuming a non-worktree session sends what it always sent: the thread and the
/// directory to reattach in.
#[test]
fn a_non_worktree_resume_injects_nothing() {
    let params = thread_resume_params("thr_plain", "/work", None);
    assert_eq!(params, json!({ "threadId": "thr_plain", "cwd": "/work" }));
}

/// A user-registered `config` that says nothing about the sandbox is **merged**
/// with, not replaced by, the grant: every key they registered survives and the
/// grant is added alongside.
#[test]
fn a_user_config_without_a_sandbox_key_is_merged_with_the_grant() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"tools":{"web_search":true},"model_reasoning_effort":"high"}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": {
                "tools": { "web_search": true },
                "model_reasoning_effort": "high",
                GRANT_KEY: ["/repos/org/repo/.git"],
            },
        })
    );
}

/// A user-registered `config` that lists its own writable roots **gains** the
/// worktree's git directory rather than suppressing the grant: on this path a
/// leaf override replaces the user's global list, so the list the thread runs
/// with is exactly this one — and Delta's path has to be in it.
///
/// The user's spelling is kept, dotted key here.
#[test]
fn a_dotted_writable_roots_gains_the_git_directory() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write.writable_roots":["/elsewhere"]}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: ["/elsewhere", "/repos/org/repo/.git"] },
        })
    );
}

/// The same union through the other spelling: a nested `sandbox_workspace_write`
/// table's `writable_roots` is appended to in place, so the config Codex
/// receives is still shaped the way the user wrote it.
#[test]
fn a_nested_writable_roots_gains_the_git_directory() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write":{"writable_roots":["/elsewhere"]}}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params["config"],
        json!({
            "sandbox_workspace_write": {
                "writable_roots": ["/elsewhere", "/repos/org/repo/.git"],
            },
        })
    );
}

/// A config that states the sandbox but no roots gains the list, inside the
/// table it already wrote — the other keys of that table are untouched.
#[test]
fn a_sandbox_table_without_roots_gains_the_list() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write":{"network_access":false}}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params["config"],
        json!({
            "sandbox_workspace_write": {
                "network_access": false,
                "writable_roots": ["/repos/org/repo/.git"],
            },
        })
    );
}

/// A user who already lists the worktree's git directory gets it once: the grant
/// is a union, not an append.
#[test]
fn an_already_listed_git_directory_is_not_added_twice() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write.writable_roots":["/repos/org/repo/.git"]}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(params["config"][GRANT_KEY], json!(["/repos/org/repo/.git"]));
}

/// A `writable_roots` that is not a list leaves Delta nothing to append to, so
/// the config passes through untouched and the server decides whether it is
/// legal. The stand-aside is logged, not silent.
#[test]
fn a_non_list_writable_roots_is_left_alone() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write.writable_roots":"/elsewhere"}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { GRANT_KEY: "/elsewhere" },
        })
    );
}

/// A `sandbox_workspace_write` stated as something other than a table is the
/// other shape with no list to reach: the grant cannot walk into it to find the
/// `writable_roots` leaf, so the config passes through untouched — Delta never
/// replaces a value the user wrote — and the stand-aside is logged.
#[test]
fn a_non_table_sandbox_workspace_write_is_left_alone() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option(
            "config",
            Some(r#"{"sandbox_workspace_write":"workspace-write"}"#),
        )],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": { "sandbox_workspace_write": "workspace-write" },
        })
    );
}

/// A single `config` launch option that is not an object at all (the registry
/// stores text, and the value is passed through unvalidated) is likewise left
/// for the server to judge: there is no object to grant a writable root inside.
#[test]
fn a_non_object_user_config_is_left_alone() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[option("config", Some("not-an-object"))],
        Some("/repos/org/repo"),
    )
    .expect("no delta-owned key is used");

    assert_eq!(
        params,
        json!({ "cwd": "/worktrees/org-repo-01", "config": "not-an-object" })
    );
}

/// The grant sees the **merged** config, not each selection: two `config` rows
/// in a worktree launch add up, and the git directory joins the roots they
/// between them state.
#[test]
fn two_config_options_merge_before_the_grant_unions_into_them() {
    let params = thread_start_params(
        "/worktrees/org-repo-01",
        &[
            option(
                "config",
                Some(r#"{"sandbox_workspace_write.writable_roots":["/elsewhere"]}"#),
            ),
            option("config", Some(r#"{"model_reasoning_effort":"high"}"#)),
        ],
        Some("/repos/org/repo"),
    )
    .expect("two config options merge");

    assert_eq!(
        params,
        json!({
            "cwd": "/worktrees/org-repo-01",
            "config": {
                "model_reasoning_effort": "high",
                GRANT_KEY: ["/elsewhere", "/repos/org/repo/.git"],
            },
        })
    );
}
