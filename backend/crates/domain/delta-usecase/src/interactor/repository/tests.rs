//! Tests for the `list_repositories` interactor.
//!
//! Use the fake store + fake `GitWorktree` to script `(origin URL, sessions)`
//! and assert the aggregated Repository tree round-trips correctly.

use crate::interactor::testing::*;
use delta_model::SessionId;

/// A path the lazy-GC stat will reliably succeed on — `/` exists on every
/// platform delta runs on, so a row pointing at it survives the filter.
const EXISTING_DIR: &str = "/";
/// A second path that exists on every platform we run on — used when a test
/// needs two distinct existing paths.
const EXISTING_DIR_2: &str = "/etc";
/// A third path that exists on every platform we run on.
const EXISTING_DIR_3: &str = "/tmp";
/// A path that definitely does not exist, used to exercise the lazy-GC
/// filter.
const MISSING_DIR: &str = "/no/such/delta/clone/path";

#[tokio::test]
async fn repositories_bundle_clones_with_the_same_origin() {
    // Two clones of the same repo (different repo_roots on disk, identical
    // origin URL) collapse into a single Repository with two clones; a third
    // clone of a different upstream stays its own Repository.
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR, "git@github.com:x7c1/delta")
        .with_origin_url(EXISTING_DIR_2, "https://github.com/x7c1/delta.git")
        .with_origin_url(EXISTING_DIR_3, "git@github.com:x7c1/other.git");
    let ix = interactor_with_git(git);

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
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s2"),
            EXISTING_DIR_2,
            Some("feature/x"),
            Some(EXISTING_DIR_2),
            Some(EXISTING_DIR_2),
        )
        .await
        .unwrap();
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s3"),
            EXISTING_DIR_3,
            Some("main"),
            Some(EXISTING_DIR_3),
            Some(EXISTING_DIR_3),
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();

    assert_eq!(repos.len(), 2, "two distinct upstreams = two repositories");

    let delta = repos
        .iter()
        .find(|r| r.identity_key == "github.com/x7c1/delta")
        .expect("the bundled repo is present");
    assert_eq!(delta.display_name, "x7c1/delta");
    assert_eq!(
        delta.clones.len(),
        2,
        "ssh + https forms collapse into one repo with two clones"
    );
    // All seeded sessions share the same `created_at` in the fake, so the
    // recency tie-break inside `list_repositories` is the input order: the
    // first row wins. The store sorts by `(recency DESC, repo_root ASC,
    // clone_path ASC)`, so among the delta clones EXISTING_DIR (`/`) precedes
    // EXISTING_DIR_2 (`/etc`) and becomes the default.
    assert_eq!(
        delta.recently_used_clone_path, EXISTING_DIR,
        "the first-by-input-order bundled clone wins the recency tie"
    );

    let other = repos
        .iter()
        .find(|r| r.identity_key == "github.com/x7c1/other")
        .expect("the second repo is present");
    assert_eq!(other.clones.len(), 1);
    assert_eq!(other.clones[0].path, EXISTING_DIR_3);
}

#[tokio::test]
async fn clones_without_origin_stand_alone_by_path() {
    // A clone whose `git config --get remote.origin.url` reports nothing
    // (the fake's default for unknown paths) falls back to identity_key =
    // the clone path itself, so two such clones never collapse.
    let ix = interactor_with_git(FakeGitWorktree::default());
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR,
            Some("main"),
            Some(EXISTING_DIR),
            None,
        )
        .await
        .unwrap();
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s2"),
            EXISTING_DIR_2,
            Some("main"),
            Some(EXISTING_DIR_2),
            None,
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();

    assert_eq!(repos.len(), 2, "no shared origin = no bundling");
    let keys: Vec<&str> = repos.iter().map(|r| r.identity_key.as_str()).collect();
    assert!(keys.contains(&EXISTING_DIR));
    assert!(keys.contains(&EXISTING_DIR_2));
}

#[tokio::test]
async fn lazy_gc_drops_clones_whose_paths_no_longer_exist() {
    // The first clone is fine (its path exists); the second's path is
    // gone, so the GC filters it out — and because every clone of the
    // second repository was filtered, the repository itself disappears.
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR, "git@github.com:x7c1/delta")
        .with_origin_url(MISSING_DIR, "git@github.com:x7c1/gone");
    let ix = interactor_with_git(git);
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
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s2"),
            MISSING_DIR,
            Some("main"),
            Some(MISSING_DIR),
            Some(MISSING_DIR),
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();

    assert_eq!(repos.len(), 1, "a repo whose every clone is gone disappears");
    assert_eq!(repos[0].identity_key, "github.com/x7c1/delta");
    assert_eq!(repos[0].clones.len(), 1);
    assert_eq!(repos[0].clones[0].path, EXISTING_DIR);
}

#[tokio::test]
async fn sessions_outside_a_git_repo_never_contribute() {
    // A session whose `repo_root` is NULL is launched outside any git
    // repo, so it never appears in the Repository tab — Recent (Directory
    // tab) is where those surface.
    let ix = interactor_with_git(FakeGitWorktree::default());
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR,
            None,
            None,
            Some(EXISTING_DIR),
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert!(
        repos.is_empty(),
        "no git sessions contributed, expected empty list got {repos:?}"
    );
}

#[tokio::test]
async fn deferred_per_clone_fields_are_empty_by_default() {
    // Phase B does not persist per-session launch options or worktree
    // state, so every clone reports the deferred fields as empty/false.
    // This test pins that contract so a future PR adding the persistence
    // also flips the assertion intentionally.
    let git =
        FakeGitWorktree::default().with_origin_url(EXISTING_DIR, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);
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

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(repos.len(), 1);
    let clone = &repos[0].clones[0];
    assert!(clone.last_launch_option_ids.is_empty());
    assert!(!clone.last_worktree_enabled);
    assert!(clone.last_worktree_start_point.is_none());
}

#[tokio::test]
async fn recency_ordering_uses_max_across_a_repos_clones() {
    // Two repos, three clones. Repo A has clones with activity at
    // 2026-01-01 and 2026-01-03; Repo B has one at 2026-01-02. The list
    // is ordered by each repo's most recent clone, so A comes first.
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR, "git@github.com:x7c1/a")
        .with_origin_url(EXISTING_DIR_2, "git@github.com:x7c1/a")
        .with_origin_url(EXISTING_DIR_3, "git@github.com:x7c1/b");
    let ix = interactor_with_git(git);
    // The store stamps `created_at` itself, so the in-test recency
    // ordering is by insertion order: insert s_old, then the b-clone,
    // then s_new. The repo bundling A wins because its newest clone
    // (s_new) is the latest insertion.
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s_old"),
            EXISTING_DIR_2,
            Some("main"),
            Some(EXISTING_DIR_2),
            Some(EXISTING_DIR_2),
        )
        .await
        .unwrap();
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s_b"),
            EXISTING_DIR_3,
            Some("main"),
            Some(EXISTING_DIR_3),
            Some(EXISTING_DIR_3),
        )
        .await
        .unwrap();
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s_new"),
            EXISTING_DIR,
            Some("main"),
            Some(EXISTING_DIR),
            Some(EXISTING_DIR),
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(repos.len(), 2);
    assert_eq!(
        repos[0].identity_key, "github.com/x7c1/a",
        "the repo whose most-recent clone is newest wins"
    );
    assert_eq!(
        repos[0].clones[0].path, EXISTING_DIR,
        "the latest clone is the default"
    );
    assert_eq!(repos[0].recently_used_clone_path, EXISTING_DIR);
}
