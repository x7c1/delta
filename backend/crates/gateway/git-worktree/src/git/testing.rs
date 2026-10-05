//! Helpers shared by the `git` module's tests: a throwaway repository and
//! plain `git` invocations that assert success.

use tokio::process::Command;

use super::trimmed_stdout;

/// Initialize a git repository at `dir` with one commit, so it has a `HEAD`
/// to branch off, and a deterministic identity/branch name regardless of the
/// host's git config.
pub(super) async fn init_repo_with_commit(dir: &std::path::Path) {
    let dir = dir.to_str().unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["commit", "-q", "--allow-empty", "-m", "initial"],
    ] {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(&args)
            .output()
            .await
            .expect("git available");
        assert!(
            status.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

/// Run `git -C <dir> <args>`, asserting success, returning trimmed stdout.
pub(super) async fn git_ok(dir: &str, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .expect("git available");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    trimmed_stdout(&output)
}

/// Canonicalize a path so comparisons survive symlink resolution (macOS
/// `/var` → `/private/var`).
pub(super) async fn canonical(path: &str) -> String {
    tokio::fs::canonicalize(path)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned()
}
