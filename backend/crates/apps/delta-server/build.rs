//! Build-time git sha probe.
//!
//! Records the short git sha of the current checkout into `DELTA_GIT_SHA` so
//! debug builds can embed it in the version string surfaced to the browser
//! (`v0.2.1+dev.a1b2c3d`). Falls back to the literal `unknown` if `git
//! rev-parse` is unavailable (e.g. a tarball extract) so a source-only build
//! never fails here.
//!
//! Both the repository's `HEAD` file and its HEAD reflog (`logs/HEAD`) are
//! registered as `rerun-if-changed` inputs. `HEAD` alone is not enough: it
//! holds `ref: refs/heads/<branch>` and is rewritten only on a branch switch,
//! so a commit or a fast-forward `git pull` on the same branch left it
//! byte-identical and cargo reused the cached script output — recompiling the
//! crate with the new sources while embedding a stale sha. The reflog closes
//! that gap: every operation that moves `HEAD` appends a line to it, and
//! unlike the ref file `HEAD` points at, `git pack-refs` never folds it into
//! `packed-refs`.
//!
//! A dirty working tree still reports the sha of the last commit; the sha is
//! a debugging hint, not a fingerprint.
//!
//! Both paths must come from `git rev-parse --git-path` at build time:
//! relative `rerun-if-changed` paths resolve against the crate directory, but
//! the git directory lives at the repository root (or elsewhere entirely in a
//! linked worktree, where `--git-path` returns the per-worktree file).
//! Registering a path that does not exist makes cargo treat the crate as
//! permanently stale and rebuild it on every invocation, and `--git-path`
//! answers with a path whether or not the file is there — the reflog is
//! absent under `core.logAllRefUpdates=false`, and in a fresh repository
//! before its first commit — so each path is registered only once it is
//! confirmed to exist. Where the reflog is missing only `HEAD` is registered
//! and the sha can lag again exactly as described above; the build itself
//! never fails over it.

fn main() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=DELTA_GIT_SHA={sha}");
    for git_path in ["HEAD", "logs/HEAD"] {
        let Some(resolved) = git(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            git_path,
        ]) else {
            continue;
        };
        if std::path::Path::new(&resolved).exists() {
            println!("cargo:rerun-if-changed={resolved}");
        }
    }
}
