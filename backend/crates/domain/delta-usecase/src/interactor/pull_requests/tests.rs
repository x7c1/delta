//! Tests for `list_pull_requests`. The shared `FakeGhCli` scripts gh's
//! answers and the in-memory `SessionStore` + `FakeGitWorktree` script
//! the local-clone registry the use case joins against.

use std::sync::Arc;

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::pull_request::{PullRequest, PullRequestLens};

/// Paths the lazy-GC stat reliably resolves on every host. The store
/// derives a clone from these so the Repository tab's filter keeps them.
const EXISTING_DIR: &str = "/";
const EXISTING_DIR_2: &str = "/etc";

/// Build a PR fixture with sensible defaults so each test names only the
/// fields that drive the assertion.
fn fixture_pr(owner: &str, name: &str, head_ref: &str) -> PullRequest {
    PullRequest {
        number: 42,
        title: format!("PR for {owner}/{name}"),
        repo_owner: owner.into(),
        repo_name: name.into(),
        head_ref: head_ref.into(),
        head_repo_owner: owner.into(),
        head_repo_name: name.into(),
        draft: false,
        url: format!("https://github.com/{owner}/{name}/pull/42"),
        updated_at: "2026-06-24T00:00:00Z".into(),
        author_login: "x7c1".into(),
        has_local_clone: false,
    }
}

#[tokio::test]
async fn gh_unavailable_returns_empty_with_the_flag_off() {
    // Missing/unauthenticated gh: empty list, gh_available = false. The
    // PR endpoint must never 5xx because gh is absent — the UI surfaces
    // the warning inline.
    let ix = interactor_with_git_and_gh(
        FakeGitWorktree::default(),
        Arc::new(FakeGhCli::unauthenticated()),
    );
    let list = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    assert!(!list.gh_available, "gh is reported as unavailable");
    assert!(
        list.pull_requests.is_empty(),
        "no PRs are returned when gh is unavailable, got {:?}",
        list.pull_requests
    );
}

#[tokio::test]
async fn has_local_clone_is_set_only_for_registered_repos() {
    // Two PRs: x7c1/delta has a registered clone (the session history
    // records a session under EXISTING_DIR whose origin maps to it),
    // x7c1/other has none. The first row's `has_local_clone` is true,
    // the second's is false — exactly the gate the UI uses.
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR, "https://github.com/x7c1/delta.git");
    let prs = vec![
        fixture_pr("x7c1", "delta", "feat/x"),
        fixture_pr("x7c1", "other", "feat/y"),
    ];
    let gh = Arc::new(FakeGhCli::authenticated(prs, Vec::new()));
    let ix = interactor_with_git_and_gh(git, gh);

    // Seed one session so the Repository aggregation finds the clone.
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR,
            Some("main"),
            Some(EXISTING_DIR),
            Some(EXISTING_DIR),
        )
        .await
        .unwrap();

    let list = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    assert!(list.gh_available);
    assert_eq!(list.pull_requests.len(), 2);

    let delta_pr = list
        .pull_requests
        .iter()
        .find(|pr| pr.repo_name == "delta")
        .expect("delta PR present");
    assert!(
        delta_pr.has_local_clone,
        "x7c1/delta has a registered local clone via the seeded session"
    );

    let other_pr = list
        .pull_requests
        .iter()
        .find(|pr| pr.repo_name == "other")
        .expect("other PR present");
    assert!(
        !other_pr.has_local_clone,
        "x7c1/other has no registered local clone"
    );
}

#[tokio::test]
async fn lens_selects_the_matching_gh_query() {
    // The reviewer and author result sets are largely disjoint; the use
    // case must shell `gh search prs` with the lens that was asked for,
    // not (silently) the other one.
    let reviewer_prs = vec![fixture_pr("x7c1", "delta", "feat/review")];
    let author_prs = vec![fixture_pr("x7c1", "other", "feat/mine")];
    let gh = Arc::new(FakeGhCli::authenticated(reviewer_prs, author_prs));
    let ix = interactor_with_git_and_gh(FakeGitWorktree::default(), gh);

    let reviewer = ix
        .list_pull_requests(PullRequestLens::Reviewer)
        .await
        .unwrap();
    assert_eq!(reviewer.pull_requests.len(), 1);
    assert_eq!(reviewer.pull_requests[0].head_ref, "feat/review");

    let author = ix
        .list_pull_requests(PullRequestLens::Author)
        .await
        .unwrap();
    assert_eq!(author.pull_requests.len(), 1);
    assert_eq!(author.pull_requests[0].head_ref, "feat/mine");
}

