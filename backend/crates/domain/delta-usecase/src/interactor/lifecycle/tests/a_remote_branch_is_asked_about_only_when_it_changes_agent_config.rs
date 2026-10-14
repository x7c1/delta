use crate::ports::WorktreeStartPoint;

use super::launch_trust_support::*;

/// A PR branch that touches no agent configuration — the common case — is
/// pre-accepted without asking, however much else it changes.
#[tokio::test]
async fn a_branch_that_leaves_agent_config_unchanged_launches_without_asking() {
    let git = repository().with_branch_commit(
        "pr-head",
        "pr0001",
        &[
            ("src/main.rs", "@@\n-a\n+b\n"),
            ("docs/README.md", "@@\n+x\n"),
        ],
    );
    let (ix, _sink) = interactor_over(git);
    let start = WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned());

    let check = check(&ix, &start).await;
    assert!(!check.needs_confirmation(), "nothing to ask, got {check:?}");
    assert_eq!(check.commit.as_deref(), Some("pr0001"));

    let (_, path) = launch(&ix, start, None).await;
    assert_eq!(pre_accepted(&ix), vec![path]);
}

/// Each kind of agent configuration a branch can change — the settings file,
/// a file under `.claude/` the trusted tree does not have, the top-level
/// `.mcp.json`, a nested `CLAUDE.md` — makes the launch ask, listing the
/// changed path with its diff, and a launch without the confirmation is not
/// pre-accepted.
#[tokio::test]
async fn a_branch_that_changes_agent_config_asks_with_the_diff() {
    for changed in [
        ".claude/settings.json",
        ".claude/a-new-config-file.json",
        ".mcp.json",
        "packages/app/CLAUDE.md",
    ] {
        let diff = format!("diff --git a/{changed} b/{changed}\n@@\n-old\n+new\n");
        let git = repository().with_branch_commit(
            "feature",
            "feat0001",
            &[("src/lib.rs", "@@\n+y\n"), (changed, &diff)],
        );
        let (ix, mut sink) = interactor_over(git);
        let start = WorktreeStartPoint::RemoteBranch("feature".to_owned());

        let check = check(&ix, &start).await;
        assert!(check.needs_confirmation(), "{changed} asks");
        assert!(!check.repository_untrusted, "the repository is trusted");
        assert_eq!(check.commit.as_deref(), Some("feat0001"));
        let paths: Vec<&str> = check
            .agent_config_changes
            .iter()
            .map(|c| c.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![changed],
            "only the agent configuration is listed"
        );
        assert_eq!(check.agent_config_changes[0].diff, diff);
        assert!(!check.agent_config_changes[0].diff_truncated);

        launch(&ix, start, None).await;
        assert_stopped_for_confirmation(&ix, &mut sink);
    }
}

/// Confirming the changed agent configuration for the commit the check
/// reported pre-accepts the worktree built on that commit.
#[tokio::test]
async fn a_confirmed_agent_config_change_launches() {
    let git = repository().with_branch_commit(
        "pr-head",
        "pr0002",
        &[(".claude/settings.json", "@@\n-a\n+b\n")],
    );
    let (ix, _sink) = interactor_over(git);
    let start = WorktreeStartPoint::UseRemoteBranch("pr-head".to_owned());
    let commit = check(&ix, &start).await.commit.unwrap();

    let (_, path) = launch(&ix, start, Some(&commit)).await;

    assert_eq!(pre_accepted(&ix), vec![path]);
    assert!(
        recorded_repositories(&ix).is_empty(),
        "the repository was already trusted"
    );
    assert_eq!(ix.tmux_fake().created.lock().unwrap().len(), 1);
}
