//! Tests for the `list_repositories` interactor.
//!
//! Use the fake store + fake `GitWorktree` to script `(origin URL, sessions)`
//! and assert the aggregated Repository tree round-trips correctly.

use crate::interactor::testing::*;
use delta_model::{AgentProvider, ContentBlock, Message, MessageUuid, Role, SessionId, ThreadId};

/// Build a minimal user-role message whose only purpose is to stamp a
/// session's `last_activity_at` to `created_at`. The other Message fields are
/// inert for the repository-tab tests.
fn mk_msg(session_id: &SessionId, thread_id: ThreadId, uuid: &str, created_at: &str) -> Message {
    Message {
        uuid: MessageUuid::from(uuid),
        session_id: session_id.clone(),
        thread_id,
        role: Role::User,
        linear_parent_uuid: None,
        semantic_parent_uuid: None,
        prompt_id: None,
        seq: 0,
        content_text: Some("hi".into()),
        content: vec![ContentBlock::Text { text: "hi".into() }],
        created_at: Some(created_at.into()),
        model: None,
        git_branch: None,
        cwd: None,
        response_time_ms: None,
        provider_item_id: None,
    }
}

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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
        )
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();

    assert_eq!(
        repos.len(),
        1,
        "a repo whose every clone is gone disappears"
    );
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
            None,
            AgentProvider::Claude,
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
    let git = FakeGitWorktree::default().with_origin_url(EXISTING_DIR, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR,
            Some("main"),
            Some(EXISTING_DIR),
            Some(EXISTING_DIR),
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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
            None,
            AgentProvider::Claude,
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

// Helper: register `subdir` as an empty git-style clone (a directory with a
// `.git` subdirectory) under `parent`, returning its canonical absolute path
// for assertions against the scan output (which canonicalises every entry).
async fn make_git_child(parent: &std::path::Path, name: &str) -> String {
    let child = parent.join(name);
    std::fs::create_dir(&child).unwrap();
    std::fs::create_dir(child.join(".git")).unwrap();
    tokio::fs::canonicalize(&child)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn scan_only_repo_surfaces_without_a_session() {
    // A scan root that contains a git clone the user has never launched a
    // session in: the clone shows up as a `last_opened_at: None` repository so
    // the PR tab's `has_local_clone` join picks it up (this is the whole
    // motivation behind Phase D's umbrella-session fix).
    let tmp = tempfile::tempdir().unwrap();
    let clone_path = make_git_child(tmp.path(), "delta").await;
    let git = FakeGitWorktree::default().with_origin_url(&clone_path, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);
    ix.store()
        .insert_repository_scan_root(tmp.path().to_str().unwrap())
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(
        repos.len(),
        1,
        "the scan-derived clone is the sole repository"
    );
    assert_eq!(repos[0].identity_key, "github.com/x7c1/delta");
    assert_eq!(repos[0].recently_used_clone_path, clone_path);
    assert_eq!(repos[0].clones.len(), 1);
    assert!(
        repos[0].clones[0].last_opened_at.is_none(),
        "a never-opened scan clone reports no recency"
    );
    assert!(
        repos[0].clones[0].last_branch.is_none(),
        "a never-opened scan clone reports no last branch"
    );
}

#[tokio::test]
async fn session_and_scan_clones_with_the_same_identity_key_union() {
    // The same repository surfaces from both the session history (one clone
    // path) and a scan root (a different clone path with the same origin):
    // the two collapse into one Repository with both clones, the session-
    // derived one keeping its recency and the scan-derived one carrying none.
    let tmp = tempfile::tempdir().unwrap();
    let scan_clone = make_git_child(tmp.path(), "delta-alt").await;
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR, "git@github.com:x7c1/delta")
        .with_origin_url(&scan_clone, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            EXISTING_DIR,
            Some("main"),
            Some(EXISTING_DIR),
            Some(EXISTING_DIR),
            None,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    ix.store()
        .insert_repository_scan_root(tmp.path().to_str().unwrap())
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(repos.len(), 1, "same origin = one repository");
    let repo = &repos[0];
    assert_eq!(repo.identity_key, "github.com/x7c1/delta");
    assert_eq!(repo.clones.len(), 2, "both clones unioned in");
    assert_eq!(
        repo.recently_used_clone_path, EXISTING_DIR,
        "the session-derived clone keeps its recency, so it wins the default"
    );
    let scan_clone_entry = repo
        .clones
        .iter()
        .find(|c| c.path == scan_clone)
        .expect("the scan-derived clone is present");
    assert!(
        scan_clone_entry.last_opened_at.is_none(),
        "the scan-derived clone reports no recency"
    );
}

#[tokio::test]
async fn scan_clone_already_in_session_history_is_not_added_twice() {
    // The user registered a scan root that points at a parent whose child is
    // the very dir they already launched sessions in. The same path must not
    // be double-counted: the session-derived row wins (carries the recency)
    // and the scan-derived hit is dropped by the de-dup guard.
    let tmp = tempfile::tempdir().unwrap();
    let clone_path = make_git_child(tmp.path(), "delta").await;
    let git = FakeGitWorktree::default().with_origin_url(&clone_path, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s1"),
            &clone_path,
            Some("main"),
            Some(&clone_path),
            Some(&clone_path),
            None,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    ix.store()
        .insert_repository_scan_root(tmp.path().to_str().unwrap())
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(
        repos[0].clones.len(),
        1,
        "the same path appearing in both sources still collapses to one clone"
    );
    assert!(
        repos[0].clones[0].last_opened_at.is_some(),
        "the surviving entry is the session-derived one (carries recency)"
    );
}

#[tokio::test]
async fn scan_root_with_no_git_children_contributes_nothing() {
    // A scan root that only contains plain (non-git) directories yields no
    // clones — the depth-1 scan looks for `.git` and nothing else.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("not-a-clone")).unwrap();
    let ix = interactor_with_git(FakeGitWorktree::default());
    ix.store()
        .insert_repository_scan_root(tmp.path().to_str().unwrap())
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert!(
        repos.is_empty(),
        "no .git children = nothing for the scan to add, got {repos:?}"
    );
}

#[tokio::test]
async fn missing_scan_root_path_is_skipped_silently() {
    // The user's scan-root parent has been removed from disk since
    // registration (or never existed): the call must not fail, the list just
    // misses what that root would have contributed.
    let ix = interactor_with_git(FakeGitWorktree::default());
    ix.store()
        .insert_repository_scan_root("/no/such/scan/root/path/anywhere")
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert!(
        repos.is_empty(),
        "a missing root simply contributes nothing"
    );
}

#[tokio::test]
async fn same_clone_path_with_different_repo_roots_dedups_keeping_newest() {
    // Two sessions share the same `clone_path = EXISTING_DIR` but with
    // different `repo_root` values (A and B). Both roots resolve to the same
    // origin URL, so the interactor groups them under one identity_key. With
    // the old SQL partitioning by `(repo_root, clone_path)` they would surface
    // as two clone entries with different `branch_at_launch` ("old" vs
    // "new"); the Rust-side path dedup must collapse them into one, keeping
    // the entry from the more recent session (branch = "new").
    let git = FakeGitWorktree::default()
        .with_origin_url(EXISTING_DIR_2, "git@github.com:x7c1/delta")
        .with_origin_url(EXISTING_DIR_3, "git@github.com:x7c1/delta");
    let ix = interactor_with_git(git);

    // Insert s_old first: its message stamps `last_activity_at` to the
    // earlier timestamp.
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s_old"),
            EXISTING_DIR,
            Some("old"),
            Some(EXISTING_DIR_2),
            Some(EXISTING_DIR),
            None,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    let s_old_thread = ix
        .store()
        .main_thread_id(&SessionId::from("s_old"))
        .await
        .unwrap();
    ix.store()
        .upsert_messages(&[mk_msg(
            &SessionId::from("s_old"),
            s_old_thread,
            "m-old",
            "2026-01-01T00:00:00Z",
        )])
        .await
        .unwrap();

    // Then s_new: more recent activity, distinct repo_root, same clone path.
    ix.store()
        .insert_spawning_session(
            &SessionId::from("s_new"),
            EXISTING_DIR,
            Some("new"),
            Some(EXISTING_DIR_3),
            Some(EXISTING_DIR),
            None,
            AgentProvider::Claude,
        )
        .await
        .unwrap();
    let s_new_thread = ix
        .store()
        .main_thread_id(&SessionId::from("s_new"))
        .await
        .unwrap();
    ix.store()
        .upsert_messages(&[mk_msg(
            &SessionId::from("s_new"),
            s_new_thread,
            "m-new",
            "2026-02-01T00:00:00Z",
        )])
        .await
        .unwrap();

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(
        repos.len(),
        1,
        "both repo_roots collapse to one identity_key"
    );
    let repo = &repos[0];
    let matches: Vec<_> = repo
        .clones
        .iter()
        .filter(|c| c.path == EXISTING_DIR)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "the shared clone_path appears exactly once after dedup, got {:?}",
        repo.clones,
    );
    assert_eq!(
        matches[0].last_branch.as_deref(),
        Some("new"),
        "the surviving entry is the most-recent one (branch = \"new\")",
    );
}

#[tokio::test]
async fn generated_paths_have_independent_cap_from_user_paths() {
    // For one repo, insert 3 user-picked paths (none under `worktree_base`)
    // and 12 generated paths (all children of `worktree_base`). The per-repo
    // caps are USER_CLONE_PATH_LIMIT = 5 and GENERATED_CLONE_PATH_LIMIT = 10,
    // so the result keeps all 3 user paths plus 10 generated paths (= 13
    // total). All paths are real on-disk directories so the lazy-GC stat
    // does not drop them.
    let tmp_worktrees = tempfile::tempdir().unwrap();
    let tmp_user = tempfile::tempdir().unwrap();
    let worktree_base = tmp_worktrees.path().to_string_lossy().into_owned();

    // 3 user-picked paths under the (separate) user tempdir.
    let mut user_paths = Vec::new();
    for i in 0..3 {
        let p = tmp_user.path().join(format!("user-{i:02}"));
        std::fs::create_dir(&p).unwrap();
        user_paths.push(p.to_string_lossy().into_owned());
    }
    // 12 generated paths under worktree_base.
    let mut generated_paths = Vec::new();
    for i in 0..12 {
        let p = tmp_worktrees.path().join(format!("delta-gen-{i:02}"));
        std::fs::create_dir(&p).unwrap();
        generated_paths.push(p.to_string_lossy().into_owned());
    }

    // One repo_root (a third tempdir, also user-picked) with the shared
    // origin. Every clone path resolves to the same origin URL, so they all
    // bundle into the same Repository.
    let repo_root = tmp_user.path().join("repo-root");
    std::fs::create_dir(&repo_root).unwrap();
    let repo_root = repo_root.to_string_lossy().into_owned();

    let mut git = FakeGitWorktree::default();
    for p in user_paths.iter().chain(generated_paths.iter()) {
        git = git.with_origin_url(p.as_str(), "git@github.com:x7c1/delta");
    }
    git = git.with_origin_url(repo_root.as_str(), "git@github.com:x7c1/delta");
    let ix = interactor_with_git_and_worktree_base(git, &worktree_base);

    let mut idx = 0;
    for p in &user_paths {
        ix.store()
            .insert_spawning_session(
                &SessionId::from(format!("s-user-{idx}").as_str()),
                p,
                Some("main"),
                Some(repo_root.as_str()),
                Some(p),
                None,
                AgentProvider::Claude,
            )
            .await
            .unwrap();
        idx += 1;
    }
    for p in &generated_paths {
        ix.store()
            .insert_spawning_session(
                &SessionId::from(format!("s-gen-{idx}").as_str()),
                p,
                Some("main"),
                Some(repo_root.as_str()),
                Some(p),
                None,
                AgentProvider::Claude,
            )
            .await
            .unwrap();
        idx += 1;
    }

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(repos.len(), 1, "one origin, one repository");
    let clones = &repos[0].clones;
    assert_eq!(
        clones.len(),
        13,
        "user cap (5) is not binding for 3 user paths, generated cap (10) clips 12 → 10, total 3 + 10 = 13 — got {clones:?}",
    );
    // Every user path survives: the user cap is 5, only 3 were inserted.
    for p in &user_paths {
        assert!(
            clones.iter().any(|c| &c.path == p),
            "the user-picked path {p} must reach the result (not squeezed out by the generated burst)",
        );
    }
    // Exactly 10 of the 12 generated paths survive.
    let kept_generated = clones
        .iter()
        .filter(|c| generated_paths.iter().any(|p| p == &c.path))
        .count();
    assert_eq!(kept_generated, 10, "the generated cap clips 12 → 10",);
}

#[tokio::test]
async fn active_repo_limit_drops_oldest_repositories() {
    // Insert ACTIVE_REPOSITORY_LIMIT + 1 distinct repo_roots, each with one
    // session whose activity timestamp grows monotonically by index. The
    // oldest repository (s-00) must be dropped wholesale by the active-repo
    // cap — its identity_key is absent from the result.
    use crate::interactor::repository::list_repositories::ACTIVE_REPOSITORY_LIMIT;
    let n = (ACTIVE_REPOSITORY_LIMIT as usize) + 1;

    let tmp = tempfile::tempdir().unwrap();
    // Build one on-disk clone path per repo so the lazy-GC stat keeps the rows.
    let mut paths = Vec::new();
    for i in 0..n {
        let p = tmp.path().join(format!("clone-{i:03}"));
        std::fs::create_dir(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }

    // Give each repo a distinct upstream so they stay separate Repositories,
    // and wire its origin URL so the interactor reaches it.
    let mut git = FakeGitWorktree::default();
    for (i, p) in paths.iter().enumerate() {
        git = git.with_origin_url(p.as_str(), format!("git@github.com:x7c1/r-{i:03}").as_str());
    }
    let ix = interactor_with_git(git);

    for (i, p) in paths.iter().enumerate() {
        let sid = SessionId::from(format!("s-{i:03}").as_str());
        ix.store()
            .insert_spawning_session(
                &sid,
                p,
                Some("main"),
                Some(p),
                Some(p),
                None,
                AgentProvider::Claude,
            )
            .await
            .unwrap();
        // Stamp a message timestamp that grows with i, so repo i is strictly
        // more recent than repo i-1. The 1970-base keeps the format short and
        // sortable.
        let thread = ix.store().main_thread_id(&sid).await.unwrap();
        ix.store()
            .upsert_messages(&[mk_msg(
                &sid,
                thread,
                format!("m-{i:03}").as_str(),
                format!("1970-01-{:02}T00:00:00Z", i + 1).as_str(),
            )])
            .await
            .unwrap();
    }

    let repos = ix.list_repositories().await.unwrap();
    assert_eq!(
        repos.len(),
        ACTIVE_REPOSITORY_LIMIT as usize,
        "the active-repo cap clips {n} → {ACTIVE_REPOSITORY_LIMIT}",
    );
    let oldest_key = format!("github.com/x7c1/r-{:03}", 0);
    assert!(
        repos.iter().all(|r| r.identity_key != oldest_key),
        "the oldest repository ({oldest_key}) must be absent, got {:?}",
        repos
            .iter()
            .map(|r| r.identity_key.as_str())
            .collect::<Vec<_>>(),
    );
}