#[tokio::test]
async fn search_results_are_memoised_per_lens() {
    // Within the cache TTL, repeated calls for the same lens must not
    // re-shell to `gh search prs`. The two lenses cache independently.
    let gh = Arc::new(FakeGhCli::authenticated(
        vec![fixture_pr("x7c1", "delta", "feat/x")],
        vec![fixture_pr("x7c1", "other", "feat/y")],
    ));
    let ix = interactor_with_git_and_gh(FakeGitWorktree::default(), gh.clone());

    let _ = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    let _ = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    let _ = ix.list_pull_requests(PullRequestLens::Author).await.unwrap();
    // Reviewer (1 miss + 1 hit) + author (1 miss) = 2 shell-outs.
    assert_eq!(
        gh.search_calls(),
        2,
        "the second reviewer call is served from the cache; the author \
         call is a separate miss",
    );
}

#[tokio::test]
async fn scan_only_repos_satisfy_the_local_clone_check() {
    // The umbrella-session motivation: the user has never launched a session
    // in `<atelier>/repos/x7c1/zatto`, but a scan root is registered at
    // `<atelier>/repos/x7c1` and the child `.git` makes the clone discoverable.
    // The PR tab must report `has_local_clone: true` for that PR even though
    // no session row points at the sub-repo.
    let tmp = tempfile::tempdir().unwrap();
    let zatto_path = {
        let dir = tmp.path().join("zatto");
        std::fs::create_dir(&dir).unwrap();
        std::fs::create_dir(dir.join(".git")).unwrap();
        tokio::fs::canonicalize(&dir).await.unwrap().to_string_lossy().into_owned()
    };
    let git = FakeGitWorktree::default()
        .with_origin_url(&zatto_path, "git@github.com:x7c1/zatto");
    let prs = vec![fixture_pr("x7c1", "zatto", "feat/x")];
    let gh = Arc::new(FakeGhCli::authenticated(prs, Vec::new()));
    let ix = interactor_with_git_and_gh(git, gh);
    ix.store()
        .insert_repository_scan_root(tmp.path().to_str().unwrap())
        .await
        .unwrap();

    let list = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    assert_eq!(list.pull_requests.len(), 1);
    assert!(
        list.pull_requests[0].has_local_clone,
        "a scan-derived clone counts as a registered local clone"
    );
}

#[tokio::test]
async fn path_keyed_repos_do_not_satisfy_the_local_clone_check() {
    // A session in a dir whose `origin` is unset is keyed by its path
    // (e.g. `/etc`); a `gh` PR could never collide with that key, so
    // `has_local_clone` must stay false even when such a clone is
    // registered.
    let git = FakeGitWorktree::default();
    // No origin URL scripted for EXISTING_DIR_2, so the aggregation
    // keeps it as a path-keyed entry.
    let prs = vec![fixture_pr("x7c1", "delta", "feat/x")];
    let gh = Arc::new(FakeGhCli::authenticated(prs, Vec::new()));
    let ix = interactor_with_git_and_gh(git, gh);
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR_2,
            Some("main"),
            Some(EXISTING_DIR_2),
            Some(EXISTING_DIR_2),
        )
        .await
        .unwrap();

    let list = ix.list_pull_requests(PullRequestLens::Reviewer).await.unwrap();
    assert_eq!(list.pull_requests.len(), 1);
    assert!(
        !list.pull_requests[0].has_local_clone,
        "a path-keyed clone never satisfies the `github.com/<owner>/<name>` lookup"
    );
}
