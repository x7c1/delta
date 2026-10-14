//! Commit lookups and tree comparisons for the launch-trust check.

use super::*;

/// Write `contents` to `rel` under `repo` (creating parent directories).
async fn write_file(repo: &str, rel: &str, contents: &str) {
    let path = std::path::Path::new(repo).join(rel);
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, contents).await.unwrap();
}

/// Commit everything in `repo` with `message`, returning the new commit id.
async fn commit_all(repo: &str, message: &str) -> String {
    git_ok(repo, &["add", "-A"]).await;
    git_ok(repo, &["commit", "-q", "-m", message]).await;
    git_ok(repo, &["rev-parse", "HEAD"]).await
}

/// An origin repository whose `main` holds agent configuration, plus a clone
/// of it (the user's repository); returns `(origin dir, clone path, main's
/// commit)`. The tempdirs are returned so they outlive the test body.
async fn origin_and_clone() -> (tempfile::TempDir, tempfile::TempDir, String, String) {
    let origin = tempfile::tempdir().unwrap();
    init_repo_with_commit(origin.path()).await;
    let origin_path = origin.path().to_str().unwrap().to_owned();
    write_file(&origin_path, ".claude/settings.json", "{\n  \"a\": 1\n}\n").await;
    write_file(&origin_path, "CLAUDE.md", "root\n").await;
    write_file(&origin_path, "src/main.rs", "fn main() {}\n").await;
    let main = commit_all(&origin_path, "config").await;

    let clone_parent = tempfile::tempdir().unwrap();
    let clone_path = clone_parent
        .path()
        .join("clone")
        .to_string_lossy()
        .into_owned();
    git_ok(&origin_path, &["clone", "-q", &origin_path, &clone_path]).await;
    (origin, clone_parent, clone_path, main)
}

/// A branch that changes `.claude/` (and other things) is reported path by
/// path, every changed path is listed, and each path's diff is git's unified
/// rendering of exactly that file.
#[tokio::test]
async fn a_branch_changing_claude_config_reports_its_paths_and_diff() {
    let (origin, _clone_parent, clone_path, main) = origin_and_clone().await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["checkout", "-q", "-b", "feature"]).await;
    write_file(origin_path, ".claude/settings.json", "{\n  \"a\": 2\n}\n").await;
    write_file(origin_path, ".claude/hooks/new.sh", "echo hi\n").await;
    write_file(origin_path, "src/main.rs", "fn main() { }\n").await;
    let feature = commit_all(origin_path, "change config").await;
    git_ok(origin_path, &["checkout", "-q", "main"]).await;

    let git = Git::new(Vec::new());
    let launch = git
        .launch_commit(
            &clone_path,
            &WorktreeStartPoint::RemoteBranch("feature".to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(launch, feature, "the fetched origin/feature tip");
    let head = git.head_commit(&clone_path).await.unwrap();
    assert_eq!(head, main, "the clone is on main");

    let mut paths = git
        .changed_paths(&clone_path, &head, &launch)
        .await
        .unwrap();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            ".claude/hooks/new.sh",
            ".claude/settings.json",
            "src/main.rs"
        ]
    );

    let diff = git
        .file_diff(&clone_path, &head, &launch, ".claude/settings.json")
        .await
        .unwrap();
    assert!(
        diff.contains("diff --git a/.claude/settings.json b/.claude/settings.json"),
        "{diff}"
    );
    assert!(
        diff.contains("-  \"a\": 1") && diff.contains("+  \"a\": 2"),
        "{diff}"
    );
    assert!(!diff.contains("src/main.rs"), "only the one path is diffed");

    let added = git
        .file_diff(&clone_path, &head, &launch, ".claude/hooks/new.sh")
        .await
        .unwrap();
    assert!(
        added.contains("new file mode") && added.contains("+echo hi"),
        "{added}"
    );
}

/// A branch that leaves every file alone reports nothing, and a path whose
/// name contains glob characters is matched literally.
#[tokio::test]
async fn an_unchanged_branch_reports_no_paths_and_paths_are_literal() {
    let (_origin, _clone_parent, clone_path, main) = origin_and_clone().await;
    let git = Git::new(Vec::new());

    assert!(git
        .changed_paths(&clone_path, &main, &main)
        .await
        .unwrap()
        .is_empty());
    let diff = git.file_diff(&clone_path, &main, &main, "*").await.unwrap();
    assert!(diff.is_empty(), "`*` names no file literally, got {diff}");
}

/// `use_remote_branch` resolves to the local branch when one exists — the
/// build checks it out rather than the remote tip — and to the fetched remote
/// tip otherwise; `head` is the repository's current `HEAD`.
#[tokio::test]
async fn launch_commit_follows_what_the_build_checks_out() {
    let (origin, _clone_parent, clone_path, main) = origin_and_clone().await;
    let origin_path = origin.path().to_str().unwrap();
    git_ok(origin_path, &["checkout", "-q", "-b", "topic"]).await;
    write_file(origin_path, ".mcp.json", "{}\n").await;
    let remote_tip = commit_all(origin_path, "mcp").await;

    let git = Git::new(Vec::new());
    assert_eq!(
        git.launch_commit(&clone_path, &WorktreeStartPoint::Head)
            .await
            .unwrap(),
        main
    );
    let use_topic = WorktreeStartPoint::UseRemoteBranch("topic".to_owned());
    assert_eq!(
        git.launch_commit(&clone_path, &use_topic).await.unwrap(),
        remote_tip,
        "no local branch yet: the fetched remote tip"
    );

    // A local `topic` that lags the remote is what the build would check out.
    git_ok(&clone_path, &["branch", "topic", &main]).await;
    assert_eq!(
        git.launch_commit(&clone_path, &use_topic).await.unwrap(),
        main,
        "an existing local branch is checked out as it is"
    );
}
